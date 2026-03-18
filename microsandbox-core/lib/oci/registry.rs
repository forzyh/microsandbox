//! OCI 注册表交互模块
//!
//! 本模块提供了与 OCI（Open Container Initiative）注册表交互的功能，
//! 用于从注册表拉取容器镜像并存储到本地缓存。
//!
//! ## 主要功能
//!
//! - **镜像拉取** - 从 OCI 兼容的注册表（如 Docker Hub）拉取镜像
//! - **层下载** - 支持断点续传的层下载机制
//! - **平台解析** - 自动选择适合当前平台的镜像 manifest
//! - **缓存管理** - 与全局缓存协作，避免重复下载
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      Registry<C>                            │
//! │  泛型参数 C: GlobalCacheOps                                 │
//! ├─────────────────────────────────────────────────────────────┤
//! │  字段：                                                     │
//! │  • client: OciClient          - OCI 客户端                  │
//! │  • auth: RegistryAuth         - 认证信息                    │
//! │  • db: Pool<Sqlite>           - SQLite 数据库连接池          │
//! │  • global_cache: C            - 全局缓存                    │
//! ├─────────────────────────────────────────────────────────────┤
//! │  方法：                                                     │
//! │  • new()                      - 创建新的注册表客户端         │
//! │  • download_image_blob()      - 下载镜像层（支持断点续传）   │
//! │  • pull_image()               - 拉取整个镜像                │
//! │  • fetch_index()              - 获取镜像索引                │
//! │  • fetch_manifest_and_config()- 获取 manifest 和配置         │
//! │  • fetch_digest_blob()        - 通过 digest 获取 blob         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## OCI 标准
//!
//! 本模块使用 `oci_client` crate 实现，遵循 [OCI Distribution Spec] 标准。
//!
//! [OCI Distribution Spec]: https://distribution.github.io/distribution/spec/manifest-v2-2/#image-manifest-version-2-schema-2

use std::{str::FromStr, sync::Arc};

use bytes::Bytes;
use futures::{
    StreamExt,
    future::{self, try_join_all},
    stream::BoxStream,
};
use oci_client::{
    Client as OciClient,
    client::{BlobResponse, ClientConfig as OciClientConfig, Config as OciConfig, LayerDescriptor},
    config::ConfigFile as OciConfigFile,
    manifest::{ImageIndexEntry, OciImageManifest, OciManifest},
    secrets::RegistryAuth,
};
use oci_spec::image::{Digest, Platform};
use sqlx::{Pool, Sqlite};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};

use crate::{
    MicrosandboxError, MicrosandboxResult,
    management::db,
    oci::{Reference, global_cache::GlobalCacheOps, image::Image, layer::LayerOps},
    utils,
};

#[cfg(feature = "cli")]
use indicatif::{ProgressBar, ProgressStyle};
#[cfg(feature = "cli")]
use microsandbox_utils::term::{self, MULTI_PROGRESS};

//--------------------------------------------------------------------------------------------------
// 常量
//--------------------------------------------------------------------------------------------------

#[cfg(feature = "cli")]
/// 获取镜像详情时的进度条提示信息
const FETCH_IMAGE_DETAILS_MSG: &str = "Fetch image details";

#[cfg(feature = "cli")]
/// 下载层时的进度条提示信息
const DOWNLOAD_LAYER_MSG: &str = "Download layers";

/// Docker 注释中的引用类型键名
///
/// 用于识别 attestation（证明）清单，这类清单需要跳过
pub(crate) const DOCKER_REFERENCE_TYPE_ANNOTATION: &str = "vnd.docker.reference.type";

/// 注册表客户端，用于从 OCI 注册表拉取镜像并存储到本地缓存
///
/// `Registry` 是对 OCI 注册表交互逻辑的抽象，提供了：
/// - 镜像拉取功能
/// - 层下载（支持断点续传）
/// - 平台解析（多架构镜像支持）
/// - 与全局缓存的集成
///
/// ## 泛型参数
///
/// - `C: GlobalCacheOps` - 全局缓存操作 trait
///   - 负责管理层的下载和提取
///   - 提供层的持久化和检索功能
///
/// ## 字段说明
///
/// - `client`: OCI 客户端，负责与注册表的实际通信
/// - `auth`: 认证信息（目前仅支持匿名访问）
/// - `db`: SQLite 数据库连接池，用于存储镜像元数据
/// - `global_cache`: 全局缓存，负责层的存储和检索
///
/// ## 使用示例
///
/// ```rust,ignore
/// let registry = Registry::new(db, platform, global_cache).await?;
/// registry.pull_image(&image_ref).await?;
/// ```
pub struct Registry<C: GlobalCacheOps> {
    /// OCI 客户端实例
    client: OciClient,

    /// 注册表认证信息
    /// TODO (#333): 支持多种认证方式（如用户名密码、token 等）
    auth: RegistryAuth,

    /// SQLite 数据库连接池，用于存储镜像配置和清单
    db: Pool<Sqlite>,

    /// 全局微沙箱缓存的抽象
    global_cache: C,
}

impl<O> Registry<O>
where
    O: GlobalCacheOps + Send + Sync,
{
    /// ### 创建新的注册表客户端
    ///
    /// 这是 `Registry` 的主要构造函数，用于初始化 OCI 注册表客户端。
    ///
    /// ## 参数
    ///
    /// * `db` - SQLite 数据库连接池，用于存储镜像配置和清单
    /// * `platform` - 目标平台（如 Linux/amd64），用于选择适合的镜像 manifest
    /// * `global_cache` - 全局层缓存，负责管理层的下载和提取
    ///
    /// ## 返回值
    ///
    /// 返回初始化完成的 `Registry` 实例
    ///
    /// ## 实现原理
    ///
    /// 1. **配置 OCI 客户端**：
    ///    - 设置 `platform_resolver` 闭包，用于在多架构镜像中选择适合目标平台的 manifest
    ///    - 闭包捕获 `platform` 变量，在拉取镜像时自动调用 `resolve_digest_for_platform()`
    ///
    /// 2. **初始化字段**：
    ///    - `client`: 使用配置的 OCI 客户端
    ///    - `auth`: 使用匿名认证（`RegistryAuth::Anonymous`）
    ///    - `db`: 保存数据库连接池
    ///    - `global_cache`: 保存全局缓存
    ///
    /// ## 多架构支持
    ///
    /// OCI 镜像可能包含多个平台的 manifest（如 amd64、arm64）。
    /// 通过设置 `platform_resolver`，客户端会自动选择适合目标平台的 manifest。
    ///
    /// ## 示例
    ///
    /// ```rust,ignore
    /// let registry = Registry::new(db, platform, global_cache).await?;
    /// ```
    pub async fn new(
        db: Pool<Sqlite>,
        platform: Platform,
        global_cache: O,
    ) -> MicrosandboxResult<Self> {
        let config = OciClientConfig {
            // 设置平台解析器闭包，用于在多架构镜像中选择适合目标平台的 manifest
            platform_resolver: Some(Box::new(move |manifests| {
                Self::resolve_digest_for_platform(platform.clone(), manifests)
            })),
            // 其他配置使用默认值
            ..Default::default()
        };

        Ok(Self {
            client: OciClient::new(config),
            auth: RegistryAuth::Anonymous,  // 目前仅支持匿名访问
            db,
            global_cache,
        })
    }

    /// ### 获取全局层缓存
    ///
    /// 返回对全局缓存的不可变引用，用于访问层的下载和提取状态。
    ///
    /// ## 返回值
    ///
    /// 返回 `&O`，其中 `O: GlobalCacheOps`
    ///
    /// ## 使用场景
    ///
    /// - 检查层是否已经下载
    /// - 获取已下载的层进行提取
    /// - 查询镜像的提取状态
    pub fn global_cache(&self) -> &O {
        &self.global_cache
    }

    /// ### 下载镜像层（支持断点续传）
    ///
    /// 从 OCI 注册表下载单个镜像层，并支持断点续传功能。
    /// 如果文件已部分下载，会从断开的位置继续下载。
    ///
    /// ## 参数
    ///
    /// * `reference` - 镜像引用（包含注册表、仓库名、标签）
    /// * `digest` - 层的 digest（SHA256 哈希值），用于验证下载完整性
    /// * `expected_size` - 层的预期大小（字节数）
    ///
    /// ## 返回值
    ///
    /// 返回下载完成的层的抽象 `Arc<dyn LayerOps>`
    ///
    /// ## 下载流程
    ///
    /// ```text
    /// download_image_blob()
    ///   │
    ///   ├─> 1. 创建进度条（CLI 模式）
    ///   │
    ///   ├─> 2. 从全局缓存获取层的抽象
    ///   │
    ///   ├─> 3. 检查层的下载状态
    ///   │     │
    ///   │     ├─ 完全下载 ──> 跳过下载，直接返回
    ///   │     │
    ///   │     ├─ 未开始下载 ──> 创建新文件，从头开始
    ///   │     │
    ///   │     └─ 部分下载 ──> 以追加模式打开，从断开位置继续
    ///   │
    ///   ├─> 4. 创建文件父目录（如果不存在）
    ///   │
    ///   ├─> 5. 获取 blob 流（fetch_digest_blob）
    ///   │
    ///   ├─> 6. 逐块写入文件
    ///   │     │
    ///   │     └─ 更新进度条（CLI 模式）
    ///   │
    ///   ├─> 7. 验证文件哈希
    ///   │     │
    ///   │     ├─ 哈希匹配 ──> 成功
    ///   │     │
    ///   │     └─ 哈希不匹配 ──> 删除文件，返回错误
    ///   │
    ///   └─> 8. 从全局缓存返回层对象
    /// ```
    ///
    /// ## 断点续传原理
    ///
    /// 1. **检查已下载大小**：通过 `layer.get_tar_size()` 获取当前文件大小
    /// 2. **设置请求范围**：调用 `fetch_digest_blob()` 时传入 `existing_size` 作为 offset
    /// 3. **追加写入**：使用 `OpenOptions::append(true)` 打开文件
    /// 4. **OCI 协议支持**：OCI 注册表支持 HTTP Range 请求，可以请求 blob 的特定范围
    ///
    /// ## 哈希验证
    ///
    /// 下载完成后，使用 digest 中的算法（通常是 SHA256）计算文件哈希：
    /// - 如果哈希匹配，说明下载完整且正确
    /// - 如果哈希不匹配，说明下载损坏，会删除文件并返回错误
    ///
    /// ## 可能的错误
    ///
    /// - 网络连接失败
    /// - 注册表返回错误
    /// - 磁盘空间不足
    /// - 哈希验证失败（数据损坏）
    ///
    /// ## CLI 进度条
    ///
    /// 在 CLI 模式下，会显示：
    /// - Digest 前 8 位作为前缀
    /// - 进度条显示下载进度
    /// - 已下载字节数 / 总字节数
    pub async fn download_image_blob(
        &self,
        reference: &Reference,
        digest: &Digest,
        expected_size: u64,
    ) -> MicrosandboxResult<Arc<dyn LayerOps>> {
        // === 步骤 1: 创建进度条（仅 CLI 模式）===
        #[cfg(feature = "cli")]
        let progress_bar = {
            // 创建新的进度条，总大小为 expected_size
            let pb = MULTI_PROGRESS.add(ProgressBar::new(expected_size));
            // 设置进度条样式：前缀 | 进度条 | 已下载字节 / 总字节数
            let style = ProgressStyle::with_template(
                "{prefix:.bold.dim} {bar:40.green/green.dim} {bytes:.bold} / {total_bytes:.dim}",
            )
            .unwrap()
            .progress_chars("=+-");  // 进度条字符：已完成=，部分完成 +，未完成-

            pb.set_style(style);
            // 使用 digest 前 8 位作为前缀（便于识别不同的层）
            let digest_short = digest.digest().get(..8).unwrap_or("");
            pb.set_prefix(digest_short.to_string());
            pb.clone()
        };

        // === 步骤 2: 从全局缓存获取层的抽象 ===
        let layer = self.global_cache.build_layer(digest).await;
        #[cfg(feature = "cli")]
        {
            // 如果已经有部分下载，在进度条上反映出来
            let downloaded_so_far = layer.get_tar_size().unwrap_or(0);
            progress_bar.set_position(downloaded_so_far);
        }

        // === 步骤 3: 确保目标目录存在 ===
        let download_path = layer.tar_path();
        if let Some(parent) = download_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // === 步骤 4: 根据下载状态打开文件 ===
        // file: OpenOptions 构建器
        // existing_size: 已下载的大小（用于断点续传）
        let (mut file, mut existing_size) = (OpenOptions::new(), 0);
        match layer.get_tar_size() {
            // 情况 1: 层已完全下载，跳过
            Some(size) if size == expected_size => {
                tracing::info!(?digest, "Layer already exists. Skipping download");
                return Ok(layer);
            }

            // 情况 2: 文件不存在或大小为 0，创建新文件
            None | Some(0) => {
                tracing::info!(?digest, ?download_path, "Layer doesn't exist. Downloading");
                file.create(true).truncate(true).write(true)
            }

            // 情况 3: 文件已部分下载，追加模式
            Some(current_size) => {
                tracing::info!(
                    ?digest,
                    current_size,
                    expected_size,
                    ?download_path,
                    "Layer exists but is incomplete. Resuming download"
                );
                existing_size = current_size;
                file.append(true)
            }
        };

        // === 步骤 5: 打开文件并获取 blob 流 ===
        let mut file = file.open(&download_path).await?;
        // 获取 blob 流，从 existing_size 位置开始（断点续传）
        let mut stream = self
            .fetch_digest_blob(reference, digest, existing_size, None)
            .await?;

        // === 步骤 6: 逐块写入文件 ===
        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            file.write_all(&bytes).await?;
            #[cfg(feature = "cli")]
            progress_bar.inc(bytes.len() as u64);  // 更新进度条
        }

        #[cfg(feature = "cli")]
        progress_bar.finish_and_clear();  // 完成并隐藏进度条

        // === 步骤 7: 验证文件哈希 ===
        let algorithm = digest.algorithm();  // 获取哈希算法（通常是 sha256）
        let expected_hash = digest.digest();  // 预期的哈希值
        let actual_hash = hex::encode(utils::get_file_hash(&download_path, algorithm).await?);

        // 如果哈希不匹配，删除文件并返回错误
        if actual_hash != expected_hash {
            fs::remove_file(&download_path).await?;
            return Err(MicrosandboxError::ImageLayerDownloadFailed(format!(
                "({reference}:{digest}) file hash {actual_hash} does not match expected hash {expected_hash}",
            )));
        }

        // === 步骤 8: 从全局缓存返回层对象 ===
        let layer = self
            .global_cache
            .get_downloaded_layer(digest)
            .await
            .expect("layer should be present in cache after download");

        tracing::info!(?digest, "layer downloaded and cached successfully");
        Ok(layer)
    }

    /// ### 为平台解析 manifest 的 digest
    ///
    /// 从多架构镜像索引中选择适合指定平台的 manifest digest。
    ///
    /// OCI 镜像可能包含多个平台的 manifest（如 linux/amd64、linux/arm64）。
    /// 此方法根据优先级选择最适合的 manifest。
    ///
    /// ## 参数
    ///
    /// * `platform` - 目标平台（如 Linux/amd64）
    /// * `manifests` - 镜像索引中的所有 manifest 条目
    ///
    /// ## 返回值
    ///
    /// 返回匹配的 manifest digest，如果没有匹配则返回 `None`
    ///
    /// ## 匹配优先级
    ///
    /// ### 第一优先级：完全匹配（OS + 架构）
    ///
    /// 首先尝试匹配 OS 和架构都完全一致的 manifest：
    /// - `p.os == platform.os()` 且
    /// - `p.architecture == platform.architecture()`
    ///
    /// ### 第二优先级：仅匹配架构（降级兼容）
    ///
    /// 如果没有找到完全匹配，尝试仅匹配架构：
    /// - 这允许在 macOS 主机上运行 Linux 镜像（通过 VM）
    /// - `p.architecture == platform.architecture()`
    ///
    /// ## 跳过 Attestation 清单
    ///
    /// Attestation（证明）清单是一种特殊的清单，用于存储软件供应链的元数据。
    /// 这类清单包含 `vnd.docker.reference.type` 注解，需要跳过。
    ///
    /// ## 实现原理
    ///
    /// 使用 `find()` 方法在迭代器中查找：
    /// 1. 首先用 `find()` 查找完全匹配（OS + 架构）
    /// 2. 如果没找到，用 `or_else()` + `find()` 查找仅架构匹配
    /// 3. 用 `map()` 提取 digest
    ///
    /// ## 示例
    ///
    /// 假设有以下 manifests：
    /// ```text
    /// [
    ///   { platform: linux/amd64, digest: "sha256:abc..." },
    ///   { platform: linux/arm64, digest: "sha256:def..." },
    ///   { platform: darwin/amd64, digest: "sha256:ghi..." }  // attestation，跳过
    /// ]
    /// ```
    ///
    /// 调用 `resolve_digest_for_platform(linux/amd64)` 将返回 `"sha256:abc..."`
    fn resolve_digest_for_platform(
        platform: Platform,
        manifests: &[ImageIndexEntry],
    ) -> Option<String> {
        manifests
            .iter()
            // 第一优先级：匹配 OS 和架构
            .find(|m| {
                m.platform.as_ref().is_some_and(|p| {
                    p.os == *platform.os()    &&
                    p.architecture == *platform.architecture() &&
                    // 跳过 attestation 清单
                    !m.annotations.as_ref().is_some_and(|a| a.contains_key(DOCKER_REFERENCE_TYPE_ANNOTATION))
                })
            })
            // 第二优先级：仅匹配架构（如果没有 Linux 匹配，降级兼容）
            .or_else(|| {
                manifests.iter().find(|m| {
                    m.platform.as_ref().is_some_and(|p| {
                        p.architecture == *platform.architecture() &&
                        // 跳过 attestation 清单
                        !m.annotations.as_ref().is_some_and(|a| a.contains_key(DOCKER_REFERENCE_TYPE_ANNOTATION))
                    })
                })
            })
            .map(|m| m.digest.clone())
    }

    /// ### 拉取整个 OCI 镜像
    ///
    /// 从指定的注册表拉取完整的 OCI 镜像，包括：
    /// 1. 下载镜像 manifest
    /// 2. 获取镜像配置
    /// 3. 下载所有镜像层
    /// 4. 提取层到文件系统
    ///
    /// ## 参数
    ///
    /// * `reference` - 镜像引用（如 `docker.io/library/nginx:latest`）
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 镜像成功拉取并提取
    /// * `Err(MicrosandboxError)` - 拉取或提取失败时的错误
    ///
    /// ## 拉取流程
    ///
    /// ```text
    /// pull_image()
    ///   │
    ///   ├─> 1. 检查是否已提取
    ///   │     └─ 如果已提取，直接返回成功（幂等操作）
    ///   │
    ///   ├─> 2. 获取镜像索引（fetch_index）
    ///   │     └─ 计算总大小
    ///   │
    ///   ├─> 3. 保存镜像记录到数据库
    ///   │
    ///   ├─> 4. 获取 manifest 和配置
    ///   │     ├─ fetch_manifest_and_config()
    ///   │     ├─ save_manifest()
    ///   │     └─ save_config()
    ///   │
    ///   ├─> 5. 保存层元数据到数据库
    ///   │     └─ 将 manifest.layers 和 config.diff_ids 配对保存
    ///   │
    ///   ├─> 6. 并发下载所有层
    ///   │     ├─ 为每个层创建下载任务
    ///   │     ├─ 调用 download_image_blob()
    ///   │     └─ 等待所有任务完成（future::join_all）
    ///   │
    ///   └─> 7. 提取所有层（Image::new(layers).extract_all()）
    /// ```
    ///
    /// ## 关键概念
    ///
    /// ### Manifest vs Config
    ///
    /// - **Manifest**: 描述镜像的层（layers）及其 digest
    /// - **Config**: 包含镜像的运行配置（环境变量、命令等）和层的 diff_ids
    ///
    /// ### diff_ids vs layers
    ///
    /// - **layers**: 压缩的层（tar.gz），digest 用于网络传输验证
    /// - **diff_ids**: 解压后的层（tar），digest 用于本地存储验证
    /// - 两者一一对应，通过 zip() 配对
    ///
    /// ## 并发下载
    ///
    /// 所有层的下载是并发进行的：
    /// 1. 为每个层创建异步任务
    /// 2. 使用 `future::join_all()` 等待所有任务完成
    /// 3. 如果任何一个失败，整体失败
    ///
    /// ## 幂等性
    ///
    /// 此方法是幂等的：
    /// - 如果镜像已提取，直接返回成功
    /// - 如果层已下载，跳过下载（断点续传机制）
    pub(crate) async fn pull_image(&self, reference: &Reference) -> MicrosandboxResult<()> {
        // === 步骤 1: 检查是否已提取（幂等操作）===
        if self.global_cache().all_layers_extracted(reference).await? {
            tracing::info!(?reference, "Image was already extracted");
            return Ok(());
        }

        // === 步骤 2: 获取镜像索引并计算总大小 ===
        #[cfg(feature = "cli")]
        let fetch_details_sp =
            term::create_spinner(FETCH_IMAGE_DETAILS_MSG.to_string(), None, None);

        let index = self.fetch_index(reference).await?;
        let size = match index {
            // 单镜像 manifest: 直接使用 config.size
            OciManifest::Image(m) => m.config.size,
            // 多平台镜像索引: 累加所有 manifest 的大小
            OciManifest::ImageIndex(m) => m.manifests.iter().map(|m| m.size).sum(),
        };
        // 保存镜像记录到数据库
        let image_id = db::save_or_update_image(&self.db, &reference.as_db_key(), size).await?;

        // === 步骤 3: 获取并保存 manifest 和配置 ===
        let (manifest, config) = self.fetch_manifest_and_config(reference).await?;
        let manifest_id = db::save_manifest(&self.db, image_id, &manifest).await?;
        db::save_config(&self.db, manifest_id, &config).await?;

        // === 步骤 4: 保存层元数据到数据库 ===
        // 将 manifest.layers（压缩层）和 config.diff_ids（解压层）配对
        let diffs = config.rootfs.diff_ids.iter();
        let layer_to_zip = manifest.layers.iter().zip(diffs);
        // 并发保存所有层的元数据
        let db_ops = layer_to_zip
            .clone()
            .map(|(layer, diff_id)| {
                db::create_or_update_manifest_layer(&self.db, layer, diff_id, manifest_id)
            })
            .collect::<Vec<_>>();
        try_join_all(db_ops).await?;

        #[cfg(feature = "cli")]
        fetch_details_sp.finish();  // 完成获取镜像详情

        // === 步骤 5: 创建下载层进度条（CLI 模式）===
        #[cfg(feature = "cli")]
        let download_layers_sp = term::create_spinner(
            DOWNLOAD_LAYER_MSG.to_string(),
            None,
            Some(manifest.layers.len() as u64),  // 总层数
        );

        // === 步骤 6: 并发下载所有层 ===
        // 为每个层创建异步下载任务
        let layer_futures: Vec<_> = layer_to_zip
            .into_iter()
            .map(|(layer, _diff_id)| async {
                #[cfg(feature = "cli")]
                download_layers_sp.inc(1);  // 更新进度
                let digest = Digest::from_str(&layer.digest)?;
                let blob = self
                    .download_image_blob(reference, &digest, layer.size as u64)
                    .await?;

                Ok::<_, MicrosandboxError>(blob)
            })
            .collect();

        // 等待所有层下载完成
        let layers = future::join_all(layer_futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        #[cfg(feature = "cli")]
        download_layers_sp.finish();  // 完成下载进度条

        // === 步骤 7: 提取所有层 ===
        Image::new(layers).extract_all().await
    }

    /// ### 获取镜像索引（manifest index）
    ///
    /// 从注册表获取指定镜像引用的所有可用 manifest。
    ///
    /// OCI 镜像可能包含多个平台的 manifest（多架构镜像）。
    /// 此方法返回完整的 manifest 索引，供后续平台选择使用。
    ///
    /// ## 参数
    ///
    /// * `reference` - 镜像引用（如 `docker.io/library/nginx:latest`）
    ///
    /// ## 返回值
    ///
    /// * `OciManifest::ImageIndex` - 多平台镜像索引（包含多个 manifest 条目）
    /// * `OciManifest::Image` - 单镜像 manifest（没有多平台支持）
    ///
    /// ## OCI Manifest 类型
    ///
    /// ### ImageIndex（镜像索引）
    ///
    /// 包含多个平台的 manifest 条目，例如：
    /// ```json
    /// {
    ///   "manifests": [
    ///     {"platform": {"os": "linux", "architecture": "amd64"}, "digest": "sha256:abc..."},
    ///     {"platform": {"os": "linux", "architecture": "arm64"}, "digest": "sha256:def..."}
    ///   ]
    /// }
    /// ```
    ///
    /// ### Image（单镜像）
    ///
    /// 直接描述单个镜像的层和配置：
    /// ```json
    /// {
    ///   "layers": [...],
    ///   "config": {"digest": "sha256:xyz..."}
    /// }
    /// ```
    ///
    /// ## 实现原理
    ///
    /// 调用 `oci_client` 的 `pull_manifest()` 方法：
    /// - 如果镜像是 multi-arch，返回 ImageIndex
    /// - 如果镜像是 single-arch，返回 Image
    pub(crate) async fn fetch_index(
        &self,
        reference: &Reference,
    ) -> MicrosandboxResult<OciManifest> {
        // 从注册表拉取 manifest
        let (index, _) = self.client.pull_manifest(reference, &self.auth).await?;
        Ok(index)
    }

    /// ### 获取镜像 manifest 和配置
    ///
    /// 从注册表获取单个镜像的 manifest 和配置信息。
    ///
    /// ## 参数
    ///
    /// * `reference` - 镜像引用（如 `docker.io/library/nginx:latest`）
    ///
    /// ## 返回值
    ///
    /// 返回 `(OciImageManifest, OciConfigFile)` 元组：
    /// - `OciImageManifest`: 包含镜像层（layers）的 digest 和大小
    /// - `OciConfigFile`: 包含镜像的运行配置和层的 diff_ids
    ///
    /// ## OCI Config 转换
    ///
    /// `oci_client` 返回的是原始 JSON 字节，需要转换为 `OciConfigFile`：
    /// 1. 使用 `OciConfig::oci_v1()` 创建 OCI V1 配置对象
    /// 2. 使用 `try_from()` 转换为 `OciConfigFile`
    ///
    /// ## Config 文件内容
    ///
    /// Config 文件包含：
    /// - `rootfs.diff_ids`: 解压后的层的 digest 列表
    /// - `config.Env`: 环境变量
    /// - `config.Cmd`: 默认启动命令
    /// - `config.ExposedPorts`: 暴露的端口
    /// - `annotations`: 镜像注解
    pub(crate) async fn fetch_manifest_and_config(
        &self,
        reference: &Reference,
    ) -> MicrosandboxResult<(OciImageManifest, OciConfigFile)> {
        // 从注册表拉取 manifest 和配置（原始 JSON 字节）
        let (manifest, _, config) = self
            .client
            .pull_manifest_and_config(reference, &self.auth)
            .await?;

        // 将原始 JSON 字节转换为 OCI V1 配置对象
        let config = OciConfig::oci_v1(config.as_bytes().to_vec(), manifest.annotations.clone());
        // 转换为 ConfigFile 类型
        let config = OciConfigFile::try_from(config)?;
        Ok((manifest, config))
    }

    /// ### 通过 digest 获取镜像 blob（支持范围请求）
    ///
    /// 从注册表获取镜像层的 blob 数据，支持断点续传（HTTP Range 请求）。
    ///
    /// ## 参数
    ///
    /// * `reference` - 镜像引用（如 `docker.io/library/nginx:latest`）
    /// * `digest` - 层的 digest（SHA256 哈希值）
    /// * `offset` - 开始读取的位置（字节偏移量）
    ///   - `0`: 从头开始读取
    ///   - `existing_size`: 从已下载位置继续（断点续传）
    /// * `length` - 要读取的字节数
    ///   - `None`: 读取到末尾
    ///   - `Some(n)`: 读取 n 个字节
    ///
    /// ## 返回值
    ///
    /// 返回 `BoxStream<'static, MicrosandboxResult<Bytes>>`：
    /// - 异步流，逐个产生字节块（chunks）
    /// - 便于边下载边写入文件，无需全部加载到内存
    ///
    /// ## HTTP Range 请求
    ///
    /// OCI 注册表支持 HTTP Range 请求头：
    /// ```
    /// Range: bytes=offset-
    /// ```
    ///
    /// 这使得断点续传成为可能：
    /// - 如果下载中断，可以从 `offset` 位置继续
    /// - 无需重新下载已经完成的部分
    ///
    /// ## BlobResponse 类型
    ///
    /// `pull_blob_stream_partial()` 返回两种响应：
    /// - `BlobResponse::Full`: 完整响应（请求范围超出 blob 大小）
    /// - `BlobResponse::Partial`: 部分响应（成功执行 Range 请求）
    ///
    /// ## 实现原理
    ///
    /// 1. 创建 `LayerDescriptor` 描述要获取的层
    /// 2. 调用 `pull_blob_stream_partial()` 获取流
    /// 3. 根据响应类型提取流
    /// 4. 将错误类型转换为 `MicosandboxResult`
    pub(crate) async fn fetch_digest_blob(
        &self,
        reference: &Reference,
        digest: &Digest,
        offset: u64,
        length: Option<u64>,
    ) -> MicrosandboxResult<BoxStream<'static, MicrosandboxResult<Bytes>>> {
        // 记录日志：显示正在获取的 blob 信息和范围
        tracing::info!(
            "fetching blob: {digest} {offset}-{}",
            length.map(|l| l.to_string()).unwrap_or("end".to_string())
        );

        // 创建层描述符，描述要获取的层
        let layer = LayerDescriptor {
            digest: digest.as_ref(),  // 层的 digest
            urls: &None,  // 不使用外部 URL，从注册表获取
        };

        // 调用 oci_client 获取 blob 流（支持 Range 请求）
        let stream = self
            .client
            .pull_blob_stream_partial(reference, &layer, offset, length)
            .await?;

        // 根据响应类型提取流
        // Full: 完整响应（请求范围超出 blob 大小）
        // Partial: 部分响应（成功执行 Range 请求）
        let stream = match stream {
            BlobResponse::Full(s) => s,
            BlobResponse::Partial(s) => s,
        };

        // 将流的错误类型转换为 MicrosandboxResult
        Ok(stream.stream.map(|r| r.map_err(Into::into)).boxed())
    }
}
