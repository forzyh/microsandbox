//! Microsandbox 沙箱根文件系统管理模块。
//!
//! # 概述
//!
//! 本模块提供 Microsandbox 沙箱根文件系统（rootfs）的管理功能，
//! 包括文件系统层的创建、提取和合并操作，遵循 OCI（Open Container
//! Initiative）规范。
//!
//! # 核心概念
//!
//! ## 什么是 Rootfs？
//!
//! Rootfs（根文件系统）是沙箱运行时的文件系统视图。在容器和沙箱技术中，
//! rootfs 提供了隔离的文件系统环境，包含沙箱运行所需的所有文件、目录和库。
//!
//! ## 分层文件系统
//!
//! Microsandbox 使用分层文件系统来构建 rootfs：
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │           最终 rootfs 视图            │
//! ├─────────────────────────────────────┤
//! │        Top RW Layer (可写层)         │  ← 沙箱运行时写入的数据
//! ├─────────────────────────────────────┤
//! │       Patch Layer (补丁层)           │  ← 沙箱脚本、配置补丁
//! ├─────────────────────────────────────┤
//! │       Image Layer N (镜像层)         │  │
//! ├─────────────────────────────────────┤  │
//! │       Image Layer N-1 (镜像层)       │  │ OCI 镜像层
//! ├─────────────────────────────────────┤  │
//! │               ...                   │  │
//! ├─────────────────────────────────────┤  │
//! │       Image Layer 0 (基础层)         │  ← 通常是基础镜像
//! └─────────────────────────────────────┘
//! ```
//!
//! ## OverlayFS 工作原理
//!
//! Microsandbox 在支持的系统上使用 OverlayFS 来合并多个层：
//!
//! - **Lower Layers**（下层）：只读的镜像层，从下到上堆叠
//! - **Upper Layer**（上层）：可写层，沙箱的修改写到这里
//! - **Merged View**（合并视图）：内核动态呈现的统一视图
//!
//! ```text
//!                    Merged View (合并视图)
//!                          │
//!          ┌───────────────┼───────────────┐
//!          │               │               │
//!    ┌─────▼─────┐  ┌──────▼──────┐  ┌────▼────┐
//!    │ Upper RW  │  │   Patch     │  │ Lower   │
//!    │   Layer   │  │   Layer     │  │ Layers  │
//!    └───────────┘  └─────────────┘  └─────────┘
//! ```
//!
//! # OCI 规范术语
//!
//! | 术语 | 说明 |
//! |------|------|
//! | **Layer** | 镜像的一个只读层，通常是 tar 包 |
//! | **Whiteout** | 表示删除文件的标记文件，前缀 `.wh.` |
//! | **Opaque Whiteout** | 表示删除整个目录的标记，文件名为 `.wh..wh..opq` |
//! | **Manifest** | 镜像清单，描述镜像的层和配置 |
//! | **Config** | 镜像配置，包含环境变量、入口点等 |
//!
//! # 主要功能
//!
//! | 函数 | 功能描述 |
//! |------|----------|
//! | `patch_with_sandbox_scripts()` | 向 rootfs 添加沙箱脚本 |
//! | `patch_with_virtiofs_mounts()` | 配置 virtio-fs 挂载点 |
//! | `patch_with_default_dns_settings()` | 配置默认 DNS 设置 |
//! | `patch_with_stat_override()` | 设置 rootfs 权限覆盖 |
//!
//! # 使用示例
//!
//! ```no_run
//! use microsandbox_core::management::rootfs;
//! use std::path::Path;
//! use std::collections::HashMap;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let rootfs_path = Path::new("/path/to/rootfs");
//!
//! // 添加沙箱脚本
//! let mut scripts = HashMap::new();
//! scripts.insert("start".to_string(), "echo 'Starting...'".to_string());
//! rootfs::patch_with_sandbox_scripts(
//!     &rootfs_path.join(".sandbox_scripts"),
//!     &scripts,
//!     "/bin/sh"
//! ).await?;
//!
//! // 配置 virtio-fs 挂载
//! // rootfs::patch_with_virtiofs_mounts(rootfs_path, &mapped_dirs).await?;
//!
//! // 配置默认 DNS
//! rootfs::patch_with_default_dns_settings(&[rootfs_path.to_path_buf()]).await?;
//!
//! // 设置权限覆盖
//! rootfs::patch_with_stat_override(rootfs_path).await?;
//! # Ok(())
//! # }
//! ```

use std::{
    collections::HashMap,
    fs::Permissions,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use tokio::fs;

use crate::{MicrosandboxResult, config::PathPair, vm::VIRTIOFS_TAG_PREFIX};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// OCI 层中使用的 opaque whiteout 标记文件名。
///
/// # 什么是 Opaque Whiteout？
///
/// 在 OCI 镜像层中，`.wh..wh..opq` 是一个特殊文件，用于表示
/// "此目录在上一层中已被删除"。当合并层时，如果某层包含此文件，
/// 则该目录在合并视图中将不显示下层中的任何内容。
///
/// ## 使用场景
///
/// 假设上层镜像想删除下层镜像中的 `/etc/configs` 目录：
///
/// 1. 在上层创建 `/etc/configs/.wh..wh..opq` 文件
/// 2. 合并时，整个 `/etc/configs` 目录被视为已删除
/// 3. 即使下层有 `/etc/configs/file1`，也不会出现在合并视图中
///
/// ## 与普通 Whiteout 的区别
///
/// - **普通 Whiteout** (`.wh.filename`): 删除单个文件
/// - **Opaque Whiteout** (`.wh..wh..opq`): 删除整个目录
pub const OPAQUE_WHITEOUT_MARKER: &str = ".wh..wh..opq";

/// OCI 层中 whiteout 文件的前缀。
///
/// # 什么是 Whiteout？
///
/// Whiteout 文件是 OCI 镜像中用于表示"删除"的特殊文件。
/// 当合并层时，whiteout 文件会"遮盖"下层同名的文件。
///
/// ## 命名规则
///
/// - 原始文件：`/path/to/file.txt`
/// - Whiteout 文件：`/path/to/.wh.file.txt`
///
/// ## 示例
///
/// ```text
/// Layer 0 (基础层):
///   /etc/config.yaml  ← 原始文件
///
/// Layer 1 (上层):
///   /etc/.wh.config.yaml  ← whiteout 文件
///
/// 合并视图:
///   /etc/config.yaml  不可见（被 whiteout 遮盖）
/// ```
pub const WHITEOUT_PREFIX: &str = ".wh.";

// XAttr（扩展属性）名称，用于覆盖容器内的文件统计信息
const XATTR_OVERRIDE_STATS_NAME: &str = "user.containers.override_stat";

// XAttr 值，格式为 "uid:gid:mode"
// 0:0:040755 表示：
// - uid: 0 (root 用户)
// - gid: 0 (root 组)
// - mode: 040755
//   - 040000 = S_IFDIR (目录文件类型)
//   - 0755 = rwxr-xr-x (所有者读写执行，组和其他用户读执行)
const XATTR_OVERRIDE_STATS_VALUE: &str = "0:0:040755";

//--------------------------------------------------------------------------------------------------
// 公开函数
//--------------------------------------------------------------------------------------------------

/// 通过在 rootfs 中添加 `/.sandbox_scripts` 目录来更新 rootfs。
///
/// # 功能说明
///
/// 此函数用于向沙箱的根文件系统添加自定义脚本。这些脚本在沙箱启动时
/// 可以被执行，用于初始化环境或启动应用程序。
///
/// ## 工作流程
///
/// 1. **清理**：如果 `.sandbox_scripts` 目录已存在，先删除它
///    - 确保每次都是干净的状态，避免残留旧脚本
///
/// 2. **创建目录**：创建 `.sandbox_scripts` 目录
///
/// 3. **写入脚本**：对于每个脚本：
///    - 创建脚本文件
///    - 添加 shebang 行（如 `#!/bin/sh`）
///    - 写入脚本内容
///    - 设置执行权限（rwxr-x---，即 0750）
///
/// 4. **创建 shell 脚本**：创建一个名为 `shell` 的特殊脚本
///    - 内容仅为 shell 路径（如 `/bin/sh`）
///    - 用于沙箱获取 shell 路径
///
/// # 参数
///
/// * `scripts_dir` - rootfs 中脚本目录的路径（通常是 `rootfs/.sandbox_scripts`）
/// * `scripts` - HashMap，键为脚本名，值为脚本内容
/// * `shell_path` - rootfs 中 shell 二进制的路径（如 "/bin/sh"）
///
/// # 权限说明
///
/// 脚本文件的权限设置为 `0750`（rwxr-x---）：
/// - **所有者（root）**: 读、写、执行
/// - **组（root）**: 读、执行
/// - **其他用户**: 无权限
///
/// 这种权限设置确保了：
/// - 沙箱内的 root 用户可以执行脚本
/// - 同组用户可以读取和执行
/// - 其他用户无法访问
///
/// # 生成的目录结构
///
/// ```text
/// rootfs/
/// └── .sandbox_scripts/
///     ├── start          # 启动脚本
///     ├── shell          # 包含 shell 路径
///     └── custom         # 自定义脚本
/// ```
///
/// # 脚本内容格式
///
/// 每个脚本的内容格式为：
/// ```text
/// #!<shell_path>
/// <script_content>
/// ```
///
/// 例如，如果 shell_path 为 "/bin/sh"，脚本名为 "start"，内容为 "echo hello"：
/// ```text
/// #!/bin/sh
/// echo hello
/// ```
pub async fn patch_with_sandbox_scripts(
    scripts_dir: &Path,
    scripts: &HashMap<String, String>,
    shell_path: impl AsRef<Path>,
) -> MicrosandboxResult<()> {
    // 如果脚本目录已存在，先删除
    // 这样可以确保每次 patch 都是干净的状态
    if scripts_dir.exists() {
        fs::remove_dir_all(&scripts_dir).await?;
    }

    // 创建脚本目录
    fs::create_dir_all(&scripts_dir).await?;

    // 获取 shell 路径字符串，用于 shebang 行
    let shell_path = shell_path.as_ref().to_string_lossy();

    // 为每个脚本创建文件
    for (script_name, script_content) in scripts.iter() {
        // 构建脚本文件路径
        let script_path = scripts_dir.join(script_name);

        // 写入 shebang 行和脚本内容
        // 格式：#!<shell_path>\n<script_content>\n
        let full_content = format!("#!{}\n{}\n", shell_path, script_content);
        fs::write(&script_path, full_content).await?;

        // 设置执行权限（rwxr-x---，即 0750）
        // 用户和组可执行，其他用户无权限
        fs::set_permissions(&script_path, Permissions::from_mode(0o750)).await?;
    }

    // 创建 shell 脚本，内容仅为 shell 路径
    // 这个脚本用于让沙箱知道 shell 的位置
    let shell_script_path = scripts_dir.join("shell");
    fs::write(&shell_script_path, shell_path.to_string()).await?;
    fs::set_permissions(&shell_script_path, Permissions::from_mode(0o750)).await?;

    Ok(())
}

/// 在客户机 rootfs 中更新 /etc/fstab 文件以挂载映射的目录。
/// 如果文件不存在则创建。
///
/// # 功能说明
///
/// 此函数用于配置沙箱启动时的文件系统挂载。它通过修改 guest rootfs 中的
/// `/etc/fstab` 文件，添加 virtio-fs 挂载条目，使得宿主机目录可以在沙箱
/// 启动时自动挂载到 guest 中。
///
/// ## 什么是 virtio-fs？
///
/// virtio-fs 是一种高性能的虚拟化文件系统，允许虚拟机直接访问宿主机
/// 的目录。与传统的 9p 文件系统相比，virtio-fs 提供了更好的性能。
///
/// ## 工作原理
///
/// ```text
/// 宿主机                          虚拟机（沙箱）
/// ┌─────────────┐               ┌─────────────┐
/// │ /host/data  │ ──virtiofs──► │ /guest/data │
/// │ /host/lib   │ ──virtiofs──► │ /guest/lib  │
/// └─────────────┘               └─────────────┘
/// ```
///
/// ## fstab 格式
///
/// 每个挂载条目使用以下格式：
/// ```text
/// virtiofs_N  /guest/path  virtiofs  defaults  0  0
/// ```
///
/// 各字段含义：
/// - **virtiofs_N**: 文件系统标识（N 是索引）
/// - **/guest/path**: guest 中的挂载点
/// - **virtiofs**: 文件系统类型
/// - **defaults**: 挂载选项
/// - **0**: dump 标志（不备份）
/// - **0**: fsck 顺序（不检查）
///
/// # 参数
///
/// * `root_path` - guest rootfs 的路径
/// * `mapped_dirs` - 宿主机到 guest 的目录映射列表
///
/// # 实现细节
///
/// 1. **创建 /etc 目录**：如果不存在则创建
/// 2. **读取现有 fstab**：如果文件存在则读取内容
/// 3. **添加头部注释**：如果文件为空，添加标准头部
/// 4. **添加挂载条目**：为每个映射目录添加 virtiofs 条目
/// 5. **创建挂载点**：在 rootfs 中创建挂载点目录
/// 6. **更新文件**：写回 fstab 文件
/// 7. **设置权限**：设置文件权限为 644（rw-r--r--）
///
/// # 错误处理
///
/// 以下情况会返回错误：
/// - 无法在 rootfs 中创建目录
/// - 无法读取或写入 fstab 文件
/// - 无法设置 fstab 文件权限
pub async fn patch_with_virtiofs_mounts(
    root_path: &Path,
    mapped_dirs: &[PathPair],
) -> MicrosandboxResult<()> {
    // fstab 文件路径
    let fstab_path = root_path.join("etc/fstab");

    // 如果父目录不存在，创建父目录
    if let Some(parent) = fstab_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // 读取现有的 fstab 内容（如果存在）
    let mut fstab_content = if fstab_path.exists() {
        fs::read_to_string(&fstab_path).await?
    } else {
        String::new()
    };

    // 如果文件为空，添加头部注释
    if fstab_content.is_empty() {
        fstab_content.push_str(
            "# /etc/fstab: static file system information.\n\
                 # <file system>\t<mount point>\t<type>\t<options>\t<dump>\t<pass>\n",
        );
    }

    // 为每个映射目录添加条目
    for (idx, dir) in mapped_dirs.iter().enumerate() {
        // 生成 virtiofs 标签：virtiofs_0, virtiofs_1, ...
        // 这个标签会被虚拟机监控器（如 QEMU/KRun）识别
        let tag = format!("{}_{}", VIRTIOFS_TAG_PREFIX, idx);
        tracing::debug!("adding virtiofs mount for {}", tag);
        let guest_path = dir.get_guest();

        // 添加此映射目录的 fstab 条目
        fstab_content.push_str(&format!(
            "{}\t{}\tvirtiofs\tdefaults\t0\t0\n",
            tag, guest_path
        ));

        // 在 guest rootfs 中创建挂载点目录
        // 将 guest 路径转换为相对路径（去除前导斜杠）
        let guest_path_str = guest_path.as_str();
        let relative_path = guest_path_str.strip_prefix('/').unwrap_or(guest_path_str);
        let mount_point = root_path.join(relative_path);
        fs::create_dir_all(mount_point).await?;
    }

    // 写入更新后的 fstab 内容
    fs::write(&fstab_path, fstab_content).await?;

    // 设置正确的权限（644 = rw-r--r--）
    // fstab 是系统配置文件，应该是只读的
    let perms = fs::metadata(&fstab_path).await?.permissions();
    let mut new_perms = perms;
    new_perms.set_mode(0o644);
    fs::set_permissions(&fstab_path, new_perms).await?;

    Ok(())
}

/// 在 guest rootfs 中更新 /etc/hosts 文件以添加主机名映射。
/// 如果文件不存在则创建。
///
/// # 功能说明
///
/// 此函数用于配置沙箱内的主机名解析。它通过修改 `/etc/hosts` 文件，
/// 添加 IP 地址到主机名的映射，使得沙箱可以解析特定的主机名。
///
/// ## hosts 文件格式
///
/// 每个主机名映射遵循标准的 hosts 文件格式：
/// ```text
/// 192.168.1.100  hostname1
/// 192.168.1.101  hostname2
/// ```
///
/// ## 使用场景
///
/// 在多沙箱环境中，可能需要沙箱之间互相访问。例如：
/// - 沙箱 A 需要访问沙箱 B 提供的数据库服务
/// - 通过在 hosts 文件中添加映射，沙箱 A 可以使用主机名访问沙箱 B
///
/// ```text
/// # /etc/hosts 示例
/// 192.168.10.1  db-sandbox
/// 192.168.10.2  cache-sandbox
/// 192.168.10.3  app-sandbox
/// ```
///
/// # 参数
///
/// * `root_path` - guest rootfs 的路径
/// * `hostname_mappings` - (IPv4 地址，主机名) 对列表
///
/// # 实现细节
///
/// 1. **创建 /etc 目录**：如果不存在则创建
/// 2. **读取现有 hosts**：如果文件存在则读取内容
/// 3. **添加默认条目**：如果文件为空，添加 localhost 等默认条目
/// 4. **添加自定义映射**：为每个 IP-主机名对添加条目
///    - 检查是否已存在该映射，避免重复
/// 5. **更新文件**：写回 hosts 文件
/// 6. **设置权限**：设置文件权限为 644
///
/// # 默认条目
///
/// 如果 hosts 文件为空，会添加以下默认条目：
/// ```text
/// 127.0.0.1\tlocalhost
/// ::1\tlocalhost ip6-localhost ip6-loopback
/// ```
///
/// # 错误处理
///
/// 以下情况会返回错误：
/// - 无法在 rootfs 中创建目录
/// - 无法读取或写入 hosts 文件
/// - 无法设置 hosts 文件权限
async fn _patch_with_hostnames(
    root_path: &Path,
    hostname_mappings: &[(std::net::Ipv4Addr, String)],
) -> MicrosandboxResult<()> {
    // hosts 文件路径
    let hosts_path = root_path.join("etc/hosts");

    // 如果父目录不存在，创建父目录
    if let Some(parent) = hosts_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // 读取现有的 hosts 内容（如果存在）
    let mut hosts_content = if hosts_path.exists() {
        fs::read_to_string(&hosts_path).await?
    } else {
        String::new()
    };

    // 如果文件为空，添加头部注释和默认条目
    if hosts_content.is_empty() {
        hosts_content.push_str(
            "# /etc/hosts: static table lookup for hostnames.\n\
             # <ip-address>\t<hostname>\n\n\
             127.0.0.1\tlocalhost\n\
             ::1\tlocalhost ip6-localhost ip6-loopback\n",
        );
    }

    // 为主机名映射添加条目
    for (ip_addr, hostname) in hostname_mappings {
        // 检查此映射是否已存在
        let entry = format!("{}\t{}", ip_addr, hostname);
        if !hosts_content.contains(&entry) {
            // 只添加不存在的映射，避免重复
            hosts_content.push_str(&format!("{}\n", entry));
        }
    }

    // 写入更新后的 hosts 内容
    fs::write(&hosts_path, hosts_content).await?;

    // 设置正确的权限（644 = rw-r--r--）
    let perms = fs::metadata(&hosts_path).await?.permissions();
    let mut new_perms = perms;
    new_perms.set_mode(0o644);
    fs::set_permissions(&hosts_path, new_perms).await?;

    Ok(())
}

/// 在 guest rootfs 中更新 /etc/resolv.conf 文件，如果不存在则添加默认 DNS 服务器。
/// 如果文件不存在则创建。
///
/// # 功能说明
///
/// 此函数用于配置沙箱的 DNS 解析。它会检查所有 rootfs 层中是否已存在
/// `/etc/resolv.conf` 文件以及是否配置了 nameserver。如果任何层都没有
/// nameserver 配置，则会在顶层添加默认的 DNS 服务器。
///
/// ## 为什么检查所有层？
///
/// 在 overlayfs 环境中，基础镜像可能已经包含配置好的 resolv.conf。
/// 为了避免覆盖用户的自定义配置，我们会先检查所有层。
///
/// ## resolv.conf 格式
///
/// resolv.conf 文件遵循标准格式：
/// ```text
/// # /etc/resolv.conf: DNS resolver configuration
/// nameserver 1.1.1.1
/// nameserver 8.8.8.8
/// ```
///
/// ## 默认 DNS 服务器
///
/// - **1.1.1.1** - Cloudflare DNS（公共 DNS 服务）
/// - **8.8.8.8** - Google DNS（公共 DNS 服务）
///
/// # 参数
///
/// * `root_paths` - 要检查的 rootfs 路径列表，从底层到顶层排序
///   - 对于 overlayfs：应该是 [lower_layers..., patch_dir]
///   - 对于 native rootfs：应该是 [root_path]
///
/// # 实现细节
///
/// 1. **遍历所有层**：检查每个 rootfs 层中的 resolv.conf
/// 2. **检查 nameserver**：如果任何层包含 nameserver 条目，则不做任何事
/// 3. **添加默认配置**：如果所有层都没有 nameserver，在顶层添加默认配置
/// 4. **设置权限**：设置文件权限为 644
///
/// # 示例
///
/// ```text
/// # overlayfs 场景
/// root_paths = [
///     "/path/to/lower_layer_0",  // 基础镜像层
///     "/path/to/lower_layer_1",  // 应用镜像层
///     "/path/to/patch",          // 补丁层（顶层）
/// ]
///
/// # native rootfs 场景
/// root_paths = [
///     "/path/to/rootfs",
/// ]
/// ```
///
/// # 错误处理
///
/// 以下情况会返回错误：
/// - 无法在 rootfs 中创建目录
/// - 无法读取或写入 resolv.conf 文件
/// - 无法设置 resolv.conf 文件权限
pub async fn patch_with_default_dns_settings(root_paths: &[PathBuf]) -> MicrosandboxResult<()> {
    // 如果 root_paths 为空，直接返回成功
    if root_paths.is_empty() {
        return Ok(());
    }

    // 检查所有层中是否已存在 nameserver 条目
    let mut has_nameserver = false;
    for root_path in root_paths {
        let resolv_path = root_path.join("etc/resolv.conf");
        if resolv_path.exists() {
            let content = fs::read_to_string(&resolv_path).await?;
            if content
                .lines()
                .any(|line| line.trim_start().starts_with("nameserver "))
            {
                // 任何层找到 nameserver 就停止检查
                has_nameserver = true;
                break;
            }
        }
    }

    // 如果所有层都没有 nameserver，在顶层添加默认配置
    if !has_nameserver {
        // 获取顶层（列表最后一个）
        let top_layer = root_paths.last().unwrap();
        let resolv_path = top_layer.join("etc/resolv.conf");

        // 如果父目录不存在，创建父目录
        if let Some(parent) = resolv_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // 创建新的 resolv.conf，包含默认 nameservers
        let mut resolv_content = String::from("# /etc/resolv.conf: DNS resolver configuration\n");
        resolv_content.push_str("nameserver 1.1.1.1\n");
        resolv_content.push_str("nameserver 8.8.8.8\n");

        // 写入文件
        fs::write(&resolv_path, resolv_content).await?;

        // 设置正确的权限（644 = rw-r--r--）
        let perms = fs::metadata(&resolv_path).await?.permissions();
        let mut new_perms = perms;
        new_perms.set_mode(0o644);
        fs::set_permissions(&resolv_path, new_perms).await?;
    }

    Ok(())
}

/// 在 rootfs 目录上设置 user.containers.override_stat 扩展属性。
///
/// # 功能说明
///
/// 此函数设置一个特殊的扩展属性（xattr），用于在虚拟机内部覆盖
/// rootfs 目录的 UID、GID 和权限信息。
///
/// ## 什么是扩展属性（XAttr）？
///
/// 扩展属性是文件系统元数据的键值对，可以附加到文件和目录上。
/// 它们超出了标准文件属性（如权限、时间戳）的范围。
///
/// ## user.containers.override_stat
///
/// 这是一个特殊的 xattr，被容器运行时（如 containerd、Podman）
/// 用于在用户命名空间（user namespace）中覆盖文件的统计信息。
///
/// ### 值格式
///
/// `"uid:gid:mode"`，例如 `"0:0:040755"`：
/// - **uid: 0** - 用户 ID 为 0（root）
/// - **gid: 0** - 组 ID 为 0（root 组）
/// - **mode: 040755** - 文件模式
///   - `040000` = S_IFDIR（目录类型标识符）
///   - `0755` = rwxr-xr-x（权限位）
///
/// ## 为什么需要这个？
///
/// 当使用用户命名空间时，宿主机上的文件所有者可能在虚拟机内部
/// 映射为不同的用户。这个 xattr 告诉虚拟机运行时如何在内部
/// 呈现这些文件的统计信息。
///
/// # 参数
///
/// * `root_path` - 要修改的 rootfs 目录路径
///
/// # 错误处理
///
/// 以下情况会返回错误：
/// - 无法将路径转换为字符串
/// - 无法设置扩展属性
pub async fn patch_with_stat_override(root_path: &Path) -> MicrosandboxResult<()> {
    // 将路径转换为 CString 供 xattr crate 使用
    let path_str = root_path.to_str().ok_or_else(|| {
        crate::MicrosandboxError::InvalidArgument(format!(
            "Could not convert path to string: {}",
            root_path.display()
        ))
    })?;

    // 设置扩展属性
    match xattr::set(
        path_str,
        XATTR_OVERRIDE_STATS_NAME,
        XATTR_OVERRIDE_STATS_VALUE.as_bytes(),
    ) {
        Ok(_) => {
            tracing::debug!(
                "Set xattr {} = {} on {}",
                XATTR_OVERRIDE_STATS_NAME,
                XATTR_OVERRIDE_STATS_VALUE,
                root_path.display()
            );
            Ok(())
        }
        Err(err) => Err(crate::MicrosandboxError::Io(std::io::Error::other(
            format!("Failed to set xattr on {}: {}", root_path.display(), err),
        ))),
    }
}

//--------------------------------------------------------------------------------------------------
// 测试模块
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::MicrosandboxError;

    use super::*;

    #[tokio::test]
    async fn test_patch_rootfs_with_virtiofs_mounts() -> anyhow::Result<()> {
        // 创建一个临时目录作为 rootfs
        let root_dir = TempDir::new()?;
        let root_path = root_dir.path();

        // 为宿主路径创建临时目录
        let host_dir = TempDir::new()?;
        let host_data = host_dir.path().join("data");
        let host_config = host_dir.path().join("config");
        let host_app = host_dir.path().join("app");

        // 创建宿主目录
        fs::create_dir_all(&host_data).await?;
        fs::create_dir_all(&host_config).await?;
        fs::create_dir_all(&host_app).await?;

        // 创建测试用的目录映射
        let mapped_dirs = vec![
            format!("{}:/container/data", host_data.display()).parse::<PathPair>()?,
            format!("{}:/etc/app/config", host_config.display()).parse::<PathPair>()?,
            format!("{}:/app", host_app.display()).parse::<PathPair>()?,
        ];

        // 更新 fstab
        patch_with_virtiofs_mounts(root_path, &mapped_dirs).await?;

        // 验证 fstab 文件已创建且内容正确
        let fstab_path = root_path.join("etc/fstab");
        assert!(fstab_path.exists());

        let fstab_content = fs::read_to_string(&fstab_path).await?;

        // 检查头部
        assert!(fstab_content.contains("# /etc/fstab: static file system information"));
        assert!(
            fstab_content
                .contains("<file system>\t<mount point>\t<type>\t<options>\t<dump>\t<pass>")
        );

        // 检查条目
        assert!(fstab_content.contains("virtiofs_0\t/container/data\tvirtiofs\tdefaults\t0\t0"));
        assert!(fstab_content.contains("virtiofs_1\t/etc/app/config\tvirtiofs\tdefaults\t0\t0"));
        assert!(fstab_content.contains("virtiofs_2\t/app\tvirtiofs\tdefaults\t0\t0"));

        // 验证挂载点已创建
        assert!(root_path.join("container/data").exists());
        assert!(root_path.join("etc/app/config").exists());
        assert!(root_path.join("app").exists());

        // 验证文件权限
        let perms = fs::metadata(&fstab_path).await?.permissions();
        assert_eq!(perms.mode() & 0o777, 0o644);

        // 测试更新现有的 fstab
        let host_logs = host_dir.path().join("logs");
        fs::create_dir_all(&host_logs).await?;

        let new_mapped_dirs = vec![
            format!("{}:/container/data", host_data.display()).parse::<PathPair>()?, // 保留一个现有的
            format!("{}:/var/log", host_logs.display()).parse::<PathPair>()?,        // 添加一个新的
        ];

        // 再次更新 fstab
        patch_with_virtiofs_mounts(root_path, &new_mapped_dirs).await?;

        // 验证更新后的内容
        let updated_content = fs::read_to_string(&fstab_path).await?;
        assert!(updated_content.contains("virtiofs_0\t/container/data\tvirtiofs\tdefaults\t0\t0"));
        assert!(updated_content.contains("virtiofs_1\t/var/log\tvirtiofs\tdefaults\t0\t0"));

        // 验证新挂载点已创建
        assert!(root_path.join("var/log").exists());

        Ok(())
    }

    #[tokio::test]
    async fn test_patch_rootfs_with_virtiofs_mounts_permission_errors() -> anyhow::Result<()> {
        // 在 CI 环境中跳过此测试
        if std::env::var("CI").is_ok() {
            println!("Skipping permission test in CI environment");
            return Ok(());
        }

        // 设置一个无法写入 fstab 文件的 rootfs
        let readonly_dir = TempDir::new()?;
        let readonly_path = readonly_dir.path();
        let etc_path = readonly_path.join("etc");
        fs::create_dir_all(&etc_path).await?;

        // 将 /etc 目录设置为只读，模拟权限问题
        let mut perms = fs::metadata(&etc_path).await?.permissions();
        perms.set_mode(0o400); // 只读
        fs::set_permissions(&etc_path, perms).await?;

        // 验证权限已设置（用于调试）
        let actual_perms = fs::metadata(&etc_path).await?.permissions();
        println!("Set /etc permissions to: {:o}", actual_perms.mode());

        // 尝试在只读的 /etc 目录中写入 fstab
        let host_dir = TempDir::new()?;
        let host_path = host_dir.path().join("test");
        fs::create_dir_all(&host_path).await?;

        let mapped_dirs =
            vec![format!("{}:/container/data", host_path.display()).parse::<PathPair>()?];

        // 函数应该检测到无法写入 /etc/fstab 并返回错误
        let result = patch_with_virtiofs_mounts(readonly_path, &mapped_dirs).await;

        // 详细的错误报告（用于调试）
        if result.is_ok() {
            println!("Warning: Write succeeded despite read-only permissions");
            println!(
                "Current /etc permissions: {:o}",
                fs::metadata(&etc_path).await?.permissions().mode()
            );
            if etc_path.join("fstab").exists() {
                println!(
                    "fstab file was created with permissions: {:o}",
                    fs::metadata(etc_path.join("fstab"))
                        .await?
                        .permissions()
                        .mode()
                );
            }
        }

        assert!(
            result.is_err(),
            "Expected error when writing fstab to read-only /etc directory. \
             Current /etc permissions: {:o}",
            fs::metadata(&etc_path).await?.permissions().mode()
        );
        assert!(matches!(result.unwrap_err(), MicrosandboxError::Io(_)));

        Ok(())
    }

    #[tokio::test]
    async fn test_patch_with_hostnames() -> anyhow::Result<()> {
        use std::net::Ipv4Addr;

        // 创建一个临时目录作为 rootfs
        let root_dir = TempDir::new()?;
        let root_path = root_dir.path();

        // 创建测试主机名映射
        let hostname_mappings = vec![
            (Ipv4Addr::new(192, 168, 1, 100), "host1.local".to_string()),
            (Ipv4Addr::new(192, 168, 1, 101), "host2.local".to_string()),
        ];

        // 更新 hosts 文件
        _patch_with_hostnames(root_path, &hostname_mappings).await?;

        // 验证 hosts 文件已创建且内容正确
        let hosts_path = root_path.join("etc/hosts");
        assert!(hosts_path.exists());

        let hosts_content = fs::read_to_string(&hosts_path).await?;

        // 检查头部
        assert!(hosts_content.contains("# /etc/hosts: static table lookup for hostnames"));
        assert!(hosts_content.contains("127.0.0.1\tlocalhost"));
        assert!(hosts_content.contains("::1\tlocalhost ip6-localhost ip6-loopback"));

        // 检查条目
        assert!(hosts_content.contains("192.168.1.100\thost1.local"));
        assert!(hosts_content.contains("192.168.1.101\thost2.local"));

        // 验证文件权限
        let perms = fs::metadata(&hosts_path).await?.permissions();
        assert_eq!(perms.mode() & 0o777, 0o644);

        // 测试向现有 hosts 文件添加新条目
        let new_mappings = vec![
            (Ipv4Addr::new(192, 168, 1, 100), "host1.local".to_string()), // 现有条目
            (Ipv4Addr::new(192, 168, 1, 102), "host3.local".to_string()), // 新条目
        ];

        // 再次更新 hosts 文件
        _patch_with_hostnames(root_path, &new_mappings).await?;

        // 验证更新后的内容
        let updated_content = fs::read_to_string(&hosts_path).await?;

        // 应该仍然包含原始条目
        assert!(updated_content.contains("127.0.0.1\tlocalhost"));
        assert!(updated_content.contains("::1\tlocalhost ip6-localhost ip6-loopback"));

        // 应该包含旧条目和新条目，且无重复
        assert!(updated_content.contains("192.168.1.100\thost1.local"));
        assert!(updated_content.contains("192.168.1.102\thost3.local"));

        // 统计第一个 IP 的出现次数，确保无重复
        let count = updated_content
            .lines()
            .filter(|line| line.contains("192.168.1.100"))
            .count();
        assert_eq!(count, 1, "Should not have duplicate entries");

        Ok(())
    }

    #[tokio::test]
    async fn test_patch_with_default_dns_settings() -> anyhow::Result<()> {
        // 创建一个临时目录作为 rootfs
        let root_dir = TempDir::new()?;
        let root_path = root_dir.path();

        // 测试用例 1：不存在 resolv.conf
        patch_with_default_dns_settings(&[root_path.to_path_buf()]).await?;

        // 验证 resolv.conf 已创建且内容正确
        let resolv_path = root_path.join("etc/resolv.conf");
        assert!(resolv_path.exists());

        let resolv_content = fs::read_to_string(&resolv_path).await?;

        // 检查内容
        assert!(resolv_content.contains("# /etc/resolv.conf: DNS resolver configuration"));
        assert!(resolv_content.contains("nameserver 1.1.1.1"));
        assert!(resolv_content.contains("nameserver 8.8.8.8"));

        // 验证文件权限
        let perms = fs::metadata(&resolv_path).await?.permissions();
        assert_eq!(perms.mode() & 0o777, 0o644);

        // 测试用例 2：存在 resolv.conf 但没有 nameservers
        let root_dir2 = TempDir::new()?;
        let root_path2 = root_dir2.path();
        let resolv_path2 = root_path2.join("etc/resolv.conf");
        fs::create_dir_all(resolv_path2.parent().unwrap()).await?;
        fs::write(&resolv_path2, "# Empty resolv.conf\n").await?;

        patch_with_default_dns_settings(&[root_path2.to_path_buf()]).await?;

        // 验证 nameservers 已添加
        let content2 = fs::read_to_string(&resolv_path2).await?;
        assert!(content2.contains("nameserver 1.1.1.1"));
        assert!(content2.contains("nameserver 8.8.8.8"));

        // 测试用例 3：存在 resolv.conf 且已有 nameservers
        let root_dir3 = TempDir::new()?;
        let root_path3 = root_dir3.path();
        let resolv_path3 = root_path3.join("etc/resolv.conf");
        fs::create_dir_all(resolv_path3.parent().unwrap()).await?;
        fs::write(
            &resolv_path3,
            "# Existing nameservers\nnameserver 192.168.1.1\n",
        )
        .await?;

        patch_with_default_dns_settings(&[root_path3.to_path_buf()]).await?;

        // 验证内容未被修改
        let content3 = fs::read_to_string(&resolv_path3).await?;
        assert!(content3.contains("nameserver 192.168.1.1"));
        assert!(!content3.contains("nameserver 1.1.1.1"));
        assert!(!content3.contains("nameserver 8.8.8.8"));

        // 测试用例 4：多层（overlayfs）
        let root_dir4 = TempDir::new()?;
        let lower_layer1 = root_dir4.path().join("lower1");
        let lower_layer2 = root_dir4.path().join("lower2");
        let patch_layer = root_dir4.path().join("patch");

        // 创建目录
        fs::create_dir_all(&lower_layer1).await?;
        fs::create_dir_all(&lower_layer2).await?;
        fs::create_dir_all(&patch_layer).await?;

        // 测试 4a：任何层都没有 resolv.conf
        patch_with_default_dns_settings(&[
            lower_layer1.clone(),
            lower_layer2.clone(),
            patch_layer.clone(),
        ])
        .await?;

        // 验证 resolv.conf 只在顶层创建
        assert!(!lower_layer1.join("etc/resolv.conf").exists());
        assert!(!lower_layer2.join("etc/resolv.conf").exists());
        let patch_resolv = patch_layer.join("etc/resolv.conf");
        assert!(patch_resolv.exists());
        let content = fs::read_to_string(&patch_resolv).await?;
        assert!(content.contains("nameserver 1.1.1.1"));

        // 测试 4b：下层存在包含 nameserver 的 resolv.conf
        let root_dir5 = TempDir::new()?;
        let lower_layer = root_dir5.path().join("lower");
        let patch_layer = root_dir5.path().join("patch");
        fs::create_dir_all(&lower_layer.join("etc")).await?;
        fs::create_dir_all(&patch_layer).await?;

        // 在下层创建包含 nameserver 的 resolv.conf
        fs::write(
            lower_layer.join("etc/resolv.conf"),
            "nameserver 192.168.1.1\n",
        )
        .await?;

        patch_with_default_dns_settings(&[lower_layer.clone(), patch_layer.clone()]).await?;

        // 验证顶层没有创建 resolv.conf
        assert!(!patch_layer.join("etc/resolv.conf").exists());
        let lower_content = fs::read_to_string(lower_layer.join("etc/resolv.conf")).await?;
        assert!(lower_content.contains("nameserver 192.168.1.1"));

        Ok(())
    }

    #[tokio::test]
    async fn test_patch_with_stat_override() -> anyhow::Result<()> {
        // 如果不支持 xattr，跳过此测试
        if !xattr::SUPPORTED_PLATFORM {
            println!("Skipping xattr test on unsupported platform");
            return Ok(());
        }

        // 创建一个临时目录作为 rootfs
        let root_dir = TempDir::new()?;
        let root_path = root_dir.path();

        // 设置统计信息覆盖
        patch_with_stat_override(root_path).await?;

        // 验证 xattr 已正确设置
        let xattr_value =
            xattr::get(root_path, XATTR_OVERRIDE_STATS_NAME).expect("Failed to get xattr");

        // 检查 xattr 是否设置且具有正确的值
        assert!(xattr_value.is_some(), "xattr was not set");
        assert_eq!(
            xattr_value.unwrap(),
            XATTR_OVERRIDE_STATS_VALUE.as_bytes(),
            "xattr value incorrect"
        );

        Ok(())
    }
}
