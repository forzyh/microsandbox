// ============================================================================
// OCI 模块集成测试
// ============================================================================
//
// 本文件包含 OCI (Open Container Initiative) 模块的集成测试。
// OCI 是容器行业的标准规范，定义了容器镜像和运行时的标准格式。
//
// 这些测试主要用于验证：
// 1. 与 Docker Registry 的交互（拉取镜像、获取清单等）
// 2. 镜像数据在数据库中的存储
// 3. 镜像层（layers）的下载和验证
//
// 注意：大部分测试需要访问网络（Docker Registry），因此默认被标记为 ignore，
// 运行时需要使用 `cargo test -- --ignored` 来显式执行。

use std::str::FromStr;

use crate::{
    oci::{
        // Docker 注册表类型注解常量，用于标识 Docker 特定的镜像类型
        DOCKER_REFERENCE_TYPE_ANNOTATION,
        // 镜像引用，如 "alpine:latest" 或 "nginx:1.21"
        Reference,
        // 全局缓存操作 trait，用于管理镜像层缓存
        global_cache::GlobalCacheOps,
        // Mock 工具，用于创建测试用的注册表和数据库环境
        mocks::mock_registry_and_db,
    },
    // 工具函数模块，包含哈希计算等辅助功能
    utils,
};

// futures: 异步编程库，StreamExt 提供流（Stream）的扩展方法
use futures::StreamExt;
// oci_client: OCI 客户端库，用于与容器注册表交互
use oci_client::manifest::OciManifest;
// oci_spec: OCI 规范定义的类型，如摘要算法、操作系统类型等
use oci_spec::image::{Digest, DigestAlgorithm, Os};
// sqlx: 异步 SQL 数据库库，用于数据库操作
use sqlx::Row;
// tokio: 异步运行时，提供异步文件系统和测试宏
use tokio::{fs, io::AsyncWriteExt, test};

// ============================================================================
// 测试：从 Docker Registry 拉取镜像
// ============================================================================
//
// 测试目的：
// 验证 OCI 模块能够正确地从 Docker Hub 拉取镜像，包括：
// 1. 下载镜像的层（layers）到本地缓存
// 2. 在数据库中创建正确的镜像记录
// 3. 验证层文件的完整性（大小和哈希值）
//
// 测试场景：
// - 使用 alpine:latest 作为测试镜像（小型、常用的 Linux 镜像）
// - 验证数据库中的 images、manifests、configs、layers 表记录
// - 验证下载的层文件与数据库记录一致
//
// 为什么标记为 ignore：
// 此测试会访问 Docker Hub，需要网络连接，且可能受网络状况影响。
// 在 CI/CD 或离线环境中不应自动运行。
#[test]
#[ignore = "makes network requests to Docker registry to pull an image"]
async fn test_docker_pull_image() -> anyhow::Result<()> {
    // 创建 mock 环境：注册表客户端、数据库连接、临时目录
    // mock_registry_and_db 会设置一个内存数据库和临时文件系统
    let (registry, db, _dir) = mock_registry_and_db().await;
    // 解析镜像引用 "alpine:latest"
    // Reference 结构体封装了镜像名和标签的解析逻辑
    let reference = Reference::from_str("alpine:latest").unwrap();
    // 执行镜像拉取操作
    // 这会下载镜像的所有层并存储到全局缓存中
    let result = registry.pull_image(&reference).await;
    // 断言拉取操作成功，如果失败则打印错误信息
    assert!(result.is_ok(), "{:?}", result.err());

    // -------------------------------------------------------------------------
    // 验证数据库中的镜像记录 (images 表)
    // -------------------------------------------------------------------------
    // 查询 images 表，验证镜像记录已正确创建
    // as_db_key() 将引用转换为数据库键格式
    let image = sqlx::query("SELECT * FROM images WHERE reference = ?")
        .bind(reference.as_db_key())
        .fetch_one(&db)
        .await?;
    // 验证镜像大小大于 0（确保有实际内容）
    assert!(image.get::<i64, _>("size_bytes") > 0);

    // -------------------------------------------------------------------------
    // 验证清单记录 (manifests 表)
    // -------------------------------------------------------------------------
    // manifest 是 OCI 镜像的核心元数据文件，描述了镜像的组成
    // schema_version = 2 表示使用 OCI Manifest Schema Version 2
    let manifest = sqlx::query("SELECT * FROM manifests WHERE image_id = ?")
        .bind(image.get::<i64, _>("id"))
        .fetch_one(&db)
        .await?;
    assert_eq!(manifest.get::<i64, _>("schema_version"), 2);

    // -------------------------------------------------------------------------
    // 验证配置记录 (configs 表)
    // -------------------------------------------------------------------------
    // config 包含镜像的运行配置，如环境变量、命令、操作系统等
    let manifest_id = manifest.get::<i64, _>("id");
    let config = sqlx::query("SELECT * FROM configs WHERE manifest_id = ?")
        .bind(manifest_id)
        .fetch_one(&db)
        .await?;
    // 验证操作系统为 Linux（alpine 是 Linux 发行版）
    // Os::Linux.to_string() 将枚举转换为字符串 "linux"
    // matches! 宏用于模式匹配断言
    assert!(matches!(config.get::<String, _>("os"), s if s == Os::Linux.to_string()));

    // -------------------------------------------------------------------------
    // 验证层记录 (layers 表 + manifest_layers 关联表)
    // -------------------------------------------------------------------------
    // OCI 镜像由多个层（layer）组成，每层是一个 tar 文件
    // manifest_layers 是多对多关联表，连接 manifests 和 layers
    let layers = sqlx::query(
        "SELECT * FROM manifest_layers
        INNER JOIN layers ON manifest_layers.layer_id = layers.id
        WHERE manifest_id = ?",
    )
    .bind(manifest_id)
    .fetch_all(&db)
    .await?;
    // 确保至少有一个层（镜像必须有内容）
    assert!(!layers.is_empty());

    // -------------------------------------------------------------------------
    // 逐层验证：文件存在性、大小、哈希值
    // -------------------------------------------------------------------------
    // 遍历每个层，验证下载的层文件与数据库记录一致
    for layer in layers {
        // 获取层的唯一标识符（digest），格式如 "sha256:abc123..."
        let digest = layer.get::<String, _>("digest");
        // 获取层的压缩后大小（字节）
        let size = layer.get::<i64, _>("size_bytes");
        // 构建层文件的路径：tar_download_dir/{digest}.tar
        // 全局缓存将层文件存储在专门的目录中
        let layer_path = registry
            .global_cache()
            .tar_download_dir()
            .join(&digest)
            .with_extension("tar");

        // ---------------------------------------------------------------------
        // 验证层文件存在且大小正确
        // ---------------------------------------------------------------------
        // 首先检查文件是否存在，这是最基本的验证
        assert!(layer_path.exists(), "Layer file {} not found", digest);
        // 然后验证文件大小与数据库记录一致
        // fs::metadata 获取文件元数据，.len() 返回文件大小
        assert_eq!(
            fs::metadata(&layer_path).await?.len() as i64,
            size,
            "Layer {} size mismatch",
            digest
        );

        // ---------------------------------------------------------------------
        // 验证层文件的哈希值
        // ---------------------------------------------------------------------
        // OCI 使用 SHA-256 哈希来唯一标识和验证层内容
        // digest 格式："{algorithm}:{hex_hash}"，如 "sha256:abc123..."
        let parts: Vec<&str> = digest.split(':').collect();
        // 解析哈希算法（通常是 SHA-256）
        let algorithm = &DigestAlgorithm::try_from(parts[0])?;
        // 期望的哈希值（去掉算法前缀）
        let expected_hash = parts[1];
        // 计算文件的实际哈希值
        // utils::get_file_hash 异步读取文件并计算哈希
        // hex::encode 将字节数组转换为十六进制字符串
        let actual_hash = hex::encode(utils::get_file_hash(&layer_path, algorithm).await?);
        // 验证计算出的哈希值与 digest 中的哈希值匹配
        // 这确保文件内容完整，没有被篡改或损坏
        assert_eq!(actual_hash, expected_hash, "Layer {} hash mismatch", digest);
    }

    Ok(())
}

// ============================================================================
// 测试：从 Docker Registry 获取镜像索引（Image Index）
// ============================================================================
//
// 测试目的：
// 验证 OCI 模块能够正确地获取镜像索引（Image Index）。
//
// 背景知识：
// - Image Index（镜像索引）是 OCI 规范中的一种 manifest 类型
// - 它用于支持多架构镜像（multi-arch images）
// - 一个镜像索引可以包含多个 manifest，每个对应不同的平台（如 amd64/linux, arm64/linux）
// - 这使得同一个镜像引用（如 alpine:latest）可以在不同架构的机器上使用
//
// 测试场景：
// - 获取 alpine:latest 的镜像索引
// - 验证返回的是 ImageIndex 类型（而不是单一的 Image Manifest）
// - 验证每个 manifest 条目都有必要的字段（size, digest, media_type）
// - 验证非证明（non-attestation）manifest 有平台信息
//
// 为什么标记为 ignore：
// 此测试需要访问 Docker Hub 获取镜像索引信息。
#[test]
#[ignore = "makes network requests to Docker registry to fetch image index"]
async fn test_docker_fetch_index() -> anyhow::Result<()> {
    // 创建 mock 环境（只需要 registry，不需要数据库）
    let (registry, _, _) = mock_registry_and_db().await;
    // 解析镜像引用
    let reference = Reference::from_str("alpine:latest").unwrap();

    // 获取镜像索引
    // fetch_index 返回一个 ImageIndex 或 Image Manifest
    // alpine 是多架构镜像，所以应该返回 ImageIndex
    let result = registry.fetch_index(&reference).await;

    // 模式匹配：确保返回的是 ImageIndex 类型
    // 如果是其他类型，则 panic（alpine 应该是多架构镜像）
    let OciManifest::ImageIndex(index) = result.unwrap() else {
        panic!("alpine image should be image index");
    };

    // -------------------------------------------------------------------------
    // 验证每个 manifest 条目的字段完整性
    // -------------------------------------------------------------------------
    // ImageIndex.manifests 是一个 manifest 描述符列表
    // 每个描述符包含指向实际 manifest 的引用和元数据
    for manifest in index.manifests {
        // 验证 manifest 大小大于 0（必须有内容）
        assert!(manifest.size > 0);
        // 验证 digest 以 "sha256:" 开头
        // OCI 规范要求使用 SHA-256 作为摘要算法
        assert!(manifest.digest.to_string().starts_with("sha256:"));
        // 验证 media_type 包含 "manifest" 字样
        // 这确保我们指向的是 manifest 而不是其他类型的 blob
        assert!(manifest.media_type.to_string().contains("manifest"));

        // ---------------------------------------------------------------------
        // 验证非证明 manifest 的平台信息
        // ---------------------------------------------------------------------
        // Docker 使用特殊的注解来标识证明镜像（attestation manifests）
        // DOCKER_REFERENCE_TYPE_ANNOTATION 是 Docker 特定的注解键
        // 证明镜像是用来携带 SBOM、签名等元数据的特殊 manifest
        // 对于普通镜像（非证明），应该有平台信息（architecture, os 等）
        if !manifest
            .annotations
            .as_ref()
            .is_some_and(|a| a.contains_key(DOCKER_REFERENCE_TYPE_ANNOTATION))
        {
            // 如果不是证明镜像，则必须有平台信息
            // platform 包含架构（amd64/arm64）、操作系统（linux/windows）等信息
            let platform = manifest.platform.as_ref().expect("Platform info missing");
            // 验证操作系统为 Linux（alpine 是 Linux 发行版）
            assert_eq!(platform.os, oci_spec::image::Os::Linux);
        }
    }

    Ok(())
}

// ============================================================================
// 测试：从 Docker Registry 获取镜像清单和配置
// ============================================================================
//
// 测试目的：
// 验证 OCI 模块能够正确地获取镜像的 manifest 和 config。
//
// 背景知识：
// - Manifest（清单）：OCI 镜像的元数据文件，描述镜像的组成
//   - schema_version: 规范版本（目前为 2）
//   - config: 指向配置 blob 的引用
//   - layers: 镜像层列表，每层是一个 tar 文件
// - Config（配置）：描述镜像的运行配置
//   - os: 操作系统（linux/windows 等）
//   - architecture: CPU 架构（amd64/arm64 等）
//   - rootfs: 文件系统层信息
//   - config.cmd/env: 容器启动命令和环境变量
//
// 测试场景：
// - 获取 alpine:latest 的 manifest 和 config
// - 验证 manifest 的必要字段（schema_version, config, layers）
// - 验证 config 的必要字段（os, rootfs）
// - 验证可选字段（created, cmd, env）如果存在则格式正确
//
// 为什么标记为 ignore：
// 此测试需要访问 Docker Hub 获取镜像元数据。
#[test]
#[ignore = "makes network requests to Docker registry to fetch image manifest"]
async fn test_docker_fetch_manifest_and_config() -> anyhow::Result<()> {
    // 创建 mock 环境
    let (registry, _, _) = mock_registry_and_db().await;
    // 解析镜像引用
    let reference = Reference::from_str("alpine:latest").unwrap();

    // 获取 manifest 和 config
    // fetch_manifest_and_config 是一个便捷方法，同时返回两者
    let (manifest, config) = registry
        .fetch_manifest_and_config(&reference)
        .await
        .unwrap();

    // -------------------------------------------------------------------------
    // 验证 manifest 的必要字段
    // -------------------------------------------------------------------------
    // schema_version = 2 是 OCI Manifest 的当前版本
    assert_eq!(manifest.schema_version, 2);
    // config 必须有大小（配置 blob 不能为空）
    assert!(manifest.config.size > 0);
    // config digest 必须以 sha256: 开头
    assert!(manifest.config.digest.to_string().starts_with("sha256:"));
    // config media_type 必须包含 "config" 字样
    assert!(manifest.config.media_type.to_string().contains("config"));

    // -------------------------------------------------------------------------
    // 验证 manifest 的 layers
    // -------------------------------------------------------------------------
    // layers 是镜像的实际内容，每层是一个 tar 文件
    // alpine 至少有一个层（rootfs）
    assert!(!manifest.layers.is_empty());
    // 遍历每个 layer，验证其字段完整性
    for layer in manifest.layers {
        // 每层必须有大小（不能有空白层）
        assert!(layer.size > 0);
        // 每层的 digest 必须是有效的 SHA-256 哈希
        assert!(layer.digest.to_string().starts_with("sha256:"));
        // media_type 必须包含 "layer" 字样
        // 常见的 layer media_type 包括：
        // - application/vnd.oci.image.layer.v1.tar+gzip (压缩层)
        // - application/vnd.oci.image.layer.v1.tar (未压缩层)
        assert!(layer.media_type.to_string().contains("layer"));
    }

    // -------------------------------------------------------------------------
    // 验证 config 的必要字段
    // -------------------------------------------------------------------------
    // 验证操作系统为 Linux
    assert_eq!(config.os, oci_client::config::Os::Linux);
    // rootfs.type 必须是 "layers"（OCI 规范规定的唯一有效值）
    assert!(config.rootfs.r#type == "layers");
    // rootfs.diff_ids 是层的 uncompressed digest 列表
    // 必须与 manifest.layers 一一对应
    assert!(!config.rootfs.diff_ids.is_empty());

    // -------------------------------------------------------------------------
    // 验证 config 的可选字段（如果存在则验证格式）
    // -------------------------------------------------------------------------
    // created: 镜像创建时间戳（可选字段）
    // 不是所有镜像都有此字段（取决于构建工具）
    if let Some(created) = config.created {
        // 验证时间戳是有效的（大于 0）
        // timestamp_millis() 返回自 Unix 纪元以来的毫秒数
        assert!(created.timestamp_millis() > 0);
    }
    // config: 容器配置（可选字段）
    // 包含环境变量、命令、入口点等运行时配置
    if let Some(config_fields) = config.config {
        // env: 环境变量列表（可选）
        // 格式：["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin", ...]
        if let Some(env) = config_fields.env {
            // 如果有环境变量，则不能为空列表
            assert!(!env.is_empty());
        }
        // cmd: 容器默认命令（可选）
        // 格式：["/bin/sh", "-c", "echo hello"] 或 ["/bin/bash"]
        if let Some(cmd) = config_fields.cmd {
            // 如果有命令，则不能为空列表
            assert!(!cmd.is_empty());
        }
    }

    Ok(())
}

// ============================================================================
// 测试：从 Docker Registry 获取镜像 Blob（原始数据块）
// ============================================================================
//
// 测试目的：
// 验证 OCI 模块能够正确地通过流式下载获取镜像的 blob 数据。
//
// 背景知识：
// - Blob：OCI 中的基本数据存储单元
//   - 镜像层（layers）是 blob
//   - 配置（config）是 blob
//   - manifest 本身也是 blob
// - Digest：blob 的唯一标识符，格式为 "{algorithm}:{hash}"
//   - 例如：sha256:abc123...
// - 流式下载：对于大文件，不一次性加载到内存，而是分块下载
//
// 测试场景：
// - 从 manifest 中获取第一个 layer 的 digest
// - 使用 fetch_digest_blob 流式下载该 layer
// - 将下载的数据写入临时文件
// - 验证下载的总大小与 manifest 记录一致
// - 验证 digest 与 manifest 记录一致
//
// 为什么标记为 ignore：
// 此测试需要访问 Docker Hub 下载实际的 layer 数据。
#[test]
#[ignore = "makes network requests to Docker registry to fetch image blob"]
async fn test_docker_fetch_image_blob() -> anyhow::Result<()> {
    // 创建 mock 环境
    let (registry, _, _) = mock_registry_and_db().await;
    // 解析镜像引用
    let reference = Reference::from_str("alpine:latest").unwrap();

    // -------------------------------------------------------------------------
    // 从 manifest 中获取 layer 的 digest
    // -------------------------------------------------------------------------
    // 首先获取 manifest 和 config
    let (manifest, _) = registry.fetch_manifest_and_config(&reference).await?;
    // 获取第一个 layer（任意 layer 都可以用于测试）
    let layer = manifest.layers.first().unwrap();
    // 将 layer 的 digest 字符串转换为 Digest 类型
    // Digest 是 oci_spec 定义的强类型封装
    let digest = Digest::try_from(layer.digest.clone()).unwrap();

    // 获取 blob 下载流
    // fetch_digest_blob 参数：
    // - reference: 镜像引用
    // - digest: blob 的唯一标识符
    // - 0: 起始偏移量（从文件开头开始）
    // - None: 结束偏移量（None 表示下载整个文件）
    let mut stream = registry
        .fetch_digest_blob(&reference, &digest, 0, None)
        .await?;

    // -------------------------------------------------------------------------
    // 创建临时文件并流式写入下载的数据
    // -------------------------------------------------------------------------
    // 使用 tempfile 创建临时目录（测试结束后自动清理）
    let temp_download_dir = tempfile::tempdir()?;
    // 在临时目录中创建测试文件
    let temp_file = temp_download_dir.path().join("test_blob");
    // 创建异步文件用于写入
    let mut file = fs::File::create(&temp_file).await?;
    // 累计下载的总字节数
    let mut total_size = 0;

    // 流式读取并写入
    // stream.next() 返回下一个数据块（chunk）
    // 每个 chunk 是 Vec<u8> 类型
    while let Some(chunk) = stream.next().await {
        // 获取数据块，如果出错则返回错误
        let bytes = chunk?;
        // 累加字节数
        total_size += bytes.len();
        // 异步写入文件
        file.write_all(&bytes).await?;
    }

    // -------------------------------------------------------------------------
    // 验证下载结果
    // -------------------------------------------------------------------------
    // 验证下载的总大小大于 0（确保有实际数据）
    assert!(total_size > 0);
    // 验证下载的大小与 manifest 记录的 layer.size 一致
    // 这确保我们下载了完整的数据，没有丢失任何字节
    assert_eq!(total_size as i64, layer.size);

    // 验证 digest 一致
    // 这确保我们下载的是正确的 blob（与 manifest 描述的匹配）
    assert_eq!(digest.to_string(), layer.digest);

    Ok(())
}
