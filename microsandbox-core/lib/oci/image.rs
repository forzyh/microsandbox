//! OCI 镜像管理模块
//!
//! 本模块提供了容器镜像管理的功能，用于从各种注册表拉取容器镜像，
//! 并将对应的镜像层提取到本地文件系统。
//!
//! ## OCI 镜像基础
//!
//! OCI（Open Container Initiative）镜像由多个层（layer）组成：
//! - **层（Layer）**：镜像的压缩文件系统快照，通常为 tar.gz 格式
//! - **清单（Manifest）**：描述镜像元数据的 JSON 文件，包含层的 digest
//! - **配置（Config）**：镜像的运行配置，如环境变量、启动命令等
//! - **Digest**：镜像层的唯一标识符，通常是 SHA256 哈希值
//!
//! ## 镜像拉取流程
//!
//! ```text
//! Image::pull()
//!   │
//!   ├─> 1. 创建临时下载目录
//!   │
//!   ├─> 2. 初始化数据库连接（用于存储镜像元数据）
//!   │
//!   ├─> 3. 创建全局缓存（管理下载和提取的层）
//!   │
//!   ├─> 4. 设置平台为 Linux（libkrun 仅支持 Linux）
//!   │
//!   ├─> 5. 创建 Registry 客户端
//!   │
//!   └─> 6. 拉取镜像并提取所有层
//! ```
//!
//! ## 使用示例
//!
//! ```no_run
//! use microsandbox_core::oci::Image;
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let layer_output_dir = Some(PathBuf::from("/custom/path"));
//!
//!     // 从 Docker 注册表拉取单个镜像
//!     Image::pull("docker.io/library/ubuntu:latest".parse().unwrap(), layer_output_dir.clone()).await?;
//!
//!     // 从默认注册表拉取镜像（当引用中未指定注册表时）
//!     Image::pull("nginx:latest".parse().unwrap(), layer_output_dir.clone()).await?;
//!
//!     // 可以设置 OCI_REGISTRY_DOMAIN 环境变量来指定默认注册表
//!     unsafe { std::env::set_var("OCI_REGISTRY_DOMAIN", "docker.io") };
//!     Image::pull("alpine:latest".parse().unwrap(), layer_output_dir.clone()).await?;
//!
//!     // 从 Docker 注册表拉取镜像并将层存储在自定义目录中
//!     Image::pull("docker.io/library/ubuntu:latest".parse().unwrap(), layer_output_dir).await?;
//!
//!     Ok(())
//! }
//! ```
use crate::{
    MicrosandboxResult,
    management::db::{self},
    oci::{GlobalCache, LayerDependencies, LayerOps, Reference, Registry},
};
use futures::future;
#[cfg(feature = "cli")]
use microsandbox_utils::term::{self};
use microsandbox_utils::{LAYERS_SUBDIR, OCI_DB_FILENAME, env};
use oci_spec::image::{Digest, Os, Platform};
use std::{path::PathBuf, sync::Arc};
use tempfile::tempdir;

/// 镜像层 bundle，用于管理相关的镜像层集合
///
/// 在 OCI 镜像中，多个层按照从基础层到顶层的顺序堆叠。
/// 每个层都是一个压缩的 tar 文件，包含文件系统的一部分。
///
/// ## 层的顺序
///
/// ```text
/// 顶层（latest layer）
///   │
///   ▼
/// 中间层
///   │
///   ▼
/// 基础层（base layer）
/// ```
///
/// ## 字段说明
///
/// - `layers`: 组成镜像的层向量，按正确顺序排列
///   - 第一个层是基础层（base layer）
///   - 最后一个层是最顶层（topmost layer）
///   - 每个层都实现了 `LayerOps` trait
#[derive(Clone)]
pub struct Image {
    /// 组成镜像的层，按从基础层到顶层的顺序排列
    layers: Vec<Arc<dyn LayerOps>>,
}

impl Image {
    /// 创建新的镜像 bundle
    ///
    /// 这是 `Image` 的主要构造函数，用于将多个相关的层组合成一个镜像。
    ///
    /// ## 参数
    ///
    /// * `layers` - 层向量，包含相关的层（例如某个层的所有父层）
    ///   - 层必须按从基础层到顶层的顺序排列
    ///   - 每个层都实现了 `LayerOps` trait
    ///
    /// ## 返回值
    ///
    /// * `Self` - 新的镜像 bundle
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// let layers = vec![layer1, layer2, layer3]; // 按顺序排列
    /// let image = Image::new(layers);
    /// ```
    pub(crate) fn new(layers: Vec<Arc<dyn LayerOps>>) -> Self {
        Self { layers }
    }

    /// 返回层的切片引用
    ///
    /// ## 返回值
    ///
    /// 返回镜像中所有层的不可变切片引用 `&[Arc<dyn LayerOps>]`
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// for layer in image.layers() {
    ///     println!("Layer digest: {}", layer.digest());
    /// }
    /// ```
    pub(crate) fn layers(&self) -> &[Arc<dyn LayerOps>] {
        self.layers.as_slice()
    }

    /// 返回给定层的父层（所有在它之前的层）
    ///
    /// 在 OCI 镜像中，层是堆叠的，每个层都基于它下面的所有层。
    /// 此方法用于获取某个层的所有父层，以便在提取时正确处理依赖关系。
    ///
    /// ## 参数
    ///
    /// * `digest` - 要获取父层的层的 digest
    ///
    /// ## 返回值
    ///
    /// 返回 `LayerDependencies`，包含：
    /// - 当前层的 digest
    /// - 所有父层（不包括当前层）
    ///
    /// ## 实现原理
    ///
    /// 使用 `split()` 方法将层向量在指定 digest 处分割：
    /// - `split()` 返回一个迭代器，每次迭代返回一个切片
    /// - 第一个切片包含匹配 digest 之前的所有层
    /// - 使用 `next()` 获取第一个切片
    /// - 使用 `to_vec()` 转换为向量
    ///
    /// ## 使用示例
    ///
    /// ```text
    /// 假设有层：[A, B, C, D]
    /// 调用 get_layer_parent(C) 将返回 [A, B]
    /// ```
    pub(crate) fn get_layer_parent(&self, digest: &Digest) -> LayerDependencies {
        // 使用 split() 在指定 digest 处分割层向量
        // split() 会返回一个迭代器，每次迭代返回一个切片
        let parents = self
            .layers
            .split(|layer| layer.digest() == digest)
            .next()
            .map(|layer| layer.to_vec())
            .unwrap_or_default();

        LayerDependencies::new(digest.clone(), Image::new(parents))
    }

    /// 提取镜像中的所有层
    ///
    /// 此方法会并发地提取所有层到本地文件系统。
    /// 每个层的提取是独立的，但会按照依赖顺序进行。
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 所有层成功提取
    /// * `Err(MicrosandboxError)` - 提取失败时的错误
    ///
    /// ## 提取流程
    ///
    /// ```text
    /// extract_all()
    ///   │
    ///   ├─> 1. 创建进度条（CLI 模式）
    ///   │
    ///   ├─> 2. 为每个层创建提取任务
    ///   │     │
    ///   │     ├─ 获取父层依赖
    ///   │     ├─ 提取层（如果有父层，会先复制父层的内容）
    ///   │     ├─ 如果失败，清理已提取的内容
    ///   │     └─ 更新进度条
    ///   │
    ///   ├─> 3. 等待所有提取任务完成（future::join_all）
    ///   │
    ///   └─> 4. 完成进度条
    /// ```
    ///
    /// ## 错误处理
    ///
    /// 如果某个层的提取失败：
    /// 1. 记录错误日志
    /// 2. 清理已提取的层内容
    /// 3. 返回错误
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// let image = Image::pull(reference, None).await?;
    /// image.extract_all().await?;
    /// ```
    pub(crate) async fn extract_all(&self) -> MicrosandboxResult<()> {
        // === 步骤 1: 创建进度条（仅在 CLI 模式下）===
        #[cfg(feature = "cli")]
        let extract_layers_sp = term::create_spinner(
            "Extracting layers".to_string(),
            None,
            Some(self.layers.len() as u64),
        );

        // === 步骤 2: 创建提取任务迭代器 ===
        // 为每个层创建一个异步提取任务
        let extraction_futures = self.layers.iter().map(|layer| {
            #[cfg(feature = "cli")]
            let pb = extract_layers_sp.clone();

            async move {
                // 获取当前层的父层依赖
                let parent_layers = self.get_layer_parent(layer.digest());
                // 提取当前层
                let result = layer.extract(parent_layers).await;
                // 如果提取失败，清理已提取的内容
                if let Err(err) = &result {
                    tracing::error!(?err, "Extracting failed. Cleaning up extracted artifacts");
                    layer.cleanup_extracted().await?;
                }

                // 更新进度条（CLI 模式）
                #[cfg(feature = "cli")]
                pb.inc(1);
                result
            }
        });

        // === 步骤 3: 等待所有提取任务完成 ===
        // future::join_all 会并发地等待所有任务完成
        for result in future::join_all(extraction_futures).await {
            // 如果任何一个任务失败，返回错误
            result?;
        }

        // === 步骤 4: 完成进度条（CLI 模式）===
        #[cfg(feature = "cli")]
        extract_layers_sp.finish();

        Ok(())
    }

    /// 从注册表拉取镜像
    ///
    /// 此方法是拉取 OCI 镜像的主要入口点。它会：
    /// 1. 创建临时下载目录
    /// 2. 初始化数据库连接（用于存储镜像元数据）
    /// 3. 创建全局缓存（管理层的下载和提取）
    /// 4. 设置平台为 Linux（因为 libkrun 仅支持 Linux）
    /// 5. 创建 Registry 客户端并拉取镜像
    ///
    /// ## 参数
    ///
    /// * `image` - 要拉取的镜像引用（例如 `docker.io/library/ubuntu:latest`）
    /// * `layer_extraction_dir` - 存储层文件的目录路径
    ///   - 如果为 `None`，使用默认的层输出目录（`MICROSANDBOX_HOME/layers`）
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 镜像成功拉取并提取
    /// * `Err(MicrosandboxError)` - 拉取或提取失败时的错误
    ///
    /// ## 可能的错误
    ///
    /// - 网络连接失败
    /// - 注册表认证失败
    /// - 磁盘空间不足
    /// - 层验证失败（digest 不匹配）
    ///
    /// ## 详细流程
    ///
    /// ```text
    /// Image::pull()
    ///   │
    ///   ├─> 1. 创建临时下载目录（使用 tempfile::tempdir）
    ///   │
    ///   ├─> 2. 获取 Microsandbox 主目录路径
    ///   │
    ///   ├─> 3. 初始化 SQLite 数据库（存储镜像元数据）
    ///   │
    ///   ├─> 4. 确定层输出目录
    ///   │     ├─ 如果指定了 layer_extraction_dir，使用该目录
    ///   │     └─ 否则使用 MICROSANDBOX_HOME/layers
    ///   │
    ///   ├─> 5. 创建全局缓存（管理层的下载和提取）
    ///   │
    ///   ├─> 6. 设置平台为 Linux
    ///   │     └─ libkrun 仅支持 Linux，所以必须明确设置
    ///   │
    ///   ├─> 7. 创建 Registry 客户端
    ///   │
    ///   └─> 8. 拉取镜像并提取所有层
    /// ```
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// // 拉取镜像到默认目录
    /// Image::pull("docker.io/library/ubuntu:latest".parse().unwrap(), None).await?;
    ///
    /// // 拉取镜像到自定义目录
    /// let custom_dir = Some(PathBuf::from("/custom/layers"));
    /// Image::pull("nginx:latest".parse().unwrap(), custom_dir).await?;
    /// ```
    ///
    /// ## 环境变量
    ///
    /// - `OCI_REGISTRY_DOMAIN` - 默认注册表域名（例如 `docker.io`）
    /// - `MICROSANDBOX_HOME` - Microsandbox 主目录（默认为 `~/.microsandbox`）
    pub async fn pull(
        image: Reference,
        layer_extraction_dir: Option<PathBuf>,
    ) -> MicrosandboxResult<()> {
        // === 步骤 1: 创建临时下载目录 ===
        // 使用 tempfile 创建临时目录，下载完成后会自动清理
        let temp_download_dir = tempdir()?;
        let temp_download_dir = temp_download_dir.path().to_path_buf();
        tracing::info!(?temp_download_dir, "temporary download directory");

        // === 步骤 2: 初始化数据库连接 ===
        // 数据库用于存储镜像的元数据（manifest、config、层信息等）
        let microsandbox_home_path = env::get_microsandbox_home_path();
        let db_path = microsandbox_home_path.join(OCI_DB_FILENAME);
        let db = db::get_or_create_pool(&db_path, &db::OCI_DB_MIGRATOR).await?;

        // === 步骤 3: 确定层输出目录 ===
        // 如果用户指定了目录，使用用户指定的目录；否则使用默认目录
        let layer_output_dir = layer_extraction_dir
            .unwrap_or_else(|| env::get_microsandbox_home_path().join(LAYERS_SUBDIR));

        // === 步骤 4: 创建全局缓存 ===
        // 全局缓存负责管理层的下载和提取，避免重复下载
        let layer_cache = GlobalCache::new(temp_download_dir, layer_output_dir, db.clone()).await?;

        // === 步骤 5: 设置平台为 Linux ===
        // libkrun（我们使用的 VM 底层）仅支持 Linux
        // 所以即使 host 是 macOS，也要明确设置平台为 Linux
        let mut platform = Platform::default();
        platform.set_os(Os::Linux);

        // === 步骤 6: 创建 Registry 客户端并拉取镜像 ===
        // Registry 客户端负责与 OCI 注册表交互
        Registry::new(db.clone(), platform, layer_cache)
            .await?
            .pull_image(&image)
            .await
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::oci::mocks::mock_registry_and_db;
    use microsandbox_utils::EXTRACTED_LAYER_SUFFIX;

    use rstest::rstest;
    use tokio::fs;

    /// 镜像提取集成测试
    ///
    /// 此测试验证从真实 Docker 注册表拉取镜像并提取层的功能。
    /// 由于需要网络请求，测试标记为 `#[ignore]`，需要使用 `--ignored` 运行。
    ///
    /// ## 测试用例
    ///
    /// 1. nginx 镜像（8 个层）
    ///    - 验证提取的层包含 nginx 配置文件
    ///    - 文件：etc/nginx/nginx.conf, etc/nginx/conf.d/default.conf, usr/sbin/nginx
    ///
    /// 2. hello-app 镜像（15 个层）
    ///    - 验证提取的层包含 hello-app 可执行文件
    ///
    /// ## 测试流程
    ///
    /// ```text
    /// 1. 创建模拟注册表和数据库
    /// 2. 拉取镜像
    /// 3. 验证镜像在数据库中存在
    /// 4. 遍历提取的层目录
    /// 5. 验证预期的文件存在
    /// 6. 验证提取的层数量正确
    /// ```
    #[rstest]
    #[case(
        "docker.io/library/nginx:stable-alpine3.23",
        vec!["etc/nginx/nginx.conf", "etc/nginx/conf.d/default.conf", "usr/sbin/nginx"],
        8  // nginx 镜像的层数量
    )]
    #[case(
        "us-docker.pkg.dev/google-samples/containers/gke/hello-app:1.0",
        vec!["hello-app"],
        15  // hello-app 镜像的层数量
    )]
    #[test_log::test(tokio::test)]
    #[ignore = "makes network requests to Docker registry to pull an image"]
    async fn test_image_extraction(
        #[case] image_ref: Reference,
        #[case] files_to_verify: Vec<&'static str>,
        #[case] expected_extracted_layers: usize,
    ) -> MicrosandboxResult<()> {
        // === 步骤 1: 创建模拟注册表和数据库 ===
        let (registry, db, layers_dir) = mock_registry_and_db().await;
        let download_dir = layers_dir.path().join("download");
        let extracted_dir = layers_dir.path().join("extracted");

        // === 步骤 2: 拉取镜像并验证在数据库中存在 ===
        registry.pull_image(&image_ref).await?;
        let image_exists = db::image_exists(&db, &image_ref.to_string()).await?;
        assert!(image_exists, "Image should exist in database");

        // === 步骤 3: 验证层目录存在并包含提取的层 ===
        let mut extracted_layers_count = 0;
        let mut extracted_dir_entries = fs::read_dir(&extracted_dir).await?;
        // 使用 HashSet 追踪未找到的文件
        let mut files_not_found = HashSet::<&str>::from_iter(files_to_verify);
        assert!(download_dir.exists(), "Layers directory should exist");

        // 遍历提取目录中的所有条目
        while let Some(entry) = extracted_dir_entries.next_entry().await? {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            // 检查是否是提取的层目录（以 .extracted 结尾）
            if entry_name.ends_with(EXTRACTED_LAYER_SUFFIX) && entry.path().is_dir() {
                extracted_layers_count += 1;
            }

            // 检查预期的文件是否存在
            for file in files_not_found.clone() {
                if entry.path().join(file).exists() {
                    files_not_found.remove(file);
                }
            }
        }
        // 验证所有预期文件都找到了
        assert!(
            files_not_found.is_empty(),
            "not all files could be found: {:?}",
            files_not_found
        );

        // === 步骤 4: 验证提取的层数量正确 ===
        assert_eq!(
            extracted_layers_count, expected_extracted_layers,
            "Extracted layer should be complete"
        );

        Ok(())
    }
}
