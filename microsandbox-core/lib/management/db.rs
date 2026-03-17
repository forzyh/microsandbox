//! Microsandbox 数据库管理模块
//!
//! 本模块提供了 Microsandbox 的数据库功能，管理沙箱和 OCI（Open Container Initiative）相关数据。
//! 它处理数据库初始化、迁移，以及容器镜像、层和沙箱配置的存储和检索。
//!
//! ## 主要功能
//!
//! - **数据库初始化** - 创建 SQLite 数据库并运行迁移
//! - **沙箱管理** - 保存、更新、查询沙箱记录
//! - **OCI 数据管理** - 存储镜像、清单、配置和层信息
//! - **连接池管理** - 高效管理数据库连接
//!
//! ## 数据库结构
//!
//! ### 沙箱数据库 (sandbox.db)
//! - `sandboxes` - 沙箱运行时信息表
//! - `sandbox_metrics` - 沙箱指标表
//!
//! ### OCI 数据库 (oci.db)
//! - `images` - 镜像信息表
//! - `manifests` - 镜像清单表
//! - `configs` - 镜像配置表
//! - `layers` - 镜像层表
//! - `manifest_layers` - 清单 - 层关联表

use std::path::Path;

use chrono::{DateTime, NaiveDateTime, Utc};
use oci_client::{
    config::ConfigFile,
    manifest::{OciDescriptor, OciImageManifest},
};
use oci_spec::image::MediaType;
use sqlx::{Pool, Row, Sqlite, migrate::Migrator, sqlite::SqlitePoolOptions};
use tokio::fs;

use crate::{
    MicrosandboxResult,
    models::{Config, Image, Layer, Manifest, Sandbox},
    runtime::SANDBOX_STATUS_RUNNING,
};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// 沙箱数据库迁移器
/// 包含沙箱数据库表结构的迁移脚本
pub static SANDBOX_DB_MIGRATOR: Migrator = sqlx::migrate!("lib/migrations/sandbox");

/// OCI 数据库迁移器
/// 包含 OCI 相关表的迁移脚本
pub static OCI_DB_MIGRATOR: Migrator = sqlx::migrate!("lib/migrations/oci");

//--------------------------------------------------------------------------------------------------
// 函数实现
//--------------------------------------------------------------------------------------------------

/// 初始化新的 SQLite 数据库
///
/// 如果指定路径的数据库不存在，则创建一个新的数据库并运行迁移。
/// 此函数确保父目录存在，创建数据库文件，然后应用所有迁移。
///
/// ## 参数
/// * `db_path` - SQLite 数据库文件的创建路径
/// * `migrator` - 包含数据库模式迁移的 SQLx 迁移器
///
/// ## 返回值
/// * `Ok(Pool<Sqlite>)` - 返回数据库连接池
/// * `Err(MicrosandboxError)` - 创建或迁移失败
///
/// ## 注意事项
/// 此函数会：
/// 1. 确保父目录存在
/// 2. 创建空的数据库文件
/// 3. 创建连接池
/// 4. 运行所有迁移
pub async fn initialize(
    db_path: impl AsRef<Path>,
    migrator: &Migrator,
) -> MicrosandboxResult<Pool<Sqlite>> {
    let db_path = db_path.as_ref();

    // 确保父目录存在
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // 如果数据库文件不存在，创建一个空文件
    if !db_path.exists() {
        fs::File::create(&db_path).await?;
    }

    // 创建数据库连接池
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
        .await?;

    // 运行迁移
    migrator.run(&pool).await?;

    Ok(pool)
}

/// 创建并返回 SQLite 数据库连接池
///
/// 此函数初始化一个新的 SQLite 连接池，用于管理数据库连接。
/// 连接池最大并发连接数配置为 5。
///
/// ## 参数
/// * `db_path` - SQLite 数据库文件路径
///
/// ## 返回值
/// * `Ok(Pool<Sqlite>)` - 返回连接池
/// * `Err(MicrosandboxError)` - 连接失败
pub async fn get_pool(db_path: impl AsRef<Path>) -> MicrosandboxResult<Pool<Sqlite>> {
    let db_path = db_path.as_ref();
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
        .await?;

    Ok(pool)
}

/// 获取现有数据库连接池或创建新的连接池
///
/// 此函数将数据库初始化和连接池创建合并为单个操作。
/// 如果数据库不存在，将创建数据库并运行迁移后返回连接池。
///
/// ## 参数
/// * `db_path` - SQLite 数据库文件路径
/// * `migrator` - 包含数据库模式迁移的 SQLx 迁移器
///
/// ## 返回值
/// * `Ok(Pool<Sqlite>)` - 返回连接池
/// * `Err(MicrosandboxError)` - 初始化或连接失败
pub async fn get_or_create_pool(
    db_path: impl AsRef<Path>,
    migrator: &Migrator,
) -> MicrosandboxResult<Pool<Sqlite>> {
    // 如果数据库不存在则初始化
    initialize(&db_path, migrator).await
}

//--------------------------------------------------------------------------------------------------
// 函数：沙箱操作
//--------------------------------------------------------------------------------------------------

/// 保存或更新数据库中的沙箱记录并返回其 ID
///
/// 如果存在同名和配置文件的沙箱，则更新它；否则创建新记录。
/// 此函数使用 upsert 模式（update or insert）确保沙箱记录的唯一性。
///
/// ## 参数
/// * `pool` - 数据库连接池
/// * `name` - 沙箱名称
/// * `config_file` - 配置文件路径
/// * `config_last_modified` - 配置文件最后修改时间
/// * `status` - 沙箱状态（如 "RUNNING"、"STOPPED"）
/// * `supervisor_pid` - Supervisor 进程 ID
/// * `microvm_pid` - MicroVM 进程 ID
/// * `rootfs_paths` - 根文件系统路径
///
/// ## 返回值
/// * `Ok(i64)` - 沙箱记录的 ID
/// * `Err(MicrosandboxError)` - 数据库操作失败
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_or_update_sandbox(
    pool: &Pool<Sqlite>,
    name: &str,
    config_file: &str,
    config_last_modified: &DateTime<Utc>,
    status: &str,
    supervisor_pid: u32,
    microvm_pid: u32,
    rootfs_paths: &str,
) -> MicrosandboxResult<i64> {
    let sandbox = Sandbox {
        id: 0,
        name: name.to_string(),
        config_file: config_file.to_string(),
        config_last_modified: *config_last_modified,
        status: status.to_string(),
        supervisor_pid,
        microvm_pid,
        rootfs_paths: rootfs_paths.to_string(),
        created_at: Utc::now(),
        modified_at: Utc::now(),
    };

    // 首先尝试更新
    let update_result = sqlx::query(
        r#"
        UPDATE sandboxes
        SET config_last_modified = ?,
            status = ?,
            supervisor_pid = ?,
            microvm_pid = ?,
            rootfs_paths = ?,
            modified_at = CURRENT_TIMESTAMP
        WHERE name = ? AND config_file = ?
        RETURNING id
        "#,
    )
    .bind(sandbox.config_last_modified.to_rfc3339())
    .bind(&sandbox.status)
    .bind(sandbox.supervisor_pid)
    .bind(sandbox.microvm_pid)
    .bind(&sandbox.rootfs_paths)
    .bind(&sandbox.name)
    .bind(&sandbox.config_file)
    .fetch_optional(pool)
    .await?;

    if let Some(record) = update_result {
        tracing::debug!("updated existing sandbox record");
        Ok(record.get::<i64, _>("id"))
    } else {
        // 如果没有记录被更新，则插入新记录
        tracing::debug!("creating new sandbox record");
        let record = sqlx::query(
            r#"
            INSERT INTO sandboxes (
                name, config_file, config_last_modified,
                status, supervisor_pid, microvm_pid, rootfs_paths
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
        )
        .bind(sandbox.name)
        .bind(sandbox.config_file)
        .bind(sandbox.config_last_modified.to_rfc3339())
        .bind(sandbox.status)
        .bind(sandbox.supervisor_pid)
        .bind(sandbox.microvm_pid)
        .bind(sandbox.rootfs_paths)
        .fetch_one(pool)
        .await?;

        Ok(record.get::<i64, _>("id"))
    }
}

/// 获取指定名称和配置文件的沙箱记录
pub(crate) async fn get_sandbox(
    pool: &Pool<Sqlite>,
    name: &str,
    config_file: &str,
) -> MicrosandboxResult<Option<Sandbox>> {
    let record = sqlx::query(
        r#"
        SELECT id, name, config_file, config_last_modified, status,
               supervisor_pid, microvm_pid, rootfs_paths,
               created_at, modified_at
        FROM sandboxes
        WHERE name = ? AND config_file = ?
        "#,
    )
    .bind(name)
    .bind(config_file)
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|row| Sandbox {
        id: row.get("id"),
        name: row.get("name"),
        config_file: row.get("config_file"),
        config_last_modified: row
            .get::<String, _>("config_last_modified")
            .parse::<DateTime<Utc>>()
            .unwrap(),
        status: row.get("status"),
        supervisor_pid: row.get("supervisor_pid"),
        microvm_pid: row.get("microvm_pid"),
        rootfs_paths: row.get("rootfs_paths"),
        created_at: parse_sqlite_datetime(&row.get::<String, _>("created_at")),
        modified_at: parse_sqlite_datetime(&row.get::<String, _>("modified_at")),
    }))
}

/// 更新沙箱的状态
///
/// 通过名称和配置文件标识沙箱，将其状态更新为指定值。
/// 此函数通常在沙箱启动或停止时调用。
pub(crate) async fn update_sandbox_status(
    pool: &Pool<Sqlite>,
    name: &str,
    config_file: &str,
    status: &str,
) -> MicrosandboxResult<()> {
    sqlx::query(
        r#"
        UPDATE sandboxes
        SET status = ?,
            modified_at = CURRENT_TIMESTAMP
        WHERE name = ? AND config_file = ?
        "#,
    )
    .bind(status)
    .bind(name)
    .bind(config_file)
    .execute(pool)
    .await?;

    Ok(())
}

/// 获取与特定配置文件关联的所有运行中沙箱
pub(crate) async fn get_running_config_sandboxes(
    pool: &Pool<Sqlite>,
    config_file: &str,
) -> MicrosandboxResult<Vec<Sandbox>> {
    let records = sqlx::query(
        r#"
        SELECT id, name, config_file, config_last_modified, status,
               supervisor_pid, microvm_pid, rootfs_paths,
               created_at, modified_at
        FROM sandboxes
        WHERE config_file = ? AND status = ?
        ORDER BY created_at DESC
        "#,
    )
    .bind(config_file)
    .bind(SANDBOX_STATUS_RUNNING)
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|row| Sandbox {
            id: row.get("id"),
            name: row.get("name"),
            config_file: row.get("config_file"),
            config_last_modified: row
                .get::<String, _>("config_last_modified")
                .parse::<DateTime<Utc>>()
                .unwrap(),
            status: row.get("status"),
            supervisor_pid: row.get("supervisor_pid"),
            microvm_pid: row.get("microvm_pid"),
            rootfs_paths: row.get("rootfs_paths"),
            created_at: parse_sqlite_datetime(&row.get::<String, _>("created_at")),
            modified_at: parse_sqlite_datetime(&row.get::<String, _>("modified_at")),
        })
        .collect())
}

/// 从数据库中删除沙箱记录
pub(crate) async fn delete_sandbox(
    pool: &Pool<Sqlite>,
    name: &str,
    config_file: &str,
) -> MicrosandboxResult<()> {
    sqlx::query(
        r#"
        DELETE FROM sandboxes
        WHERE name = ? AND config_file = ?
        "#,
    )
    .bind(name)
    .bind(config_file)
    .execute(pool)
    .await?;

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 函数：镜像操作
//--------------------------------------------------------------------------------------------------

/// 保存镜像到数据库并返回其 ID
pub(crate) async fn save_image(
    pool: &Pool<Sqlite>,
    reference: &str,
    size_bytes: i64,
) -> MicrosandboxResult<i64> {
    let image = Image {
        id: 0, // 将由数据库设置
        reference: reference.to_string(),
        size_bytes,
        last_used_at: Some(Utc::now()),
        created_at: Utc::now(),
        modified_at: Utc::now(),
    };

    let record = sqlx::query(
        r#"
        INSERT INTO images (reference, size_bytes, last_used_at)
        VALUES (?, ?, CURRENT_TIMESTAMP)
        RETURNING id
        "#,
    )
    .bind(&image.reference)
    .bind(image.size_bytes)
    .fetch_one(pool)
    .await?;

    Ok(record.get::<i64, _>("id"))
}

/// 保存镜像清单到数据库并返回其 ID
pub(crate) async fn save_manifest(
    pool: &Pool<Sqlite>,
    image_id: i64,
    manifest: &OciImageManifest,
) -> MicrosandboxResult<i64> {
    let manifest_model = Manifest {
        id: 0, // 将由数据库设置
        image_id,
        schema_version: manifest.schema_version as i64,
        media_type: manifest
            .media_type
            .as_ref()
            .map(|mt| mt.to_string())
            .unwrap_or_else(|| MediaType::ImageManifest.to_string()),
        annotations_json: manifest
            .annotations
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default()),
        created_at: Utc::now(),
        modified_at: Utc::now(),
    };

    let record = sqlx::query(
        r#"
        INSERT INTO manifests (
            image_id, schema_version,
            media_type, annotations_json
        )
        VALUES (?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(manifest_model.image_id)
    .bind(manifest_model.schema_version)
    .bind(&manifest_model.media_type)
    .bind(&manifest_model.annotations_json)
    .fetch_one(pool)
    .await?;

    Ok(record.get::<i64, _>("id"))
}

/// 保存镜像配置到数据库
pub(crate) async fn save_config(
    pool: &Pool<Sqlite>,
    manifest_id: i64,
    config: &ConfigFile,
) -> MicrosandboxResult<i64> {
    let config_model = Config {
        id: 0, // 将由数据库设置
        manifest_id,
        media_type: MediaType::ImageConfig.to_string(),
        created: config.created,
        architecture: config.architecture.to_string(),
        os: config.os.to_string(),
        // os_variant 不在正式规范中，由实现者自行决定
        // 由于我们不需要它，为了向后兼容性跳过它
        os_variant: None,
        config_env_json: config
            .config
            .as_ref()
            .map(|c| serde_json::to_string(&c.env).unwrap_or_default()),
        config_cmd_json: config
            .config
            .as_ref()
            .map(|c| serde_json::to_string(&c.cmd).unwrap_or_default()),
        config_working_dir: config
            .config
            .as_ref()
            .and_then(|c| c.working_dir.as_ref().map(String::from)),
        config_entrypoint_json: config
            .config
            .as_ref()
            .map(|c| serde_json::to_string(&c.entrypoint).unwrap_or_default()),
        config_volumes_json: config
            .config
            .as_ref()
            .map(|c| serde_json::to_string(&c.volumes).unwrap_or_default()),
        config_exposed_ports_json: config
            .config
            .as_ref()
            .map(|c| serde_json::to_string(&c.exposed_ports).unwrap_or_default()),
        config_user: config
            .config
            .as_ref()
            .and_then(|c| c.user.as_ref().map(String::from)),
        rootfs_type: config.rootfs.r#type.to_string(),
        rootfs_diff_ids_json: Some(
            serde_json::to_string(&config.rootfs.diff_ids).unwrap_or_default(),
        ),
        history_json: Some(serde_json::to_string(&config.history).unwrap_or_default()),
        created_at: Utc::now(),
        modified_at: Utc::now(),
    };

    let record = sqlx::query(
        r#"
        INSERT INTO configs (
            manifest_id, media_type, created, architecture,
            os, os_variant, config_env_json, config_cmd_json,
            config_working_dir, config_entrypoint_json,
            config_volumes_json, config_exposed_ports_json,
            config_user, rootfs_type, rootfs_diff_ids_json,
            history_json
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(config_model.manifest_id)
    .bind(&config_model.media_type)
    .bind(config_model.created.map(|dt| dt.to_rfc3339()))
    .bind(&config_model.architecture)
    .bind(&config_model.os)
    .bind(&config_model.os_variant)
    .bind(&config_model.config_env_json)
    .bind(&config_model.config_cmd_json)
    .bind(&config_model.config_working_dir)
    .bind(&config_model.config_entrypoint_json)
    .bind(&config_model.config_volumes_json)
    .bind(&config_model.config_exposed_ports_json)
    .bind(&config_model.config_user)
    .bind(&config_model.rootfs_type)
    .bind(&config_model.rootfs_diff_ids_json)
    .bind(&config_model.history_json)
    .fetch_one(pool)
    .await?;

    Ok(record.get::<i64, _>("id"))
}

/// 保存镜像层到数据库
pub(crate) async fn save_layer(
    pool: &Pool<Sqlite>,
    media_type: &str,
    digest: &str,
    size_bytes: i64,
    diff_id: &str,
) -> MicrosandboxResult<i64> {
    let layer_model = Layer {
        id: 0, // 将由数据库设置
        media_type: media_type.to_string(),
        digest: digest.to_string(),
        diff_id: diff_id.to_string(),
        size_bytes,
        created_at: Utc::now(),
        modified_at: Utc::now(),
    };

    let record = sqlx::query(
        r#"
        INSERT INTO layers (
            media_type, digest, size_bytes, diff_id
        )
        VALUES (?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(&layer_model.media_type)
    .bind(&layer_model.digest)
    .bind(layer_model.size_bytes)
    .bind(&layer_model.diff_id)
    .fetch_one(pool)
    .await?;

    Ok(record.get::<i64, _>("id"))
}

/// 保存或更新数据库中的层记录
///
/// 如果层存在，更新 size_bytes 和其他字段；
/// 如果不存在，创建新记录。
pub(crate) async fn save_or_update_layer(
    pool: &Pool<Sqlite>,
    media_type: &str,
    digest: &str,
    size_bytes: i64,
    diff_id: &str,
) -> MicrosandboxResult<i64> {
    let layer_model = Layer {
        id: 0, // 将由数据库设置
        media_type: media_type.to_string(),
        digest: digest.to_string(),
        diff_id: diff_id.to_string(),
        size_bytes,
        created_at: Utc::now(),
        modified_at: Utc::now(),
    };

    // 首先尝试更新
    let update_result = sqlx::query(
        r#"
        UPDATE layers
        SET media_type = ?,
            size_bytes = ?,
            diff_id = ?,
            modified_at = CURRENT_TIMESTAMP
        WHERE digest = ?
        RETURNING id
        "#,
    )
    .bind(&layer_model.media_type)
    .bind(layer_model.size_bytes)
    .bind(&layer_model.diff_id)
    .bind(&layer_model.digest)
    .fetch_optional(pool)
    .await?;

    if let Some(record) = update_result {
        Ok(record.get::<i64, _>("id"))
    } else {
        // 如果没有记录被更新，则插入新记录
        save_layer(pool, media_type, digest, size_bytes, diff_id).await
    }
}

/// 在 manifest_layers 关联表中将层与清单关联
pub(crate) async fn save_manifest_layer(
    pool: &Pool<Sqlite>,
    manifest_id: i64,
    layer_id: i64,
) -> MicrosandboxResult<i64> {
    let record = sqlx::query(
        r#"
        INSERT INTO manifest_layers (manifest_id, layer_id)
        VALUES (?, ?)
        ON CONFLICT (manifest_id, layer_id) DO NOTHING
        RETURNING id
        "#,
    )
    .bind(manifest_id)
    .bind(layer_id)
    .fetch_optional(pool)
    .await?;

    if let Some(record) = record {
        Ok(record.get::<i64, _>("id"))
    } else {
        // 如果没有插入记录（因为已存在），则获取现有 ID
        let record = sqlx::query(
            r#"
            SELECT id FROM manifest_layers
            WHERE manifest_id = ? AND layer_id = ?
            "#,
        )
        .bind(manifest_id)
        .bind(layer_id)
        .fetch_one(pool)
        .await?;

        Ok(record.get::<i64, _>("id"))
    }
}

/// 从数据库获取镜像的所有层
pub async fn get_image_layers(
    pool: &Pool<Sqlite>,
    reference: &str,
) -> MicrosandboxResult<Vec<Layer>> {
    let records = sqlx::query(
        r#"
        SELECT l.id, l.media_type, l.digest,
               l.diff_id, l.size_bytes, l.created_at, l.modified_at
        FROM layers l
        JOIN manifest_layers ml ON l.id = ml.layer_id
        JOIN manifests m ON ml.manifest_id = m.id
        JOIN images i ON m.image_id = i.id
        WHERE i.reference = ?
        ORDER BY l.id ASC
        "#,
    )
    .bind(reference)
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|row| Layer {
            id: row.get("id"),
            media_type: row.get("media_type"),
            digest: row.get("digest"),
            diff_id: row.get("diff_id"),
            size_bytes: row.get("size_bytes"),
            created_at: parse_sqlite_datetime(&row.get::<String, _>("created_at")),
            modified_at: parse_sqlite_datetime(&row.get::<String, _>("modified_at")),
        })
        .collect())
}

/// 检查镜像是否存在于数据库中
pub(crate) async fn image_exists(pool: &Pool<Sqlite>, reference: &str) -> MicrosandboxResult<bool> {
    let record = sqlx::query(
        r#"
        SELECT COUNT(*) as count
        FROM images
        WHERE reference = ?
        "#,
    )
    .bind(reference)
    .fetch_one(pool)
    .await?;

    Ok(record.get::<i64, _>("count") > 0)
}

/// 从数据库获取镜像的配置
///
/// 此函数检索指定镜像引用的配置详情，
/// 包括架构、操作系统、环境变量、命令、
/// 工作目录和其他容器配置元数据。
///
/// ## 参数
/// * `pool` - SQLite 连接池
/// * `reference` - OCI 镜像引用字符串（如 "ubuntu:latest"）
///
/// ## 返回值
/// 返回 `MicrosandboxResult`，包含镜像 `Config` 或错误
pub(crate) async fn get_image_config(
    pool: &Pool<Sqlite>,
    reference: &str,
) -> MicrosandboxResult<Option<Config>> {
    let record = sqlx::query(
        r#"
        SELECT c.id, c.manifest_id, c.media_type, c.created, c.architecture,
               c.os, c.os_variant, c.config_env_json, c.config_cmd_json,
               c.config_working_dir, c.config_entrypoint_json,
               c.config_volumes_json, c.config_exposed_ports_json,
               c.config_user, c.rootfs_type, c.rootfs_diff_ids_json,
               c.history_json, c.created_at, c.modified_at
        FROM configs c
        JOIN manifests m ON c.manifest_id = m.id
        JOIN images i ON m.image_id = i.id
        WHERE i.reference = ?
        LIMIT 1
        "#,
    )
    .bind(reference)
    .fetch_optional(pool)
    .await?;

    Ok(record.map(|row| Config {
        id: row.get("id"),
        manifest_id: row.get("manifest_id"),
        media_type: row.get("media_type"),
        created: row
            .get::<Option<String>, _>("created")
            .map(|dt| dt.parse::<DateTime<Utc>>().unwrap()),
        architecture: row.get("architecture"),
        os: row.get("os"),
        os_variant: row.get("os_variant"),
        config_env_json: null_to_none(row.get("config_env_json")),
        config_cmd_json: null_to_none(row.get("config_cmd_json")),
        config_working_dir: row.get("config_working_dir"),
        config_entrypoint_json: null_to_none(row.get("config_entrypoint_json")),
        config_volumes_json: null_to_none(row.get("config_volumes_json")),
        config_exposed_ports_json: null_to_none(row.get("config_exposed_ports_json")),
        config_user: row.get("config_user"),
        rootfs_type: row.get("rootfs_type"),
        rootfs_diff_ids_json: row.get("rootfs_diff_ids_json"),
        history_json: null_to_none(row.get("history_json")),
        created_at: parse_sqlite_datetime(&row.get::<String, _>("created_at")),
        modified_at: parse_sqlite_datetime(&row.get::<String, _>("modified_at")),
    }))
}

/// 保存或更新镜像到数据库
///
/// 如果镜像存在，更新 size_bytes 和 last_used_at；
/// 如果不存在，创建新记录。
pub(crate) async fn save_or_update_image(
    pool: &Pool<Sqlite>,
    reference: &str,
    size_bytes: i64,
) -> MicrosandboxResult<i64> {
    // 首先尝试更新
    let update_result = sqlx::query(
        r#"
        UPDATE images
        SET size_bytes = ?, last_used_at = CURRENT_TIMESTAMP, modified_at = CURRENT_TIMESTAMP
        WHERE reference = ?
        RETURNING id, reference, size_bytes, last_used_at, created_at, modified_at
        "#,
    )
    .bind(size_bytes)
    .bind(reference)
    .fetch_optional(pool)
    .await?;

    if let Some(record) = update_result {
        Ok(record.get::<i64, _>("id"))
    } else {
        // 如果没有记录被更新，则插入新记录
        save_image(pool, reference, size_bytes).await
    }
}

/// 通过 digest 值从数据库获取层
pub(crate) async fn get_layer_by_digest(
    pool: &Pool<Sqlite>,
    digest: &str,
) -> MicrosandboxResult<Option<Layer>> {
    let mut layers = get_layers_by_digest(pool, &[digest.to_string()]).await?;
    Ok(layers.pop())
}

/// 通过 digest 值从数据库获取多个层
///
/// 此函数检索与 digest 值列表匹配的层信息，
/// 不需要清单关系。这在尝试下载特定层之前
/// 检查它们是否存在于数据库中非常有用。
///
/// ## 参数
/// * `pool` - SQLite 连接池
/// * `digests` - 要搜索的层 digest 字符串列表
///
/// ## 返回值
/// 返回 `MicrosandboxResult`，包含与提供的 digest 匹配的 `Layer` 对象向量
pub(crate) async fn get_layers_by_digest(
    pool: &Pool<Sqlite>,
    digests: &[String],
) -> MicrosandboxResult<Vec<Layer>> {
    if digests.is_empty() {
        return Ok(Vec::new());
    }

    // 为 IN 子句创建占位符 (?,?,?)
    let placeholders = (0..digests.len())
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");

    let query = format!(
        r#"
        SELECT id, media_type, digest, diff_id, size_bytes, created_at, modified_at
        FROM layers
        WHERE digest IN ({})
        "#,
        placeholders
    );

    // 使用动态参数数量构建查询
    let mut query_builder = sqlx::query(&query);
    for digest in digests {
        query_builder = query_builder.bind(digest);
    }

    let records = query_builder.fetch_all(pool).await?;

    Ok(records
        .into_iter()
        .map(|row| Layer {
            id: row.get("id"),
            media_type: row.get("media_type"),
            digest: row.get("digest"),
            diff_id: row.get("diff_id"),
            size_bytes: row.get("size_bytes"),
            created_at: parse_sqlite_datetime(&row.get::<String, _>("created_at")),
            modified_at: parse_sqlite_datetime(&row.get::<String, _>("modified_at")),
        })
        .collect())
}

/// 从数据库获取镜像清单的所有层 digest
///
/// 此函数检索与特定镜像引用关联的所有层的 digest 字符串。
/// 这对于在不需要完整层详情的情况下检查层是否存在非常有用。
///
/// ## 参数
/// * `pool` - SQLite 连接池
/// * `reference` - OCI 镜像引用字符串（如 "ubuntu:latest"）
///
/// ## 返回值
/// 返回 `MicrosandboxResult`，包含层 digest 字符串向量
pub(crate) async fn get_image_layer_digests(
    pool: &Pool<Sqlite>,
    reference: &str,
) -> MicrosandboxResult<Vec<String>> {
    let records = sqlx::query(
        r#"
        SELECT l.digest
        FROM layers l
        JOIN manifest_layers ml ON l.id = ml.layer_id
        JOIN manifests m ON ml.manifest_id = m.id
        JOIN images i ON m.image_id = i.id
        WHERE i.reference = ?
        ORDER BY l.id ASC
        "#,
    )
    .bind(reference)
    .fetch_all(pool)
    .await?;

    Ok(records
        .into_iter()
        .map(|row| row.get::<String, _>("digest"))
        .collect())
}

/// 在数据库中将层与清单关联
///
/// 如果层不存在，将先创建层记录，然后再
/// 将其链接到清单。
///
/// ## 参数
/// * `pool` - SQLite 连接池
/// * `layer` - OCI 层描述符
/// * `diff_id` - 层的 Diff ID
/// * `manifest_id` - 要链接层的清单 ID
///
/// ## 返回值
/// 如果成功，返回清单层 ID
pub(crate) async fn create_or_update_manifest_layer(
    pool: &Pool<Sqlite>,
    layer: &OciDescriptor,
    diff_id: &str,
    manifest_id: i64,
) -> MicrosandboxResult<i64> {
    // 如果为 None，表示层还不存在于数据库中
    let db_layer_id = get_layer_by_digest(pool, &layer.digest.to_string())
        .await?
        .map(|l| l.id);

    let db_layer_id = match db_layer_id {
        Some(layer_id) => layer_id,
        None => {
            save_or_update_layer(pool, &layer.media_type, &layer.digest, layer.size, diff_id)
                .await?
        }
    };

    // 最后，将层链接到清单
    save_manifest_layer(pool, manifest_id, db_layer_id).await
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_init_sandbox_db() -> MicrosandboxResult<()> {
        // 创建临时目录
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test_sandbox.db");

        // 初始化数据库
        initialize(&db_path, &SANDBOX_DB_MIGRATOR).await?;

        // 测试数据库连接
        let pool = get_pool(&db_path).await?;

        // 通过查询验证表存在
        let tables = sqlx::query("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(&pool)
            .await?;

        let table_names: Vec<String> = tables
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect();

        assert!(
            table_names.contains(&"sandboxes".to_string()),
            "sandboxes table not found"
        );
        assert!(
            table_names.contains(&"sandbox_metrics".to_string()),
            "sandbox_metrics table not found"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_init_oci_db() -> MicrosandboxResult<()> {
        // 创建临时目录
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test_oci.db");

        // 初始化数据库
        initialize(&db_path, &OCI_DB_MIGRATOR).await?;

        // 测试数据库连接
        let pool = get_pool(&db_path).await?;

        // 通过查询验证表存在
        let tables = sqlx::query("SELECT name FROM sqlite_master WHERE type='table'")
            .fetch_all(&pool)
            .await?;

        let table_names: Vec<String> = tables
            .iter()
            .map(|row| row.get::<String, _>("name"))
            .collect();

        assert!(
            table_names.contains(&"images".to_string()),
            "images table not found"
        );
        assert!(
            table_names.contains(&"manifests".to_string()),
            "manifests table not found"
        );
        assert!(
            table_names.contains(&"configs".to_string()),
            "configs table not found"
        );
        assert!(
            table_names.contains(&"layers".to_string()),
            "layers table not found"
        );

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// 将 SQLite 日期时间字符串（格式为 "YYYY-MM-DD HH:MM:SS"）解析为 DateTime<Utc>
fn parse_sqlite_datetime(s: &str) -> DateTime<Utc> {
    let naive_dt = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .unwrap_or_else(|e| panic!("Failed to parse datetime string '{}': {:?}", s, e));
    DateTime::from_naive_utc_and_offset(naive_dt, Utc)
}

/// 有时数据库中的 json 列可能有字面量 "null" 值
/// 此函数将它们转换为 None
fn null_to_none(value: Option<String>) -> Option<String> {
    value.filter(|v| v != "null")
}
