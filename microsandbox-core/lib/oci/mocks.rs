//! OCI 模块的测试工具
//!
//! 本模块提供了用于单元测试和集成测试的辅助函数。
//!
//! ## 为什么需要测试工具？
//!
//! OCI 模块依赖外部资源：
//! - SQLite 数据库（持久化存储）
//! - 文件系统（层下载和提取目录）
//! - 远程注册表（镜像拉取）
//!
//! 测试时需要：
//! 1. **隔离环境**: 使用临时目录，避免污染真实数据
//! 2. **可重复性**: 每次测试都从干净的状态开始
//! 3. **自动清理**: 测试完成后自动删除临时文件
//!
//! ## 模块组成
//!
//! - `mock_registry_and_db()`: 创建模拟的注册表客户端和数据库

use oci_spec::image::Platform;
use sqlx::{Pool, Sqlite};

use crate::{
    management::db::{self, OCI_DB_MIGRATOR},
    oci::{Registry, global_cache::GlobalCache},
};
use tempfile::TempDir;

/// 创建模拟的注册表客户端和数据库
///
/// 此函数用于测试，创建一个完全隔离的测试环境：
/// - 临时目录（测试完成后自动删除）
/// - 临时 SQLite 数据库
/// - 临时层下载和提取目录
/// - 初始化的注册表客户端
///
/// ## 返回值
///
/// 返回三元组 `(Registry, Pool<Sqlite>, TempDir)`：
/// - `Registry<GlobalCache>`: 注册表客户端
/// - `Pool<Sqlite>`: 数据库连接池
/// - `TempDir`: 临时目录（用于自动清理）
///
/// ## 临时目录结构
///
/// ```text
/// /tmp/random-name/
/// ├── download/          # 层 tar 文件下载目录
/// ├── extracted/         # 层提取目录
/// └── db/                # SQLite 数据库文件
/// ```
///
/// ## TempDir 的 Drop 实现
///
/// `tempfile::TempDir` 在被丢弃（drop）时会自动删除目录及其内容。
/// 所以测试结束后不需要手动清理。
///
/// ## 使用示例
///
/// ```rust,ignore
/// #[tokio::test]
/// async fn test_pull_image() {
///     let (registry, db, _temp_dir) = mock_registry_and_db().await;
///
///     // 使用 registry 进行测试
///     // 测试结束后，_temp_dir 被丢弃，临时目录自动删除
/// }
/// ```
///
/// ## 为什么返回 TempDir？
///
/// 返回 `TempDir` 是为了让调用者持有它的所有权。
/// 只要 `TempDir` 不被丢弃，临时目录就会一直存在。
/// 当测试函数返回时，`TempDir` 被丢弃，目录自动删除。
pub(crate) async fn mock_registry_and_db() -> (Registry<GlobalCache>, Pool<Sqlite>, TempDir) {
    // 创建临时目录（在 /tmp 或系统临时目录）
    let temp_dir = TempDir::new().unwrap();

    // 构建各子目录的路径
    let layers_tar_dir = temp_dir.path().join("download");
    let extracted_layers_dir = temp_dir.path().join("extracted");
    let db_path = temp_dir.path().join("db");

    // 创建或获取数据库连接池
    // OCI_DB_MIGRATOR 会自动运行数据库迁移
    let db = db::get_or_create_pool(&db_path, &OCI_DB_MIGRATOR)
        .await
        .unwrap();

    // 运行数据库迁移（创建表、索引等）
    OCI_DB_MIGRATOR.run(&db).await.unwrap();

    // 使用默认的平台（通常是 linux/amd64）
    let platform = Platform::default();

    // 初始化全局缓存
    let layer_ops = GlobalCache::new(layers_tar_dir, extracted_layers_dir, db.clone())
        .await
        .expect("global cache to be initialized");

    // 创建注册表客户端
    let registry = Registry::new(db.clone(), platform, layer_ops)
        .await
        .unwrap();

    // 返回注册表、数据库和临时目录
    (registry, db, temp_dir)
}
