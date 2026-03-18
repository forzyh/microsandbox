//! OCI 镜像层提取模块
//!
//! 本模块实现了从下载的 tar.gz 文件中提取镜像层的功能。
//!
//! ## 层提取的挑战
//!
//! OCI 镜像层是叠加的，每个层可能：
//! 1. 添加新文件
//! 2. 修改现有文件（覆盖父层的版本）
//! 3. 删除文件（使用 whiteout 文件标记）
//!
//! 提取当前层时，如果某个文件的父目录不存在于当前层，
//! 需要从父层中复制目录结构和元数据。
//!
//! ## 提取流程
//!
//! ```text
//! extract_tar_with_ownership_override()
//!   │
//!   ├─> 1. 遍历 tar 中的所有条目
//!   │     │
//!   │     ├─ 硬链接 ──> 收集到列表，稍后处理
//!   │     ├─ 其他类型 ──> 调用 unpack() 提取
//!   │
//!   ├─> 2. 对于非 symlink 条目：
//!   │     ├─ 设置文件权限（目录至少 u+rwx，文件至少 u+rw）
//!   │     └─ 存储原始 uid/gid/mode 到 xattr
//!   │
//!   └─> 3. 处理硬链接列表
//! ```
//!
//! ## 关键概念
//!
//! ### xattr（扩展属性）
//!
//! 由于 macOS 和 Linux 的用户/组 ID 可能不同，需要保存原始的 uid/gid/mode。
//! 使用 `user.containers.override_stat` xattr 存储：`"uid:gid:0mode"` 格式。
//!
//! ### 硬链接的两阶段处理
//!
//! 硬链接必须在目标文件创建之后才能创建，所以：
//! 1. 第一阶段：收集所有硬链接信息
//! 2. 第二阶段：创建硬链接并设置 xattr
//!
//! ### 父层目录复制
//!
//! 如果当前层的文件路径是 `a/b/c/file.txt`，但 `a/b/c/` 目录不在当前层：
//! 1. 遍历祖先目录（`a`, `a/b`, `a/b/c`）
//! 2. 在父层中搜索每个祖先目录
//! 3. 复制找到的目录及其 xattr 和权限

use std::{
    ffi::{CStr, CString},
    io::ErrorKind,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
};

use anyhow::anyhow;
use futures::StreamExt;
use tokio::{
    fs::{self, DirBuilder},
    io::AsyncRead,
};
use tokio_tar::{Archive, Entry};

use crate::{MicrosandboxError, MicrosandboxResult, oci::LayerDependencies};

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// 获取完整的模式位（包含文件类型位）
///
/// 在 Unix 系统中，文件权限模式包含：
/// - 文件类型位（高 4 位）：标识文件是普通文件、目录、symlink 等
/// - 权限位（低 12 位）：rwxrwxrwx 权限
///
/// ## 参数
///
/// * `entry_type` - tar 条目中的文件类型
/// * `permission_bits` - 权限位（不包含文件类型）
///
/// ## 返回值
///
/// 返回完整的模式值，包含文件类型位和权限位
///
/// ## 文件类型常量
///
/// | 类型 | libc 常量 | 说明 |
/// |------|----------|------|
/// | 普通文件 | `S_IFREG` | 常规文件 |
/// | 目录 | `S_IFDIR` | 目录文件 |
/// | 符号链接 | `S_IFLNK` | 软链接 |
/// | 块设备 | `S_IFBLK` | 块设备文件 |
/// | 字符设备 | `S_IFCHR` | 字符设备文件 |
/// | FIFO | `S_IFIFO` | 命名管道 |
///
/// ## 为什么需要此函数？
///
/// tar 文件头中的模式只包含权限位，不包含文件类型位。
/// 在设置 xattr 时，需要保存完整的模式以便后续恢复。
#[allow(clippy::unnecessary_cast)] // libc::S_IF* types differ between platforms (u16 on macOS, u32 on Linux)
fn get_full_mode(entry_type: &tokio_tar::EntryType, permission_bits: u32) -> u32 {
    let file_type_bits = if entry_type.is_file() {
        libc::S_IFREG as u32
    } else if entry_type.is_dir() {
        libc::S_IFDIR as u32
    } else if entry_type.is_symlink() {
        libc::S_IFLNK as u32
    } else if entry_type.is_block_special() {
        libc::S_IFBLK as u32
    } else if entry_type.is_character_special() {
        libc::S_IFCHR as u32
    } else if entry_type.is_fifo() {
        libc::S_IFIFO as u32
    } else {
        0 // Unknown type
    };

    file_type_bits | permission_bits
}

/// 设置 xattr 以存储统计信息
///
/// 此函数将文件的原始 uid/gid/mode 存储到扩展属性（xattr）中。
/// 格式：`"uid:gid:0mode"`（例如：`"1000:1000:0755"`）
///
/// ## 参数
///
/// * `path` - 文件路径
/// * `xattr_name` - xattr 名称（如 `user.containers.override_stat`）
/// * `uid` - 用户 ID
/// * `gid` - 组 ID
/// * `mode` - 完整模式（包含文件类型位）
///
/// ## 跨平台差异
///
/// macOS 和 Linux 的 `setxattr` 系统调用签名不同：
/// - **macOS**: 需要 `position` 参数（通常为 0）
/// - **Linux**: 需要 `flags` 参数
///
/// ## 错误处理
///
/// - 如果文件系统不支持 xattr（`ENOTSUP`），记录警告并继续
/// - 其他错误会返回 `LayerExtraction` 错误
///
/// ## 为什么使用 xattr？
///
/// 在 macOS 上运行 Linux 容器镜像时：
/// - macOS 和 Linux 的 uid/gid 映射可能不同
/// - 需要保存原始的 uid/gid 以便在 VM 中正确恢复
/// - xattr 提供了不修改文件本身的存储方式
fn set_stat_xattr(
    path: &Path,
    xattr_name: &CStr,
    uid: u64,
    gid: u64,
    mode: u32,
) -> Result<(), MicrosandboxError> {
    use std::ffi::CString;

    let stat_data = format!("{}:{}:0{:o}", uid, gid, mode);
    let path_cstring = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|e| MicrosandboxError::LayerExtraction(format!("Invalid path: {:?}", e)))?;

    let result = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::setxattr(
                path_cstring.as_ptr(),
                xattr_name.as_ptr(),
                stat_data.as_ptr() as *const libc::c_void,
                stat_data.len(),
                0, // position parameter for macOS
                0, // options
            )
        }
        #[cfg(target_os = "linux")]
        {
            libc::setxattr(
                path_cstring.as_ptr(),
                xattr_name.as_ptr(),
                stat_data.as_ptr() as *const libc::c_void,
                stat_data.len(),
                0, // flags
            )
        }
    };

    if result != 0 {
        let errno = std::io::Error::last_os_error();
        if errno.raw_os_error() == Some(libc::ENOTSUP) {
            tracing::warn!(
                "Filesystem does not support xattrs for {}, continuing without stat shadowing",
                path.display()
            );
        } else {
            return Err(MicrosandboxError::LayerExtraction(format!(
                "Failed to set xattr on {}: {}",
                path.display(),
                errno
            )));
        }
    }
    Ok(())
}

/// 从下载的 tar.gz 文件中提取层到提取目录
///
/// 这是层提取的核心函数，支持：
/// 1. 在提取过程中修改文件所有权
/// 2. 从父层复制缺失的祖先目录
/// 3. 保存原始 uid/gid/mode 到 xattr
/// 4. 两阶段处理硬链接
///
/// ## 参数
///
/// * `archive` - tar archive（已解压 gzip）
/// * `extract_dir` - 提取目标目录
/// * `parent_layers` - 父层依赖关系（用于复制缺失的目录）
///
/// ## 返回值
///
/// * `Ok(())` - 提取成功
/// * `Err(MicrosandboxError)` - 提取失败
///
/// ## 提取流程
///
/// ```text
/// extract_tar_with_ownership_override()
///   │
///   ├─> 1. 初始化 xattr 名称缓存
///   │
///   ├─> 2. 创建硬链接收集器
///   │
///   ├─> 3. 遍历 tar 中的所有条目
///   │     │
///   │     ├─ 获取条目信息（路径、uid、gid、mode、类型）
///   │     │
///   │     ├─ 硬链接 ──> 收集到列表，稍后处理
///   │     │
///   │     ├─ 其他类型 ──> 调用 unpack() 提取
///   │           │
///   │           └─ 如果父目录不存在，从父层复制
///   │
///   ├─> 4. 对于非 symlink 条目：
///   │     ├─ 获取当前元数据
///   │     ├─ 计算目标权限（目录至少 0o700，文件至少 0o600）
///   │     ├─ 设置权限（如果需要）
///   │     └─ 存储 uid/gid/mode 到 xattr
///   │
///   └─> 5. 处理硬链接列表（第二阶段）
/// ```
///
/// ## 权限调整规则
///
/// | 类型 | 最小权限 | 说明 |
/// |------|---------|------|
/// | 目录 | `u+rwx` (0o700) | 所有者可读写执行 |
/// | 文件 | `u+rw` (0o600) | 所有者可读写 |
///
/// ## 为什么跳过 symlink 的权限设置？
///
/// 符号链接的权限在大多数系统上是固定的（`lrwxrwxrwx`），
/// 实际权限由目标文件决定，所以不需要设置。
pub(crate) async fn extract_tar_with_ownership_override<R: AsyncRead + Unpin>(
    archive: &mut Archive<R>,
    extract_dir: &Path,
    parent_layers: LayerDependencies,
) -> MicrosandboxResult<()> {
    // 缓存 xattr 名称，避免重复分配
    let xattr_name = CString::new("user.containers.override_stat")
        .map_err(|e| anyhow::anyhow!("Invalid attr name: {e:?}"))?;

    // 存储硬链接以便稍后处理（所有常规文件提取完成后）
    let mut hard_links = HardLinkVec::default();
    let mut entries = archive.entries()?;

    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let entry_path = entry.path()?.to_path_buf();
        let dst_path = extract_dir.join(&entry_path);

        // 从 tar 条目获取原始元数据
        let original_uid = entry.header().uid()?;
        let original_gid = entry.header().gid()?;
        let permission_bits = entry.header().mode()?;

        // 检查条目类型
        let entry_type = entry.header().entry_type();
        let is_symlink = entry_type.is_symlink();
        let is_hard_link = entry_type.is_hard_link();

        // 计算完整的模式（包含文件类型位）
        let original_mode = get_full_mode(&entry_type, permission_bits);

        // 单独处理硬链接 - 收集它们以便在所有文件提取完成后处理
        if is_hard_link {
            if let Ok(Some(link_name)) = entry.link_name() {
                hard_links.push(HardLink {
                    link_path: dst_path.clone(),
                    target_path: extract_dir.join(link_name.as_ref()),
                    uid: original_uid,
                    gid: original_gid,
                    mode: original_mode,
                });
            }
            continue;
        }

        // 提取条目（普通文件、目录、symlink）
        tracing::debug!(path = %dst_path.display(), "Extracting entry");
        unpack(
            entry,
            &entry_path,
            &dst_path,
            extract_dir,
            parent_layers.clone(),
        )
        .await?;

        tracing::debug!(dst_path = %dst_path.display(), "Done unpacking entry");

        // 跳过 symlink 的所有后续操作
        if is_symlink {
            tracing::trace!(
                dst_path = %dst_path.display(),
                "Extracted symlink with original uid:gid:mode {}:{}:{:o}",
                original_uid,
                original_gid,
                original_mode
            );
            continue;
        }

        // 对于普通文件和目录，处理权限和 xattr
        let metadata = std::fs::metadata(&dst_path)?;
        let is_dir = metadata.is_dir();
        let current_mode = metadata.permissions().mode();
        let current_permission_bits = current_mode & 0o7777; // 仅提取权限位

        // 计算最终期望的权限
        let desired_permission_bits = if is_dir {
            // 对于目录，确保至少 u+rwx (0o700)
            current_permission_bits | 0o700
        } else {
            // 对于文件，确保至少 u+rw (0o600)
            current_permission_bits | 0o600
        };

        // 如果需要修改权限，执行一次设置操作
        if current_permission_bits != desired_permission_bits {
            let mut permissions = metadata.permissions();
            permissions.set_mode(desired_permission_bits);
            std::fs::set_permissions(&dst_path, permissions)?;
        }

        // 将原始 uid/gid/mode 存储到 xattr
        set_stat_xattr(
            &dst_path,
            &xattr_name,
            original_uid,
            original_gid,
            original_mode,
        )?;

        tracing::trace!(
            "Extracted {} with original uid:gid:mode {}:{}:{:o}, stored in xattr",
            dst_path.display(),
            original_uid,
            original_gid,
            original_mode
        );
    }

    // 第二阶段：处理硬链接
    hard_links.extract(&xattr_name).await?;
    Ok(())
}

/// 将 tar 条目解压到目标路径
///
/// 此函数负责处理 tar 条目的实际解压。
/// 如果条目的父目录不存在，会从父层中复制祖先目录。
///
/// ## 参数
///
/// * `entry` - 要解压的 tar 条目
/// * `entry_path` - tar 条目中的路径
/// * `dst_path` - 解压目标路径
/// * `extract_dir` - 提取目录根路径
/// * `parent_layers` - 父层依赖关系
///
/// ## 返回值
///
/// * `Ok(())` - 解压成功
/// * `Err(MicrosandboxError)` - 解压失败
///
/// ## 处理流程
///
/// ```text
/// unpack()
///   │
///   ├─> 1. 尝试直接解压条目
///   │     │
///   │     ├─ 成功 ──> 返回 Ok(())
///   │     │
///   │     └─ 失败 ──> 检查错误类型
///   │           │
///   │           ├─ NotFound ──> 继续步骤 2
///   │           │
///   │           └─ 其他错误 ──> 返回错误
///   │
///   ├─> 2. 获取条目的父目录
///   │
///   ├─> 3. 遍历所有祖先目录（从根到父目录）
///   │     │
///   │     ├─ 检查是否是 `..`（父目录引用）
///   │     │   │
///   │     │   ├─ 是 ──> 跳过（防止目录遍历攻击）
///   │     │   │
///   │     │   └─ 否 ──> 继续在父层中搜索
///   │     │
///   │     ├─ 在父层中搜索祖先目录
///   │     │   │
///   │     │   ├─ 找到 ──> 复制目录及其属性
///   │     │   │
///   │     │   └─ 未找到 ──> 返回错误
///   │     │
///   │     └─ 继续下一个祖先目录
///   │
///   └─> 4. 重试解压条目
/// ```
///
/// ## 安全考虑
///
/// ### 防止目录遍历攻击
///
/// 如果 tar 条目包含 `../` 路径组件：
/// - 检测到 `Component::ParentDir` 时立即停止
/// - 记录调试日志并中断处理
///
/// ### 为什么需要复制父层的目录？
///
/// OCI 镜像层是差异化的：
/// - 基础层可能包含 `/etc/nginx/` 目录
/// - 顶层只包含 `/etc/nginx/nginx.conf` 文件
/// - 提取顶层时，需要基础层的 `/etc/nginx/` 目录存在
///
/// ## 为什么要复制目录属性？
///
/// 目录的权限和 xattr 可能包含重要信息：
/// - 权限位：决定谁可以访问该目录
/// - xattr：存储原始的 uid/gid/mode（用于在 VM 中恢复）
async fn unpack<R: AsyncRead + Unpin>(
    mut entry: Entry<Archive<R>>,
    entry_path: &Path,
    dst_path: &Path,
    extract_dir: &Path,
    parent_layers: LayerDependencies,
) -> MicrosandboxResult<()> {
    // 尝试直接解压条目
    let Err(err) = entry.unpack(&dst_path).await else {
        tracing::debug!(path = %dst_path.display(), "Done unpacking entry");
        return Ok(());
    };

    // 如果不是 NotFound 错误，直接返回
    if !matches!(err.kind(), ErrorKind::NotFound) {
        return Err(err.into());
    }

    // 获取条目的父目录路径
    let parent = entry_path.parent().expect("tar entry to have a parent");
    // 获取所有祖先目录（从深到浅：[a/b/c, a/b, a, .]）
    let ancestors = parent.ancestors().collect::<Vec<_>>();

    // 逆序遍历祖先目录（从浅到深：[., a, a/b, a/b/c]），跳过根目录 `.`
    for ancestor in ancestors.into_iter().rev().skip(1) {
        // 防止目录遍历攻击：跳过 `..` 组件
        if ancestor.components().next() == Some(Component::ParentDir) {
            tracing::debug!(ancestor = %ancestor.display(), "Skipping parent directory");
            break;
        }

        // 在父层中搜索祖先目录
        let (digest, parent_path) = parent_layers.find_dir(ancestor).await?.ok_or_else(|| {
            anyhow!(
                "ancestor directory not found in any parent layer: {}",
                ancestor.display()
            )
        })?;

        let dest_dir = extract_dir.join(ancestor);
        tracing::debug!(
            %digest,
            parent_layer_path = %parent_path.display(),
            extract_dir = %dest_dir.display(),
            "Found dir in a parent layer. Proceeding to copy"
        );

        // 创建目录并复制属性
        create_and_copy_dir_attr(&parent_path, &dest_dir).await?;
        tracing::debug!("Copied parent directory for: {}", entry_path.display());
    }

    // 重试解压条目
    if let Err(err) = entry.unpack(&dst_path).await {
        return Err(MicrosandboxError::LayerExtraction(format!(
            "layer extraction failed after retry: {err}",
        )));
    }

    Ok(())
}

/// 创建目录并复制模板目录的属性
///
/// 此函数用于从父层复制目录时，保持权限和 xattr 不变。
///
/// ## 参数
///
/// * `template_dir` - 模板目录（源目录）
/// * `dest_dir` - 目标目录（要创建的目录）
///
/// ## 返回值
///
/// * `Ok(())` - 创建成功
/// * `Err(MicrosandboxError)` - 创建失败
///
/// ## 处理流程
///
/// ```text
/// create_and_copy_dir_attr()
///   │
///   ├─> 1. 检查目标目录是否已存在
///   │     │
///   │     ├─ 存在 ──> 记录调试日志，直接返回
///   │     │
///   │     └─ 不存在 ──> 继续步骤 2
///   │
///   ├─> 2. 验证源目录存在且是目录
///   │
///   ├─> 3. 获取源目录的权限模式
///   │
///   ├─> 4. 创建新目录（使用相同权限）
///   │
///   └─> 5. 复制所有 xattr
///   │     │
///   │     ├─ 遍历源目录的所有 xattr
///   │     ├─ 读取 xattr 值
///   │     └─ 设置到目标目录（失败则记录警告）
///   │
///   └─> 返回 Ok(())
/// ```
///
/// ## 为什么失败只记录警告？
///
/// xattr 设置失败可能有多种原因：
/// - 文件系统不支持 xattr
/// - xattr 名称在目标系统上无效
/// - 权限不足
///
/// 这些通常不是致命错误，所以只记录警告并继续。
async fn create_and_copy_dir_attr(template_dir: &Path, dest_dir: &Path) -> MicrosandboxResult<()> {
    if dest_dir.exists() {
        tracing::debug!(dest_dir = %dest_dir.display(), "Destination directory already exists");
        return Ok(());
    }

    if !template_dir.is_dir() {
        return Err(MicrosandboxError::LayerExtraction(format!(
            "Source directory is not a directory or does not exist: {}",
            template_dir.display()
        )));
    }

    // 创建新目录，并从模板目录复制权限和 xattr
    let mode = fs::metadata(&template_dir).await?.permissions().mode();
    DirBuilder::new().mode(mode).create(&dest_dir).await?;
    if let Ok(xattrs) = xattr::list(template_dir) {
        for attr in xattrs {
            if let Ok(Some(value)) = xattr::get(template_dir, &attr)
                && let Err(e) = xattr::set(dest_dir, &attr, &value)
            {
                tracing::warn!("Failed to set xattr: {}", e);
            }
        }
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 硬链接处理
//--------------------------------------------------------------------------------------------------

/// 硬链接信息结构体
///
/// 用于存储硬链接的元数据，以便在所有文件提取完成后处理。
///
/// ## 字段说明
///
/// - `link_path`: 硬链接的路径
/// - `target_path`: 硬链接指向的目标路径
/// - `uid`: 原始用户 ID
/// - `gid`: 原始组 ID
/// - `mode`: 完整模式（包含文件类型位）
struct HardLink {
    link_path: PathBuf,
    target_path: PathBuf,
    uid: u64,
    gid: u64,
    mode: u32,
}

/// 硬链接收集器
///
/// 用于收集所有硬链接条目，以便第二阶段统一处理。
///
/// ## 为什么需要两阶段处理？
///
/// 硬链接的目标文件必须先存在，才能创建硬链接。
/// 但 tar 文件中条目的顺序是不确定的：
/// - 硬链接条目可能在目标文件之前出现
/// - 目标文件可能来自父层
///
/// 解决方案：
/// 1. 第一阶段：收集所有硬链接信息
/// 2. 第二阶段：在所有常规文件提取完成后创建硬链接
#[derive(Default)]
struct HardLinkVec {
    hard_links: Vec<HardLink>,
}

impl From<Vec<HardLink>> for HardLinkVec {
    fn from(value: Vec<HardLink>) -> Self {
        Self { hard_links: value }
    }
}

impl HardLinkVec {
    /// 添加一个硬链接信息
    pub fn push(&mut self, link: HardLink) {
        self.hard_links.push(link);
    }

    /// 提取所有硬链接（第二阶段）
    ///
    /// 此方法会：
    /// 1. 遍历所有收集的硬链接
    /// 2. 创建硬链接
    /// 3. 设置权限（至少 u+rw）
    /// 4. 存储 uid/gid/mode 到 xattr
    ///
    /// ## 参数
    ///
    /// * `xattr_name` - xattr 名称
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 所有硬链接创建成功
    /// * `Err(MicrosandboxError)` - 创建失败
    ///
    /// ## 错误处理策略
    ///
    /// 硬链接创建失败只记录警告，不中断整个提取过程：
    /// - 目标文件可能不存在
    /// - 文件系统可能不支持硬链接
    /// - xattr 设置可能失败
    ///
    /// 这些通常是可容忍的错误。
    async fn extract(&self, xattr_name: &CStr) -> MicrosandboxResult<()> {
        // 第二阶段：处理所有硬链接
        for link_info in &self.hard_links {
            // 创建硬链接
            match std::fs::hard_link(&link_info.target_path, &link_info.link_path) {
                Ok(_) => {
                    // 硬链接创建成功，处理元数据
                    // 获取元数据并设置正确的权限
                    let metadata = match std::fs::metadata(&link_info.link_path) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to get metadata for hard link {}: {}",
                                link_info.link_path.display(),
                                e
                            );
                            continue;
                        }
                    };

                    let current_mode = metadata.permissions().mode();
                    let current_permission_bits = current_mode & 0o7777; // 仅提取权限位
                    let desired_permission_bits = current_permission_bits | 0o600; // 确保至少 u+rw

                    // 设置权限（如果需要）
                    if current_permission_bits != desired_permission_bits {
                        let mut permissions = metadata.permissions();
                        permissions.set_mode(desired_permission_bits);
                        if let Err(e) = std::fs::set_permissions(&link_info.link_path, permissions)
                        {
                            tracing::warn!(
                                "Failed to set permissions for hard link {}: {}",
                                link_info.link_path.display(),
                                e
                            );
                            continue;
                        }
                    }

                    // 将原始 uid/gid/mode 存储到 xattr
                    if let Err(e) = set_stat_xattr(
                        &link_info.link_path,
                        xattr_name,
                        link_info.uid,
                        link_info.gid,
                        link_info.mode,
                    ) {
                        // 对于硬链接，xattr 错误只记录警告而不是失败
                        tracing::warn!(
                            "Failed to set xattr on hard link {}: {}",
                            link_info.link_path.display(),
                            e
                        );
                    }

                    tracing::trace!(
                        "Created hard link {} -> {} with original uid:gid:mode {}:{}:{:o}",
                        link_info.link_path.display(),
                        link_info.target_path.display(),
                        link_info.uid,
                        link_info.gid,
                        link_info.mode
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to create hard link {} -> {}: {}",
                        link_info.link_path.display(),
                        link_info.target_path.display(),
                        e
                    );
                }
            }
        }

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oci::{Image, LayerDependencies, LayerOps, global_cache::GlobalCacheOps};
    use async_trait::async_trait;
    use oci_spec::image::Digest;
    use std::{io::Cursor, os::unix::fs::PermissionsExt, str::FromStr, sync::Arc};
    use tempfile::TempDir;
    use tokio::sync::{Mutex, OwnedMutexGuard};
    use tokio_tar::Archive;

    /// A minimal mock for GlobalCacheOps used by MockLayer.
    struct MockGlobalCacheOps {
        tar_dir: PathBuf,
        extracted_dir: PathBuf,
    }

    #[async_trait]
    impl GlobalCacheOps for MockGlobalCacheOps {
        fn tar_download_dir(&self) -> &PathBuf {
            &self.tar_dir
        }

        fn extracted_layers_dir(&self) -> &PathBuf {
            &self.extracted_dir
        }

        async fn build_layer(&self, digest: &Digest) -> Arc<dyn LayerOps> {
            Arc::new(MockLayer::new(digest.clone(), self.extracted_dir.clone()))
        }

        async fn all_layers_extracted(
            &self,
            _image: &crate::oci::Reference,
        ) -> crate::MicrosandboxResult<bool> {
            Ok(true)
        }
    }

    /// A mock layer whose extracted directory is pre-populated on disk.
    struct MockLayer {
        digest: Digest,
        extracted_dir: PathBuf,
        lock: Arc<Mutex<()>>,
        global_ops: Arc<dyn GlobalCacheOps>,
    }

    impl MockLayer {
        fn new(digest: Digest, base_extracted_dir: PathBuf) -> Self {
            let global_ops: Arc<dyn GlobalCacheOps> = Arc::new(MockGlobalCacheOps {
                tar_dir: base_extracted_dir.clone(),
                extracted_dir: base_extracted_dir,
            });
            Self {
                digest,
                extracted_dir: global_ops.extracted_layers_dir().clone(),
                lock: Arc::new(Mutex::new(())),
                global_ops,
            }
        }
    }

    #[async_trait]
    impl LayerOps for MockLayer {
        fn global_layer_ops(&self) -> &dyn GlobalCacheOps {
            self.global_ops.as_ref()
        }

        fn digest(&self) -> &Digest {
            &self.digest
        }

        async fn extracted(&self) -> crate::MicrosandboxResult<(bool, OwnedMutexGuard<()>)> {
            let guard = self.lock.clone().lock_owned().await;
            Ok((true, guard))
        }

        async fn cleanup_extracted(&self) -> crate::MicrosandboxResult<()> {
            Ok(())
        }

        async fn extract(&self, _parent: LayerDependencies) -> crate::MicrosandboxResult<()> {
            Ok(())
        }

        async fn find_dir(&self, path_in_tar: &Path) -> Option<PathBuf> {
            let canonical_path = self.extracted_dir.join(path_in_tar);
            if canonical_path.exists() && canonical_path.is_dir() {
                return Some(canonical_path);
            }
            None
        }
    }

    /// Build a tar archive (in memory) containing a single file at `file_path`
    /// with `contents`, but **without** any parent directory entries.
    fn build_tar_without_parent_dirs(file_path: &str, contents: &[u8]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_path(file_path).unwrap();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_uid(1000);
        header.set_gid(1000);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append(&header, contents).unwrap();
        builder.into_inner().unwrap()
    }

    /// Test scenario:
    ///
    /// - Layer A (grandparent): has directories `a/`, `a/b/`, `a/b/c/`, `a/b/c/d/`
    /// - Layer B (immediate parent): does NOT have those directories
    /// - Layer C (current): has file `a/b/c/d/example.txt` but no directory entries
    ///
    /// The extraction of Layer C must skip Layer B (which lacks the dirs) and
    /// source the ancestor directories from Layer A (the grandparent).
    #[tokio::test]
    async fn test_extract_layer_with_missing_deeply_nested_parents() {
        let temp = TempDir::new().unwrap();
        let grandparent_digest = Digest::from_str(
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let parent_digest = Digest::from_str(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .unwrap();
        let current_digest = Digest::from_str(
            "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        )
        .unwrap();

        // Layer A (grandparent): has the deeply nested directories
        let grandparent_extracted_dir = temp.path().join("grandparent_extracted");
        std::fs::create_dir_all(grandparent_extracted_dir.join("a/b/c/d")).unwrap();

        // Set a unique xattr and distinct permissions on each ancestor directory
        let dir_modes: &[(&str, u32)] = &[
            ("a", 0o755),
            ("a/b", 0o750),
            ("a/b/c", 0o700),
            ("a/b/c/d", 0o751),
        ];
        for &(dir, mode) in dir_modes {
            let dir_path = grandparent_extracted_dir.join(dir);
            std::fs::set_permissions(&dir_path, std::fs::Permissions::from_mode(mode)).unwrap();
            xattr::set(&dir_path, "user.test_marker", dir.as_bytes()).unwrap();
        }

        let mock_grandparent = MockLayer {
            digest: grandparent_digest.clone(),
            extracted_dir: grandparent_extracted_dir.clone(),
            lock: Arc::new(Mutex::new(())),
            global_ops: Arc::new(MockGlobalCacheOps {
                tar_dir: temp.path().to_path_buf(),
                extracted_dir: grandparent_extracted_dir.clone(),
            }),
        };

        // Layer B (immediate parent): empty — does NOT have the directories
        let parent_extracted_dir = temp.path().join("parent_extracted");
        std::fs::create_dir_all(&parent_extracted_dir).unwrap();

        let mock_parent = MockLayer {
            digest: parent_digest.clone(),
            extracted_dir: parent_extracted_dir.clone(),
            lock: Arc::new(Mutex::new(())),
            global_ops: Arc::new(MockGlobalCacheOps {
                tar_dir: temp.path().to_path_buf(),
                extracted_dir: parent_extracted_dir.clone(),
            }),
        };

        // Build an Image with both parent layers: [grandparent, parent] (base -> top)
        let parent_image = Image::new(vec![
            Arc::new(mock_grandparent) as Arc<dyn LayerOps>,
            Arc::new(mock_parent) as Arc<dyn LayerOps>,
        ]);
        let parent_layers = LayerDependencies::new(current_digest, parent_image);

        // Build a tar with a deeply nested file but NO directory entries
        let tar_bytes = build_tar_without_parent_dirs("a/b/c/d/example.txt", b"hello world");
        let cursor = Cursor::new(tar_bytes);
        let mut archive = Archive::new(cursor);

        // Extract into a fresh directory (Layer C)
        let extract_dir = temp.path().join("current_extracted");
        std::fs::create_dir_all(&extract_dir).unwrap();

        extract_tar_with_ownership_override(&mut archive, &extract_dir, parent_layers)
            .await
            .expect("extraction should succeed even when immediate parent lacks the dirs");

        // Verify the file was extracted
        let extracted_file = extract_dir.join("a/b/c/d/example.txt");
        assert!(
            extracted_file.exists(),
            "deeply nested file should be extracted"
        );
        let content = std::fs::read_to_string(&extracted_file).unwrap();
        assert_eq!(content, "hello world");

        // Verify all ancestor directories were created with correct permissions and xattrs
        for &(dir, expected_mode) in dir_modes {
            let dir_path = extract_dir.join(dir);
            assert!(dir_path.is_dir(), "dir '{dir}' should exist");

            let actual_mode = std::fs::metadata(&dir_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                actual_mode, expected_mode,
                "permissions mismatch on '{dir}': expected {expected_mode:#o}, got {actual_mode:#o}"
            );

            let attr = xattr::get(&dir_path, "user.test_marker")
                .expect("xattr read should not fail")
                .unwrap_or_else(|| panic!("xattr 'user.test_marker' missing on '{dir}'"));
            assert_eq!(attr, dir.as_bytes(), "xattr value mismatch on '{dir}'");
        }
    }
}
