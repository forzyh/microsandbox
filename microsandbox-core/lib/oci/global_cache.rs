//! OCI 全局缓存模块
//!
//! 本模块定义了全局层缓存的 trait 和实现。
//!
//! ## 什么是全局缓存？
//!
//! 全局缓存（`GlobalCache`）管理 OCI 镜像层的存储和元数据：
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      GlobalCache                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │  存储目的地：                                               │
//! │  • tar_download_dir    - 下载的层 tar 文件（压缩）            │
//! │  • extracted_layers_dir - 提取的层目录（解压后）             │
//! │  • db (Sqlite)         - 层元数据（manifest, config 等）      │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 为什么需要全局缓存？
//!
//! 1. **避免重复下载**: 多个镜像可能共享相同的层
//! 2. **元数据管理**: 使用 SQLite 存储镜像和层的信息
//! 3. **统一接口**: `GlobalCacheOps` trait 提供抽象层
//!
//! ## 层的存储结构
//!
//! ```text
//! MICROSANDBOX_HOME/layers/
//! ├── sha256.abc123...tar        # 下载的压缩层
//! ├── sha256.def456...tar        # 下载的压缩层
//! ├── sha256.abc123...extracted/ # 提取的层目录
//! ├── sha256.def456...extracted/ # 提取的层目录
//! └── ...
//! ```
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! // 创建全局缓存
//! let cache = GlobalCache::new(
//!     tar_download_dir.clone(),
//!     extracted_layers_dir.clone(),
//!     db_pool.clone(),
//! ).await?;
//!
//! // 获取层操作接口
//! let layer = cache.build_layer(&digest).await;
//!
//! // 检查层是否已下载
//! if let Some(layer) = cache.get_downloaded_layer(&digest).await {
//!     // 层已下载
//! }
//!
//! // 检查镜像的所有层是否已提取
//! if cache.all_layers_extracted(&reference).await? {
//!     // 所有层都已提取，可以跳过
//! }
//! ```

use std::{path::PathBuf, str::FromStr, sync::Arc};

use async_trait::async_trait;
use oci_spec::image::Digest;
use sqlx::{Pool, Sqlite};
use tokio::fs;

use crate::{
    MicrosandboxResult,
    management::db,
    oci::{
        Reference,
        layer::{Layer, LayerOps},
    },
};

//--------------------------------------------------------------------------------------------------
// GlobalCacheOps trait - 全局缓存操作的核心抽象
//--------------------------------------------------------------------------------------------------

/// 全局缓存操作 trait
///
/// 此 trait 定义了对全局层缓存进行操作的通用接口。
/// 实现此 trait 的类型必须提供对以下资源的访问：
/// - tar 下载目录（压缩层文件存储位置）
/// - 提取目录（解压后的文件系统）
/// - 数据库连接（元数据存储）
///
/// ## 设计原理
///
/// 使用 trait 而非具体类型的原因：
/// 1. **测试 mock**: 可以在测试中创建假的 GlobalCacheOps 实现
/// 2. **灵活性**: 未来可以添加不同的缓存实现
/// 3. **解耦**: 与具体的缓存实现解耦
///
/// ## 核心方法
///
/// | 方法 | 用途 |
/// |------|------|
/// | `tar_download_dir()` | 获取 tar 文件下载目录 |
/// | `extracted_layers_dir()` | 获取提取后的层目录 |
/// | `build_layer()` | 构建层操作接口 |
/// | `get_downloaded_layer()` | 获取已下载的层（如果存在） |
/// | `all_layers_extracted()` | 检查镜像的所有层是否都已提取 |
#[async_trait]
pub trait GlobalCacheOps: Send + Sync {
    /// 获取 tar 文件下载目录的引用
    ///
    /// 此目录存储从 OCI 注册表下载的压缩层文件（tar.gz 格式）。
    /// 文件命名格式：`<digest>.tar`
    ///
    /// ## 返回值
    ///
    /// 返回下载目录的 `PathBuf` 引用
    ///
    /// ## 使用示例
    ///
    /// ```text
    /// /home/user/.microsandbox/layers/
    /// ```
    fn tar_download_dir(&self) -> &PathBuf;

    /// 获取提取后的层目录的引用
    ///
    /// 此目录存储解压后的层文件系统。
    /// 目录命名格式：`<digest>.extracted/`
    ///
    /// ## 返回值
    ///
    /// 返回提取目录的 `PathBuf` 引用
    ///
    /// ## 使用示例
    ///
    /// ```text
    /// /home/user/.microsandbox/layers/
    /// ```
    fn extracted_layers_dir(&self) -> &PathBuf;

    /// 构建层操作接口
    ///
    /// 此方法根据层的 digest 创建或获取层操作接口。
    /// 返回的接口可以用于：
    /// - 下载层 tar 文件
    /// - 提取层到文件系统
    /// - 检查层状态
    ///
    /// ## 参数
    ///
    /// * `digest` - 层的 digest（SHA256 哈希值）
    ///
    /// ## 返回值
    ///
    /// 返回实现了 `LayerOps` trait 的对象，包装在 `Arc` 中
    ///
    /// ## 为什么返回 Arc？
    ///
    /// 层操作接口会被多个任务共享使用（如并发提取），
    /// 所以使用 `Arc`（原子引用计数）来安全地共享所有权。
    async fn build_layer(&self, digest: &Digest) -> Arc<dyn LayerOps>;

    /// 获取已下载的层
    ///
    /// 此方法检查层的 tar 文件是否已存在于磁盘上，
    /// 如果存在则返回层操作接口，否则返回 `None`。
    ///
    /// ## 参数
    ///
    /// * `digest` - 层的 digest
    ///
    /// ## 返回值
    ///
    /// - `Some(Arc<dyn LayerOps>)`: tar 文件存在时返回层接口
    /// - `None`: tar 文件不存在或目录为空
    ///
    /// ## 实现细节
    ///
    /// 1. 构建层操作接口
    /// 2. 检查 tar 文件路径是否存在
    /// 3. 如果不存在，返回 `None`
    /// 4. 如果存在，检查文件所在目录是否有内容
    /// 5. 防止空目录被误认为是有效的层
    ///
    /// ## 为什么检查目录内容？
    ///
    /// 可能的边缘情况：
    /// - 下载过程中断，留下空的 tar 文件
    /// - 文件系统错误导致文件损坏
    async fn get_downloaded_layer(&self, digest: &Digest) -> Option<Arc<dyn LayerOps>> {
        let layer = self.build_layer(digest).await;
        let tar_path = layer.tar_path();
        if !tar_path.exists() {
            tracing::warn!(?digest, tar_path = %tar_path.display(), "layer does not exist");
            return None;
        }

        // 检查 tar 文件所在目录是否有内容
        // 如果目录存在但至少有一个文件，认为层已下载
        let parent = tar_path.parent().expect("tar path to have parent");
        if let Ok(mut read_dir) = tokio::fs::read_dir(parent).await
            && let Ok(Some(_)) = read_dir.next_entry().await
        {
            return Some(layer);
        }

        tracing::warn!(?digest, "layer exists but is empty");
        None
    }

    /// 检查镜像的所有层是否都已提取
    ///
    /// 此方法验证镜像的所有层是否：
    /// 1. 存在于数据库中（有元数据记录）
    /// 2. 存在于提取目录中（有实际的文件系统）
    /// 3. 层计数匹配（数据库和文件系统一致）
    ///
    /// ## 参数
    ///
    /// * `image` - 要检查的镜像引用
    ///
    /// ## 返回值
    ///
    /// - `Ok(true)`: 所有层都存在且有效
    /// - `Ok(false)`: 有任何层缺失或无效
    /// - `Err(MicrosandboxError)`: 检查过程中发生错误
    ///
    /// ## 检查流程
    ///
    /// ```text
    /// all_layers_extracted()
    ///   │
    ///   ├─> 1. 检查数据库中是否存在镜像记录
    ///   │
    ///   ├─> 2. 获取镜像的所有层 digest 列表
    ///   │
    ///   ├─> 3. 遍历每个 digest：
    ///   │     ├─ 构建层操作接口
    ///   │     ├─ 检查是否已提取（调用 layer.extracted()）
    ///   │     └─ 如果有未提取的层，返回 false
    ///   │
    ///   ├─> 4. 获取数据库中的镜像 config
    ///   │
    ///   ├─> 5. 解析 config 中的 diff_ids（层 digest 列表）
    ///   │
    ///   ├─> 6. 比较数据库 digest 数量和 diff_ids 数量
    ///   │     └─ 如果不匹配，返回 false
    ///   │
    ///   └─> 7. 所有检查通过，返回 true
    /// ```
    ///
    /// ## 为什么需要检查数据库和文件系统？
    ///
    /// - **数据库**: 存储镜像的元数据（manifest, config, 层 digest 列表）
    /// - **文件系统**: 存储实际的层文件系统（提取后的内容）
    ///
    /// 两者必须一致才能确保镜像可以正常使用。
    ///
    /// ## 为什么检查 diff_ids？
    ///
    /// OCI 镜像 config 中的 `rootfs.diff_ids` 字段存储了所有层的 digest。
    /// 通过比较数据库中的层 digest 数量和 diff_ids 数量，
    /// 可以确保数据库记录完整，没有丢失任何层的信息。
    async fn all_layers_extracted(&self, image: &Reference) -> MicrosandboxResult<bool>;
}

//--------------------------------------------------------------------------------------------------
// GlobalCache 结构体 - 全局缓存的具体实现
//--------------------------------------------------------------------------------------------------

/// 全局缓存的具体实现
///
/// 此结构体是 `GlobalCacheOps` trait 的具体实现，负责管理：
/// - tar 下载目录（压缩层文件存储位置）
/// - 提取目录（解压后的文件系统）
/// - 数据库池（层元数据存储）
///
/// ## 字段说明
///
/// - `tar_download_dir`: 存储下载的层 tar 文件（压缩格式）
/// - `extracted_layers_dir`: 存储提取后的层目录（解压后的文件系统）
/// - `db`: SQLite 数据库池，存储镜像和层的元数据
///
/// ## 为什么使用 SQLite？
///
/// SQLite 是轻量级的嵌入式数据库，适合存储：
/// - 镜像的 manifest（镜像清单）
/// - 镜像的 config（镜像配置）
/// - 层的 digest 列表
/// - 镜像与层的关系
///
/// 使用数据库可以避免：
/// - 重复下载相同的层
/// - 重复提取相同的层
/// - 丢失镜像和层的关联关系
#[derive(Clone)]
pub(crate) struct GlobalCache {
    /// 存储下载的层 tar 文件的目录
    tar_download_dir: PathBuf,

    /// 存储提取后的层目录的目录
    extracted_layers_dir: PathBuf,

    /// SQLite 数据库池，用于存储和查询层元数据
    db: Pool<Sqlite>,
}

impl GlobalCache {
    /// 创建新的全局缓存
    ///
    /// ## 参数
    ///
    /// * `tar_download_dir` - tar 文件下载目录
    /// * `extracted_layers_dir` - 提取后的层目录
    /// * `db` - SQLite 数据库池
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `GlobalCache` 实例
    ///
    /// ## 初始化流程
    ///
    /// 1. 创建 `GlobalCache` 结构体
    /// 2. 确保提取目录存在（调用 `ensure_layers_dir()`）
    /// 3. 返回结果
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// let cache = GlobalCache::new(
    ///     tar_download_dir.clone(),
    ///     extracted_layers_dir.clone(),
    ///     db_pool.clone(),
    /// ).await?;
    /// ```
    pub async fn new(
        tar_download_dir: PathBuf,
        extracted_layers_dir: PathBuf,
        db: Pool<Sqlite>,
    ) -> MicrosandboxResult<Self> {
        let this = Self {
            tar_download_dir,
            extracted_layers_dir,
            db,
        };
        // 确保提取目录存在（如果不存在则创建）
        this.ensure_layers_dir().await?;
        Ok(this)
    }

    /// 确保提取目录存在
    ///
    /// 此方法检查提取目录是否存在，如果不存在则创建。
    /// 使用 `create_dir_all` 可以创建多级目录（如果父目录不存在）。
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 目录存在或创建成功
    /// - `Err(MicrosandboxError)`: 创建目录失败
    ///
    /// ## 为什么只需要创建提取目录？
    ///
    /// tar 下载目录通常在创建全局缓存之前就已经准备好，
    /// 而提取目录可能需要在初始化时动态创建。
    async fn ensure_layers_dir(&self) -> MicrosandboxResult<()> {
        fs::create_dir_all(&self.extracted_layers_dir).await?;
        Ok(())
    }
}

#[async_trait]
impl GlobalCacheOps for GlobalCache {
    async fn ensure_layers_dir(&self) -> MicrosandboxResult<()> {
        // 创建提取目录（包括所有父目录）
        fs::create_dir_all(&self.extracted_layers_dir).await?;
        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// GlobalCacheOps trait 实现
//--------------------------------------------------------------------------------------------------

#[async_trait]
impl GlobalCacheOps for GlobalCache {
    fn tar_download_dir(&self) -> &PathBuf {
        &self.tar_download_dir
    }

    fn extracted_layers_dir(&self) -> &PathBuf {
        &self.extracted_layers_dir
    }

    /// 构建层操作接口
    ///
    /// 此方法实现 `GlobalCacheOps::build_layer()`。
    /// 创建一个新的 `Layer` 对象，包装当前全局缓存的引用和层的 digest。
    ///
    /// ## 实现细节
    ///
    /// 1. 使用 `Arc::new(self.clone())` 创建全局缓存的共享引用
    /// 2. 使用 `Layer::new()` 创建层对象
    /// 3. 返回 `Arc<dyn LayerOps>` trait 对象
    ///
    /// ## 为什么需要 clone？
    ///
    /// `GlobalCache` 实现了 `Clone` trait，所以可以安全地克隆。
    /// 使用 `Arc` 包装是为了让多个 `Layer` 对象可以共享同一个全局缓存。
    async fn build_layer(&self, digest: &Digest) -> Arc<dyn LayerOps> {
        Arc::new(Layer::new(Arc::new(self.clone()), digest.clone()))
    }

    /// 检查镜像的所有层是否都已提取
    ///
    /// 此方法实现 `GlobalCacheOps::all_layers_extracted()`。
    ///
    /// ## 实现细节
    ///
    /// ### 步骤 1: 检查数据库中是否存在镜像记录
    ///
    /// 使用 `db::image_exists()` 检查镜像引用是否在数据库中。
    /// - 如果返回 `Ok(true)`: 继续下一步
    /// - 如果返回 `Ok(false)` 或 `Err(_)`: 记录警告并返回 `Ok(false)`
    ///
    /// ### 步骤 2: 获取镜像的所有层 digest 列表
    ///
    /// 使用 `db::get_image_layer_digests()` 从数据库获取层 digest 列表。
    /// - 如果成功且非空：继续下一步
    /// - 如果失败或为空：记录警告并返回 `Ok(false)`
    ///
    /// ### 步骤 3: 遍历每个层 digest
    ///
    /// 对于每个 digest：
    /// 1. 从字符串解析 `Digest` 对象
    /// 2. 构建层操作接口（`build_layer()`）
    /// 3. 调用 `layer.extracted()` 检查是否已提取
    /// 4. 如果返回 `(false, _)`，记录警告并返回 `Ok(false)`
    ///
    /// ### 步骤 4: 获取镜像 config
    ///
    /// 使用 `db::get_image_config()` 从数据库获取镜像配置。
    /// - 如果返回 `None`: 记录警告并返回 `Ok(false)`
    ///
    /// ### 步骤 5: 解析 diff_ids
    ///
    /// 从 config 中提取 `rootfs_diff_ids_json` 字段，解析为字符串数组。
    /// - 如果解析失败：记录警告并返回 `Ok(false)`
    ///
    /// ### 步骤 6: 比较层数量
    ///
    /// 比较 diff_ids 数量和层 digest 数量：
    /// - 如果不匹配：记录警告并返回 `Ok(false)`
    ///
    /// ### 步骤 7: 所有检查通过
    ///
    /// 如果所有检查都通过，返回 `Ok(true)`。
    ///
    /// ## 返回值
    ///
    /// - `Ok(true)`: 所有层都存在且有效
    /// - `Ok(false)`: 有任何层缺失、未提取或数据不一致
    ///
    /// ## 为什么需要这么多检查？
    ///
    /// 镜像提取是一个复杂的过程，可能出现各种问题：
    /// - 数据库记录存在但文件丢失
    /// - 文件存在但提取不完整
    /// - 层数量不匹配（数据库损坏）
    ///
    /// 通过多层检查，可以确保镜像状态的准确性。
    async fn all_layers_extracted(&self, image: &Reference) -> MicrosandboxResult<bool> {
        // 步骤 1: 检查数据库中是否存在镜像记录
        match db::image_exists(&self.db, &image.to_string()).await {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(?image, "Image does not exist in db");
                return Ok(false);
            }
            Err(err) => {
                tracing::warn!(?err, ?image, "Error checking image existence");
                return Ok(false);
            }
        }

        // 步骤 2: 获取镜像的所有层 digest 列表
        let layer_digests = match db::get_image_layer_digests(&self.db, &image.to_string()).await {
            Ok(layer_digests) => layer_digests,
            Err(err) => {
                tracing::warn!(?err, ?image, "Error checking layer digests");
                return Ok(false);
            }
        };

        tracing::info!(?image, ?layer_digests, "Layer digests");
        if layer_digests.is_empty() {
            tracing::warn!(?image, "No layers found for image");
            return Ok(false);
        }

        // 步骤 3: 遍历每个层 digest，检查是否已提取
        for digest in &layer_digests {
            // 从字符串解析 Digest 对象
            let digest = Digest::from_str(digest)?;
            // 构建层操作接口
            let layer = self.build_layer(&digest).await;

            // 检查层是否已提取（调用 layer.extracted()）
            let (extracted, _) = layer.extracted().await?;
            if !extracted {
                tracing::warn!(?digest, "Layer not fully extracted");
                return Ok(false);
            }

            tracing::trace!(?digest, "Layer fully extracted and valid");
        }

        // 步骤 4: 获取数据库中的镜像 config
        let Some(config) = db::get_image_config(&self.db, &image.to_string()).await? else {
            tracing::warn!(?image, "Image config does not exist in db");
            return Ok(false);
        };

        // 步骤 5: 解析 config 中的 diff_ids
        let Some(diff_ids) = &config.rootfs_diff_ids_json else {
            tracing::warn!(?image, "Failed to parse rootfs diff ids from db");
            return Ok(false);
        };

        let diff_ids = serde_json::from_str::<Vec<String>>(diff_ids)
            .map_err(|_| anyhow::anyhow!("Failed to parse rootfs diff ids"))?;

        // 步骤 6: 比较层数量
        if diff_ids.len() != layer_digests.len() {
            tracing::warn!(
                ?image,
                db_digest_len = diff_ids.len(),
                disk_digest_len = layer_digests.len(),
                "Layer count mismatch",
            );
            return Ok(false);
        }

        // 步骤 7: 所有检查通过
        tracing::info!(?image, "All layers for image exist and are valid");
        Ok(true)
    }
}
