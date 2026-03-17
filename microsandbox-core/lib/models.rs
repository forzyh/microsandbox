//! # 数据库模型模块
//!
//! 本模块定义了 microsandbox 系统中用于数据库持久化的数据结构。
//! 这些结构对应 SQLite 数据库中的表，用于存储沙箱状态、OCI 镜像元数据等信息。

use chrono::{DateTime, Utc};

//--------------------------------------------------------------------------------------------------
// 类型：沙箱 (Sandbox)
//--------------------------------------------------------------------------------------------------

/// 沙箱是由 Microsandbox 管理的活动虚拟机实例
///
/// 沙箱是 microsandbox 系统的核心概念，代表一个正在运行或已停止的
/// 隔离执行环境。每个沙箱都有唯一的标识符和相关的元数据。
///
/// ## 字段说明
/// * `id` - 数据库中的唯一标识符，自增主键
/// * `name` - 沙箱的名称，用户在配置文件中定义
/// * `config_file` - 定义此沙箱的 microsandbox 配置文件名
/// * `config_last_modified` - 配置文件最后修改时间，用于检测配置变更
/// * `status` - 沙箱当前状态（如 "RUNNING"、"STOPPED"）
/// * `supervisor_pid` - 监督此沙箱的 supervisor 进程 ID
/// * `microvm_pid` - 微虚拟机进程的 ID
/// * `rootfs_paths` - 沙箱根文件系统的路径列表
/// * `created_at` - 沙箱创建时间
/// * `modified_at` - 沙箱记录最后修改时间
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sandbox {
    /// 沙箱的唯一标识符
    pub id: i64,

    /// 沙箱的名称
    pub name: String,

    /// 定义沙箱的 Microsandbox 配置文件名
    pub config_file: String,

    /// Microsandbox 配置文件的最后修改日期和时间
    pub config_last_modified: DateTime<Utc>,

    /// 沙箱的状态
    ///
    /// 可能的值包括：
    /// - "RUNNING" - 沙箱正在运行
    /// - "STOPPED" - 沙箱已停止
    pub status: String,

    /// 沙箱监督进程的 PID (Process ID)
    ///
    /// supervisor 负责管理沙箱的生命周期
    pub supervisor_pid: u32,

    /// 沙箱微虚拟机进程的 PID
    pub microvm_pid: u32,

    /// 沙箱根文件系统的路径
    ///
    /// 对于 overlayfs 类型的 rootfs，这里存储多个层的路径
    pub rootfs_paths: String,

    /// 沙箱创建时间
    pub created_at: DateTime<Utc>,

    /// 沙箱最后修改时间
    pub modified_at: DateTime<Utc>,
}

//--------------------------------------------------------------------------------------------------
// 类型：OCI 相关模型
//--------------------------------------------------------------------------------------------------

/// 代表数据库中的 OCI 容器镜像
///
/// 这个结构存储了从容器注册表拉取的镜像的基本信息。
/// OCI (Open Container Initiative) 是容器镜像的标准规范。
///
/// ## 字段说明
/// * `id` - 数据库中的唯一标识符
/// * `reference` - 镜像引用字符串，如 "library/ubuntu:latest"
/// * `size_bytes` - 镜像总大小（字节）
/// * `last_used_at` - 镜像最后使用时间
/// * `created_at` - 记录创建时间
/// * `modified_at` - 记录最后修改时间
#[derive(Debug, Clone)]
pub struct Image {
    /// 镜像的唯一标识符
    pub id: i64,

    /// 镜像的引用字符串
    ///
    /// 格式示例：
    /// - "ubuntu:latest" - 简单引用
    /// - "docker.io/library/ubuntu:20.04" - 完整引用
    /// - "alpine@sha256:..." - 使用 digest 引用
    pub reference: String,

    /// 镜像大小（字节）
    pub size_bytes: i64,

    /// 镜像最后使用时间
    pub last_used_at: Option<DateTime<Utc>>,

    /// 记录创建时间
    pub created_at: DateTime<Utc>,

    /// 记录最后修改时间
    pub modified_at: DateTime<Utc>,
}

/// 代表数据库中的 OCI 镜像索引 (Index)
///
/// OCI 镜像索引用于多平台镜像，它包含多个 manifest 的引用，
/// 每个 manifest 对应一个特定的平台（如 linux/amd64）。
///
/// ## 字段说明
/// * `id` - 数据库中的唯一标识符
/// * `image_id` - 所属镜像的 ID
/// * `schema_version` - 索引模式版本
/// * `media_type` - 媒体类型，如 "application/vnd.oci.image.index.v1+json"
/// * `platform_os` - 目标操作系统，如 "linux"
/// * `platform_arch` - 目标架构，如 "amd64"、"arm64"
/// * `platform_variant` - 平台变体，如 "v8"
/// * `annotations_json` - 注释信息的 JSON 表示
#[derive(Debug, Clone)]
pub struct Index {
    /// 索引的唯一标识符
    pub id: i64,

    /// 所属镜像的 ID
    pub image_id: i64,

    /// 索引的模式版本
    ///
    /// OCI 规范当前版本通常为 2
    pub schema_version: i64,

    /// 索引的媒体类型
    pub media_type: String,

    /// 目标操作系统
    pub platform_os: Option<String>,

    /// 目标硬件架构
    pub platform_arch: Option<String>,

    /// 平台变体
    pub platform_variant: Option<String>,

    /// 注释信息的 JSON 字符串
    pub annotations_json: Option<String>,

    /// 记录创建时间
    pub created_at: DateTime<Utc>,

    /// 记录最后修改时间
    pub modified_at: DateTime<Utc>,
}

/// 代表数据库中的 OCI 镜像 manifest
///
/// Manifest 是 OCI 镜像的核心元数据文件，描述了镜像的层和配置。
///
/// ## 字段说明
/// * `id` - 数据库中的唯一标识符
/// * `image_id` - 所属镜像的 ID
/// * `schema_version` - manifest 模式版本
/// * `media_type` - 媒体类型
/// * `annotations_json` - 注释信息的 JSON 表示
#[derive(Debug, Clone)]
pub struct Manifest {
    /// manifest 的唯一标识符
    pub id: i64,

    /// 所属镜像的 ID
    pub image_id: i64,

    /// manifest 的模式版本
    pub schema_version: i64,

    /// manifest 的媒体类型
    pub media_type: String,

    /// 注释信息的 JSON 字符串
    pub annotations_json: Option<String>,

    /// 记录创建时间
    pub created_at: DateTime<Utc>,

    /// 记录最后修改时间
    pub modified_at: DateTime<Utc>,
}

/// 代表数据库中的 OCI 镜像配置 (Config)
///
/// Config 包含了运行容器所需的全部配置信息，如环境变量、
/// 启动命令、工作目录等。
///
/// ## 重要字段说明
/// * `config_env_json` - 环境变量的 JSON 数组
/// * `config_cmd_json` - 默认命令的 JSON 数组
/// * `config_entrypoint_json` - 入口点的 JSON 数组
/// * `rootfs_type` - 根文件系统类型，通常是 "layers"
/// * `rootfs_diff_ids_json` - 各层 diff ID 的 JSON 数组
#[derive(Debug, Clone)]
pub struct Config {
    /// 配置的唯一标识符
    pub id: i64,

    /// 所属 manifest 的 ID
    pub manifest_id: i64,

    /// 配置的媒体类型
    pub media_type: String,

    /// 镜像创建时间
    pub created: Option<DateTime<Utc>>,

    /// 镜像架构
    pub architecture: String,

    /// 操作系统
    pub os: String,

    /// 操作系统变体
    pub os_variant: Option<String>,

    /// 环境变量的 JSON 字符串
    pub config_env_json: Option<String>,

    /// 默认命令的 JSON 字符串
    pub config_cmd_json: Option<String>,

    /// 工作目录
    pub config_working_dir: Option<String>,

    /// 入口点的 JSON 字符串
    pub config_entrypoint_json: Option<String>,

    /// 卷挂载点的 JSON 字符串
    pub config_volumes_json: Option<String>,

    /// 暴露端口的 JSON 字符串
    pub config_exposed_ports_json: Option<String>,

    /// 运行用户
    pub config_user: Option<String>,

    /// 根文件系统类型
    pub rootfs_type: String,

    /// 根文件系统 diff IDs 的 JSON 字符串
    pub rootfs_diff_ids_json: Option<String>,

    /// 镜像构建历史的 JSON 字符串
    pub history_json: Option<String>,

    /// 记录创建时间
    pub created_at: DateTime<Utc>,

    /// 记录最后修改时间
    pub modified_at: DateTime<Utc>,
}

/// 代表 OCI 容器镜像中的层 (Layer)
///
/// OCI 镜像由多个层组成，每个层是一个压缩的文件系统快照。
/// 层是只读的，多个镜像可以共享相同的层以节省空间。
///
/// ## 重要概念
/// * `digest` - 压缩层的哈希值，用于唯一标识和验证完整性
/// * `diff_id` - 未压缩层的哈希值，用于层叠加时的引用
/// * `size_bytes` - 压缩后的大小
///
/// ## 字段说明
/// * `id` - 数据库中的唯一标识符
/// * `media_type` - 层的媒体类型
/// * `digest` - 压缩层的 digest
/// * `diff_id` - 未压缩层的 diff ID
/// * `size_bytes` - 层大小（字节）
#[derive(Debug, Clone)]
pub struct Layer {
    /// 层的唯一标识符
    pub id: i64,

    /// 层的媒体类型
    ///
    /// 常见类型：
    /// - "application/vnd.oci.image.layer.v1.tar+gzip" - gzip 压缩层
    /// - "application/vnd.oci.image.layer.v1.tar+zstd" - zstd 压缩层
    pub media_type: String,

    /// 压缩层的 digest（哈希值）
    ///
    /// 格式： "sha256:abcdef..."
    /// 用于唯一标识层和验证下载完整性
    pub digest: String,

    /// 未压缩层的 diff ID
    ///
    /// 这是层内容（解压后）的哈希值
    /// 用于在 config 中引用层
    pub diff_id: String,

    /// 层大小（字节）
    ///
    /// 这是压缩后的大小
    pub size_bytes: i64,

    /// 记录创建时间
    pub created_at: DateTime<Utc>,

    /// 记录最后修改时间
    pub modified_at: DateTime<Utc>,
}
