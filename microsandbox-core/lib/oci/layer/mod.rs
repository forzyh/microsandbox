//! OCI 镜像层（Layer）模块
//!
//! 本模块提供了 OCI 镜像层的核心抽象和操作 trait。
//!
//! ## 什么是镜像层？
//!
//! OCI 镜像由多个层（layer）堆叠而成，每个层是：
//! - 一个压缩的 tar 文件（通常为 tar.gz 格式）
//! - 包含文件系统的一部分（如基础系统、应用程序等）
//! - 有唯一的 digest（SHA256 哈希值）标识
//!
//! ## 层的堆叠模型
//!
//! ```text
//! ┌─────────────────────────┐
//! │   顶层（应用层）          │  ← 最后添加的层
//! ├─────────────────────────┤
//! │   中间层（依赖库）        │
//! ├─────────────────────────┤
//! │   基础层（操作系统）      │  ← 第一层
//! └─────────────────────────┘
//! ```
//!
//! ## 模块组成
//!
//! - **extraction**: 层提取逻辑，将压缩的 tar 文件解压到文件系统
//! - **progress**: 下载和提取进度条（CLI 模式）
//! - **LayerOps trait**: 层操作的核心抽象
//! - **Layer 结构体**: LayerOps 的具体实现
//! - **LayerDependencies**: 层依赖关系管理
//!
//! ## LayerOps trait
//!
//! 定义了层操作的核心接口：
//! - `digest()`: 获取层的 digest
//! - `tar_path()`: 获取压缩 tar 文件的路径
//! - `get_tar_size()`: 获取 tar 文件大小
//! - `extracted_layer_dir()`: 获取提取后的目录路径
//! - `extracted()`: 检查层是否已提取
//! - `cleanup_extracted()`: 清理已提取的层
//! - `extract()`: 提取层到文件系统
//! - `find_dir()`: 在层中查找目录

pub(crate) mod extraction;
mod progress;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_compression::tokio::bufread::GzipDecoder;
use async_trait::async_trait;

use microsandbox_utils::EXTRACTED_LAYER_SUFFIX;
use oci_spec::image::Digest;
use tokio::{
    fs,
    io::BufReader,
    sync::{Mutex, OwnedMutexGuard},
};
use tokio_tar::Archive;

use crate::{
    MicrosandboxError, MicrosandboxResult,
    oci::{
        extraction::extract_tar_with_ownership_override, global_cache::GlobalCacheOps, image::Image,
    },
};

//--------------------------------------------------------------------------------------------------
// LayerOps trait - 层操作的核心抽象
//--------------------------------------------------------------------------------------------------

/// 层操作 trait
///
/// 此 trait 定义了对 OCI 镜像层进行操作的通用接口。
/// 实现此 trait 的类型必须提供对 `GlobalCacheOps` 的访问，
/// 以便管理层的下载和提取。
///
/// ## 设计原理
///
/// 使用 trait 而非具体类型的原因：
/// 1. **测试 mock**: 可以在测试中创建假的 LayerOps 实现
/// 2. **灵活性**: 未来可以添加不同的层实现
/// 3. **解耦**: 与具体的缓存实现解耦
///
/// ## 核心方法
///
/// | 方法 | 用途 |
/// |------|------|
/// | `digest()` | 获取层的唯一标识符 |
/// | `tar_path()` | 获取压缩 tar 文件路径 |
/// | `get_tar_size()` | 获取 tar 文件大小（用于断点续传） |
/// | `extracted()` | 检查层是否已提取（带锁） |
/// | `extract()` | 提取层到文件系统 |
/// | `find_dir()` | 在层中搜索目录 |
#[async_trait]
pub(crate) trait LayerOps: Send + Sync {
    /// 获取全局层操作接口的引用
    ///
    /// 此方法返回对 `GlobalCacheOps` 的引用，用于访问：
    /// - 下载目录（tar 文件存储位置）
    /// - 提取目录（解压后的文件系统）
    /// - 数据库连接（元数据存储）
    fn global_layer_ops(&self) -> &dyn GlobalCacheOps;

    /// 获取层的 digest
    ///
    /// Digest 是层的唯一标识符，通常是 SHA256 哈希值。
    /// 格式：`sha256:abcdef123456...`
    ///
    /// ## 返回值
    ///
    /// 返回层的 `Digest` 引用
    fn digest(&self) -> &Digest;

    /// 获取层 tar 文件的路径
    ///
    /// 根据层的 digest 计算 tar 文件的存储路径：
    /// `<download_dir>/<digest>.tar`
    ///
    /// ## 返回值
    ///
    /// 返回 tar 文件的完整路径
    ///
    /// ## 示例
    ///
    /// ```text
    /// /home/user/.microsandbox/layers/sha256.abc123.tar
    /// ```
    fn tar_path(&self) -> PathBuf {
        self.global_layer_ops()
            .tar_download_dir()
            .join(self.digest().to_string())
            .with_extension("tar")
    }

    /// 获取层 tar 文件的大小
    ///
    /// 此方法用于：
    /// 1. 断点续传：计算已下载的大小
    /// 2. 进度条：显示总大小
    ///
    /// ## 返回值
    ///
    /// - `Some(size)`: 文件存在时返回大小（字节）
    /// - `None`: 文件不存在
    ///
    /// ## 实现细节
    ///
    /// 使用 `std::fs::metadata` 获取文件大小，
    /// 如果文件不存在或无法访问则返回 `None`。
    fn get_tar_size(&self) -> Option<u64> {
        let tar_path = self.tar_path();
        if !tar_path.exists() {
            return None;
        }

        let len = tar_path
            .metadata()
            .expect("Failed to get layer file metadata")
            .len();

        Some(len)
    }

    /// 获取提取后的层目录路径
    ///
    /// 提取后的层存储在：
    /// `<extracted_layers_dir>/<digest>.extracted`
    ///
    /// 使用 `.extracted` 后缀是为了方便识别哪些目录是已提取的层。
    ///
    /// ## 返回值
    ///
    /// 返回提取后的层目录路径
    ///
    /// ## 示例
    ///
    /// ```text
    /// /home/user/.microsandbox/layers/sha256.abc123.extracted/
    /// ```
    fn extracted_layer_dir(&self) -> PathBuf {
        let file_name = self.digest().to_string();
        self.global_layer_ops()
            .extracted_layers_dir()
            .join(format!("{}.{}", file_name, EXTRACTED_LAYER_SUFFIX))
    }

    /// 检查层是否已提取
    ///
    /// 此方法不仅检查目录是否存在，还验证目录是否有内容。
    ///
    /// ## 返回值
    ///
    /// 返回 `(bool, OwnedMutexGuard<()>)` 元组：
    /// - `bool`: 层是否已提取且有内容
    /// - `OwnedMutexGuard<()>`: 互斥锁的 guard，用于防止并发提取
    ///
    /// ## 为什么返回锁？
    ///
    /// 提取操作是并发的，但每个层只能被提取一次。
    /// 返回锁的 guard 确保：
    /// 1. 调用者在提取期间持有锁
    /// 2. 其他任务必须等待锁释放
    /// 3. 避免重复提取或竞争条件
    ///
    /// ## 实现细节
    ///
    /// 1. 获取互斥锁（`self.lock.lock_owned()`）
    /// 2. 检查目录是否存在
    /// 3. 如果存在，读取目录并检查是否有内容
    /// 4. 返回结果和锁的 guard
    async fn extracted(&self) -> MicrosandboxResult<(bool, OwnedMutexGuard<()>)>;

    /// 清理已提取的层目录
    ///
    /// 如果提取失败或需要重新提取，使用此方法清理已提取的内容。
    ///
    /// ## 实现细节
    ///
    /// 1. 获取互斥锁
    /// 2. 检查目录是否存在
    /// 3. 如果存在，使用 `remove_dir_all` 删除整个目录
    /// 4. 记录日志
    async fn cleanup_extracted(&self) -> MicrosandboxResult<()>;

    /// 提取层到文件系统
    ///
    /// 这是层处理的核心方法，会：
    /// 1. 检查层是否已提取（避免重复工作）
    /// 2. 创建目标目录
    /// 3. 打开 tar 文件
    /// 4. 使用 gzip 解压
    /// 5. 提取到目标目录
    /// 6. 更新进度条（CLI 模式）
    ///
    /// ## 参数
    ///
    /// * `parent` - 父层依赖关系
    ///   - 包含当前层 digest 和所有父层
    ///   - 用于在提取时处理文件所有权
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 提取成功
    /// * `Err(MicrosandboxError)` - 提取失败
    ///
    /// ## 提取流程
    ///
    /// ```text
    /// extract()
    ///   │
    ///   ├─> 1. 检查是否已提取（已提取则直接返回）
    ///   │
    ///   ├─> 2. 创建目标目录
    ///   │
    ///   ├─> 3. 打开 tar 文件
    ///   │
    ///   ├─> 4. 创建 gzip 解码器
    ///   │
    ///   ├─> 5. 创建进度条（CLI 模式）
    ///   │
    ///   ├─> 6. 提取 tar 到目录（带所有权处理）
    ///   │
    ///   └─> 7. 完成进度条
    /// ```
    ///
    /// ## 为什么需要父层依赖？
    ///
    /// OCI 镜像层是叠加的，每个层可能包含：
    /// - 新增的文件
    /// - 修改的文件（覆盖父层的同名文件）
    /// - 删除的文件（使用 whiteout 文件标记）
    ///
    /// 提取时需要知道父层的信息来正确处理：
    /// - 文件所有权（继承父层的用户/组）
    /// - whiteout 文件（删除父层的文件）
    async fn extract(&self, parent: LayerDependencies) -> MicrosandboxResult<()>;

    /// 在层中搜索目录
    ///
    /// 此方法用于在提取后的层目录中查找指定的目录。
    ///
    /// ## 参数
    ///
    /// * `path_in_tar` - tar 文件中的路径（例如 `etc/nginx`）
    ///
    /// ## 返回值
    ///
    /// 如果目录存在，返回提取后的规范路径；否则返回 `None`
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// let nginx_dir = layer.find_dir(Path::new("etc/nginx")).await;
    /// if let Some(path) = nginx_dir {
    ///     println!("找到 nginx 配置目录：{:?}", path);
    /// }
    /// ```
    async fn find_dir(&self, path_in_tar: &Path) -> Option<PathBuf>;
}

/// OCI 镜像层的具体实现
///
/// `Layer` 是 `LayerOps` trait 的具体实现，负责：
/// 1. 存储层的 digest
/// 2. 管理提取锁（防止并发提取同一层）
/// 3. 引用全局缓存操作接口
///
/// ## 字段说明
///
/// - `global_layer_ops`: 全局缓存操作接口，用于访问下载/提取目录
/// - `lock`: 互斥锁，确保同一层不会被并发提取
/// - `digest`: 层的唯一标识符（SHA256）
///
/// ## 为什么需要锁？
///
/// 镜像通常有多个层，提取是并发进行的。但同一个层：
/// - 只能被提取一次
/// - 提取过程中其他任务必须等待
/// - 提取完成后可以复用结果
///
/// 使用 `Arc<Mutex<()>>` 实现：
/// - `Arc`: 多个层对象可以共享同一个锁
/// - `Mutex<()>`: 互斥锁，guard 类型为 `OwnedMutexGuard<()>`
#[derive(Clone)]
pub struct Layer {
    /// 全局缓存操作接口
    global_layer_ops: Arc<dyn GlobalCacheOps>,
    /// 提取锁，防止并发提取同一层
    lock: Arc<Mutex<()>>,
    /// 层的 digest
    digest: Digest,
}

impl Layer {
    /// 创建新的层对象
    ///
    /// ## 参数
    ///
    /// * `global_layer_ops` - 全局缓存操作接口
    /// * `digest` - 层的 digest
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `Layer` 实例
    ///
    /// ## 示例
    ///
    /// ```rust,ignore
    /// let layer = Layer::new(cache_ops, digest.clone());
    /// ```
    pub fn new(global_layer_ops: Arc<dyn GlobalCacheOps>, digest: Digest) -> Self {
        Self {
            global_layer_ops,
            digest,
            lock: Arc::new(Mutex::new(())),
        }
    }
}

#[async_trait]
impl LayerOps for Layer {
    fn global_layer_ops(&self) -> &dyn GlobalCacheOps {
        self.global_layer_ops.as_ref()
    }

    fn digest(&self) -> &Digest {
        &self.digest
    }

    /// 检查层是否已提取
    ///
    /// 此方法实现 `LayerOps::extracted()`。
    ///
    /// ## 实现细节
    ///
    /// 1. 获取互斥锁（`self.lock.clone().lock_owned().await`）
    ///    - 使用 `clone()` 是因为 `lock_owned()` 消耗所有权
    ///    - 返回的 guard 会持有锁直到被丢弃
    ///
    /// 2. 检查目录是否存在
    ///    - 如果不存在，返回 `(false, guard)`
    ///
    /// 3. 检查目录是否有内容
    ///    - 使用 `read_dir().next_entry()` 检查是否有第一个条目
    ///    - 如果有，返回 `(true, guard)`
    ///    - 如果目录存在但为空，记录警告并返回 `(false, guard)`
    ///
    /// ## 为什么检查目录内容？
    ///
    /// 可能的边缘情况：
    /// - 提取过程中断，留下空目录
    /// - 文件系统错误导致目录损坏
    ///
    /// 仅当目录有内容时才认为是"已提取"状态。
    async fn extracted(&self) -> MicrosandboxResult<(bool, OwnedMutexGuard<()>)> {
        let guard = self.lock.clone().lock_owned().await;
        let dir = self.extracted_layer_dir();
        if !dir.exists() {
            return Ok((false, guard));
        }

        // 检查层目录是否有内容
        let mut read_dir = fs::read_dir(&dir).await?;
        let next = read_dir.next_entry().await;
        if next.is_ok() {
            tracing::debug!(digest = %self.digest(), "layer directory has content");
            return Ok((true, guard));
        }

        tracing::warn!(digest = %self.digest(), "layer exists but is empty");
        Ok((false, guard))
    }

    /// 清理已提取的层目录
    ///
    /// 此方法实现 `LayerOps::cleanup_extracted()`。
    ///
    /// ## 实现细节
    ///
    /// 1. 获取互斥锁
    /// 2. 检查目录是否存在
    /// 3. 如果存在，使用 `remove_dir_all` 删除
    /// 4. 记录日志（成功或失败）
    ///
    /// ## 错误处理
    ///
    /// 如果删除失败：
    /// - 使用 `inspect_err` 记录错误
    /// - 使用 `?` 向上传播错误
    async fn cleanup_extracted(&self) -> MicrosandboxResult<()> {
        let _guard = self.lock.lock().await;
        let layer_path = self.extracted_layer_dir();
        if layer_path.exists() {
            tracing::debug!(layer_path = %layer_path.display(), "Cleaning up extracted layer");

            tokio::fs::remove_dir_all(&layer_path)
                .await
                .inspect_err(|err| {
                    tracing::error!(?err, "Failed to clean extracted layer");
                })?;
        }

        Ok(())
    }

    /// 提取层到文件系统
    ///
    /// 此方法实现 `LayerOps::extract()`。
    ///
    /// ## tracing 注解
    ///
    /// 使用 `#[tracing::instrument]` 宏自动记录日志：
    /// - `extract_dir`: 提取目录路径
    /// - `digest`: 层的 digest
    ///
    /// ## 实现细节
    ///
    /// 1. 断言当前层与父依赖的 digest 匹配
    /// 2. 检查是否已提取（已提取则直接返回）
    /// 3. 创建提取目录
    /// 4. 打开 tar 文件
    /// 5. 创建 gzip 解码器
    /// 6. 创建进度条（CLI 模式）
    /// 7. 提取 tar 到目录
    /// 8. 完成进度条
    #[tracing::instrument(skip_all, fields(
        extract_dir = %self.extracted_layer_dir().display(),
        digest = %self.digest(),
    ))]
    async fn extract(&self, parent: LayerDependencies) -> MicrosandboxResult<()> {
        // 断言：当前层的 digest 必须与父依赖的 digest 匹配
        assert_eq!(self.digest(), parent.digest());

        // 检查是否已提取，如果已提取则直接返回（避免重复工作）
        let (false, _guard) = self.extracted().await? else {
            return Ok(());
        };

        // 获取 tar 文件路径
        let layer_path = self.tar_path();
        let digest = self.digest().clone();
        let extract_dir = self.extracted_layer_dir();

        // 创建提取目录（如果不存在）
        fs::create_dir_all(&extract_dir).await.map_err(|source| {
            MicrosandboxError::LayerHandling {
                layer: digest.to_string(),
                source,
            }
        })?;

        tracing::info!("Extracting layer");

        // 打开 tar 文件
        let file = tokio::fs::File::open(&layer_path).await?;

        // CLI 模式：创建进度条
        #[cfg(feature = "cli")]
        let (file, pb) = {
            use crate::oci::layer::progress::{ProgressReader, build_progress_bar};

            let total_bytes = fs::metadata(&layer_path).await?.len();
            let bar = build_progress_bar(total_bytes, &digest.digest()[..8]);
            let bar_clone = bar.clone();
            (ProgressReader { inner: file, bar }, bar_clone)
        };

        // 创建 tar archive（带 gzip 解压）
        let mut archive = Archive::new(GzipDecoder::new(BufReader::new(file)));

        // 提取 tar 到目录（带所有权处理）
        extract_tar_with_ownership_override(&mut archive, &extract_dir, parent)
            .await
            .map_err(|e| MicrosandboxError::LayerExtraction(format!("{e:?}")))?;

        // CLI 模式：完成进度条
        #[cfg(feature = "cli")]
        pb.finish_and_clear();

        tracing::info!("Successfully extracted layer");
        Ok(())
    }

    /// 在层中搜索目录
    ///
    /// 此方法实现 `LayerOps::find_dir()`。
    ///
    /// ## 实现细节
    ///
    /// 1. 计算规范路径：`<extracted_dir>/<path>`
    /// 2. 检查路径是否存在且是目录
    /// 3. 如果满足条件，返回路径；否则返回 `None`
    ///
    /// ## 注意
    ///
    /// 此方法不会在父层中搜索，仅检查当前层。
    /// 如果需要在父层中搜索，使用 `LayerDependencies::find_dir()`。
    async fn find_dir(&self, path: &Path) -> Option<PathBuf> {
        let canonical_path = self.extracted_layer_dir().join(path);
        if canonical_path.exists() && canonical_path.is_dir() {
            return Some(canonical_path);
        }

        None
    }
}

/// 层依赖关系集合
///
/// 此结构体封装了对某个层的所有依赖（即它的所有父层）。
///
/// ## 为什么需要层依赖？
///
/// OCI 镜像层是叠加的，每个层基于它的所有父层构建。
/// 在提取层时，可能需要：
/// 1. 复制父层的文件（保持所有权）
/// 2. 处理 whiteout 文件（删除父层的文件）
/// 3. 覆盖父层的文件
///
/// ## 字段说明
///
/// - `layer`: 当前关注的层的 digest
/// - `image`: 当前层所属的镜像（包含所有父层）
///
/// ## 使用示例
///
/// ```rust,ignore
/// // 从 Image 获取层的父依赖
/// let parent_deps = image.get_layer_parent(&digest);
///
/// // 在父层中搜索目录
/// let (found_digest, path) = parent_deps.find_dir("etc/nginx").await?;
/// ```
#[derive(Clone)]
pub(crate) struct LayerDependencies {
    /// 当前关注的层的 digest
    layer: Digest,
    /// 当前层所属的镜像（包含所有父层）
    image: Image,
}

impl LayerDependencies {
    /// 创建新的层依赖关系
    ///
    /// ## 参数
    ///
    /// * `layer` - 当前关注的层的 digest
    /// * `image` - 当前层所属的镜像
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `LayerDependencies` 实例
    pub(crate) fn new(layer: Digest, image: Image) -> Self {
        Self { layer, image }
    }

    /// 获取当前层的 digest
    ///
    /// ## 返回值
    ///
    /// 返回当前层 digest 的引用
    pub(crate) fn digest(&self) -> &Digest {
        &self.layer
    }

    /// 在所有父层中搜索目录
    ///
    /// 此方法会遍历所有父层（从基础层到当前层的前一层），
    /// 返回第一个包含指定目录的层。
    ///
    /// ## 参数
    ///
    /// * `path` - 要搜索的路径（例如 `etc/nginx`）
    ///
    /// ## 返回值
    ///
    /// - `Ok(Some((digest, path)))`: 找到目录，返回层的 digest 和路径
    /// - `Ok(None)`: 所有父层都不包含该目录
    /// - `Err(MicrosandboxError)`: 提取过程中发生错误
    ///
    /// ## 实现细节
    ///
    /// 1. 遍历镜像的所有层（从基础层到顶层）
    /// 2. 对于每个层，先提取（如果未提取）
    /// 3. 使用 `find_dir()` 在该层中搜索
    /// 4. 如果找到，返回该层的 digest 和路径
    /// 5. 如果遍历完成都未找到，返回 `None`
    ///
    /// ## 为什么按顺序遍历？
    ///
    /// OCI 镜像层是从基础层到顶层叠加的：
    /// - 基础层在最下面
    /// - 顶层在最上面
    ///
    /// 当搜索目录时，应该返回"最上层"的那个版本。
    /// 但由于我们是从头开始遍历，先找到的是"最下层"的版本。
    /// 这个方法会返回第一个找到的结果，可能不是最上层版本。
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// // 在父层中搜索 nginx 配置目录
    /// if let Some((digest, path)) = parent_deps.find_dir("etc/nginx").await? {
    ///     println!("在层 {} 找到 nginx 配置目录：{:?}", digest, path);
    /// }
    /// ```
    pub(crate) async fn find_dir(
        &self,
        path: impl AsRef<Path>,
    ) -> MicrosandboxResult<Option<(Digest, PathBuf)>> {
        let path = path.as_ref().to_path_buf();

        // 遍历所有父层（从基础层到顶层）
        // 如果层未提取，先提取它
        // 如果在层中找到文件，返回该层的 digest 和路径
        for layer in self.image.layers().iter() {
            // 提取当前层（如果未提取）
            layer
                .extract(self.image.get_layer_parent(layer.digest()))
                .await?;

            // 在当前层中搜索
            if let Some(path) = layer.find_dir(&path).await {
                return Ok(Some((layer.digest().clone(), path)));
            }
        }

        Ok(None)
    }
}
