//! # 错误处理模块
//!
//! 本模块定义了 microsandbox 项目中使用的各种错误类型。
//! 使用 `thiserror` crate 来提供美观的错误调试输出和自动的 `From` trait 实现。

use microsandbox_utils::MicrosandboxUtilsError;
use oci_client::errors::OciDistributionError;
use sqlx::migrate::MigrateError;
use std::{
    error::Error,
    fmt::{self, Display},
    path::{PathBuf, StripPrefixError},
    time::SystemTimeError,
};
use thiserror::Error;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// microsandbox 相关操作的结果类型别名
///
/// 这是一个泛型类型，`T` 是成功时返回的值类型
/// 错误类型固定为 `MicrosandboxError`
///
/// ## 使用示例
/// ```rust
/// use microsandbox_core::{MicrosandboxResult, MicrosandboxError};
///
/// fn do_something() -> MicrosandboxResult<String> {
///     // 成功时返回 Ok
///     Ok("success".to_string())
///     // 或者失败时返回 Err(MicrosandboxError::...)
/// }
/// ```
pub type MicrosandboxResult<T> = Result<T, MicrosandboxError>;

/// microsandbox 操作中可能发生的错误
///
/// 这个枚举包含了所有可能的错误情况，使用 `#[from]` 属性可以自动
/// 将其他错误类型转换为 `MicrosandboxError`
#[derive(pretty_error_debug::Debug, Error)]
pub enum MicrosandboxError {
    /// I/O 错误（来自标准库的 std::io::Error）
    ///
    /// 这个错误变体使用 `#[from]` 属性，意味着任何 `std::io::Error` 都可以
    /// 自动转换为这个错误类型，无需手动调用 `.into()`
    #[error("io 错误：{0}")]
    Io(#[from] std::io::Error),

    /// 可以表示任何错误的通用错误类型
    ///
    /// 使用 `anyhow::Error` 作为后备错误类型，用于处理无法归入其他
    /// 特定类别的错误
    #[error(transparent)]
    AnyError(#[from] anyhow::Error),

    /// OCI 分发操作中发生的错误
    ///
    /// OCI (Open Container Initiative) 是容器镜像的标准规范
    /// 这个错误在从容器注册表拉取镜像时可能发生
    #[error("oci 分发错误：{0}")]
    OciDistribution(#[from] OciDistributionError),

    /// HTTP 请求过程中发生的错误
    ///
    /// 使用 `reqwest` crate 进行 HTTP 通信，这个错误在访问
    /// 容器注册表 API 时可能发生
    #[error("http 请求错误：{0}")]
    HttpRequest(#[from] reqwest::Error),

    /// HTTP 中间件操作中发生的错误
    ///
    /// 使用 `reqwest_middleware` 提供额外的 HTTP 功能，如重试、认证等
    #[error("http 中间件错误：{0}")]
    HttpMiddleware(#[from] reqwest_middleware::Error),

    /// 数据库操作中发生的错误
    ///
    /// 使用 `sqlx` 异步 SQL 数据库操作，主要是 SQLite
    #[error("数据库错误：{0}")]
    Database(#[from] sqlx::Error),

    /// 未找到 manifest 时的错误
    ///
    /// manifest 是 OCI 镜像的元数据文件，描述了镜像的层和配置
    #[error("manifest 未找到")]
    ManifestNotFound,

    /// join handle 返回错误时的错误
    ///
    /// 在使用 `tokio::spawn` 创建异步任务后，等待任务完成时可能收到此错误
    #[error("join 错误：{0}")]
    JoinError(#[from] tokio::task::JoinError),

    /// 使用了不支持的镜像哈希算法
    ///
    /// OCI 镜像支持多种哈希算法（如 SHA-256、SHA-384、SHA-512）
    /// 当遇到不支持的算法时会返回此错误
    #[error("不支持的镜像哈希算法：{0}")]
    UnsupportedImageHashAlgorithm(String),

    /// 镜像层下载失败
    ///
    /// OCI 镜像由多个层组成，每个层是一个压缩的文件系统快照
    /// 下载任一层失败都会导致此错误
    #[error("镜像层下载失败：{0}")]
    ImageLayerDownloadFailed(String),

    /// 使用了无效的路径对 (PathPair)
    ///
    /// PathPair 用于表示主机和访客系统之间的路径映射
    /// 格式为 "host_path:guest_path"
    #[error("无效的路径对：{0}")]
    InvalidPathPair(String),

    /// 使用了无效的端口对 (PortPair)
    ///
    /// PortPair 用于表示主机和访客系统之间的端口映射
    /// 格式为 "host_port:guest_port"
    #[error("无效的端口对：{0}")]
    InvalidPortPair(String),

    /// 使用了无效的环境变量对
    ///
    /// 环境变量对格式为 "NAME=value"
    #[error("无效的环境变量对：{0}")]
    InvalidEnvPair(String),

    /// MicroVm 配置无效时发生的错误
    #[error("无效的 MicroVm 配置：{0}")]
    InvalidMicroVMConfig(InvalidMicroVMConfigError),

    /// 资源限制格式无效
    ///
    /// Linux 资源限制格式为 "RESOURCE=soft:hard"
    /// 例如 "RLIMIT_NOFILE=1024:2048"
    #[error("无效的资源限制格式：{0}")]
    InvalidRLimitFormat(String),

    /// 资源限制值无效
    #[error("无效的资源限制值：{0}")]
    InvalidRLimitValue(String),

    /// 资源限制资源类型无效
    #[error("无效的资源限制资源：{0}")]
    InvalidRLimitResource(String),

    /// Serde JSON 序列化/反序列化错误
    #[error("serde json 错误：{0}")]
    SerdeJson(#[from] serde_json::Error),

    /// Serde YAML 序列化/反序列化错误
    #[error("serde yaml 错误：{0}")]
    SerdeYaml(#[from] serde_yaml::Error),

    /// TOML 解析错误
    #[error("toml 错误：{0}")]
    Toml(#[from] toml::de::Error),

    /// 配置验证失败
    #[error("配置验证错误：{0}")]
    ConfigValidation(String),

    /// 多个配置验证错误
    #[error("配置验证错误：{0:?}")]
    ConfigValidationErrors(Vec<String>),

    /// 尝试访问不属于任何组的服务的资源时的错误
    #[error("服务 '{0}' 不属于任何组")]
    ServiceBelongsToNoGroup(String),

    /// 尝试访问属于不同组的服务的资源时的错误
    #[error("服务 '{0}' 属于错误的组：'{1}'")]
    ServiceBelongsToWrongGroup(String, String),

    /// 无法获取关机 eventfd
    ///
    /// eventfd 是 Linux 用于事件通知的文件描述符
    #[error("无法获取关机 eventfd: {0}")]
    FailedToGetShutdownEventFd(i32),

    /// 无法写入关机 eventfd
    #[error("无法写入关机 eventfd: {0}")]
    FailedToShutdown(String),

    /// 启动 VM 失败
    #[error("启动 VM 失败：{0}")]
    FailedToStartVM(i32),

    /// 路径不存在
    #[error("路径不存在：{0}")]
    PathNotFound(String),

    /// rootfs 路径不存在
    ///
    /// rootfs (root filesystem) 是容器的根文件系统
    #[error("rootfs 路径不存在：{0}")]
    RootFsPathNotFound(String),

    /// 未找到 supervisor 二进制文件
    ///
    /// supervisor 是监督和管理沙箱进程的程序
    #[error("supervisor 二进制文件未找到：{0}")]
    SupervisorBinaryNotFound(String),

    /// 启动 VM 失败（备用错误类型）
    #[error("启动 VM 失败：{0}")]
    StartVmFailed(i32),

    /// 等待进程退出时发生错误
    #[error("进程等待错误：{0}")]
    ProcessWaitError(String),

    /// 运行 supervisor 时发生错误
    #[error("supervisor 错误：{0}")]
    SupervisorError(String),

    /// 终止进程失败
    #[error("无法终止进程：{0}")]
    ProcessKillError(String),

    /// 配置合并时发生错误
    #[error("配置合并错误：{0}")]
    ConfigMerge(String),

    /// 没有更多可分配的 IP 地址
    ///
    /// 当 IP 地址池耗尽时返回此错误
    #[error("没有可用的 IP 地址")]
    NoAvailableIPs,

    /// walkdir 操作中发生错误
    ///
    /// walkdir 用于递归遍历目录树
    #[error("walkdir 错误：{0}")]
    WalkDir(#[from] walkdir::Error),

    /// 移除路径前缀时发生错误
    #[error("strip prefix 错误：{0}")]
    StripPrefix(#[from] StripPrefixError),

    /// nix crate 操作中发生错误
    ///
    /// nix 提供了对 POSIX 系统调用的 Rust 绑定
    #[error("nix 错误：{0}")]
    NixError(#[from] nix::Error),

    /// 系统时间转换错误
    #[error("系统时间错误：{0}")]
    SystemTime(#[from] SystemTimeError),

    /// 层提取时发生错误
    ///
    /// 这通常发生在阻塞任务的 join handle 失败时
    #[error("层提取错误：{0}")]
    LayerExtraction(String),

    /// 层处理操作（如打开文件或解包归档）时发生错误
    ///
    /// 包含底层的 I/O 错误和正在处理的层路径
    #[error("层处理错误：{source}")]
    LayerHandling {
        /// 底层的 I/O 错误
        source: std::io::Error,
        /// 发生错误时正在处理的层路径
        layer: String,
    },

    /// 配置文件未找到
    #[error("配置文件未找到：{0}")]
    ConfigNotFound(String),

    /// 服务的 rootfs 目录未找到
    #[error("服务 rootfs 未找到：{0}")]
    RootfsNotFound(String),

    /// 解析镜像引用时发生错误
    ///
    /// 镜像引用格式如 "docker.io/library/ubuntu:latest"
    #[error("无效的镜像引用：{0}")]
    ImageReferenceError(String),

    /// 尝试删除正在运行的服务时的错误
    #[error("无法删除正在运行的服务：{0}")]
    ServiceStillRunning(String),

    /// 命令行参数无效
    #[error("{0}")]
    InvalidArgument(String),

    /// 路径验证失败
    #[error("路径验证错误：{0}")]
    PathValidation(String),

    /// microsandbox 配置文件未找到
    #[error("microsandbox 配置文件未找到于：{0}")]
    MicrosandboxConfigNotFound(String),

    /// 解析配置文件失败
    #[error("解析配置文件失败：{0}")]
    ConfigParseError(String),

    /// 日志文件未找到
    #[error("日志未找到：{0}")]
    LogNotFound(String),

    /// pager 错误
    ///
    /// pager 用于分页显示长文本输出
    #[error("pager 错误：{0}")]
    PagerError(String),

    /// 来自 microsandbox-utils 的错误
    #[error("microsandbox-utils 错误：{0}")]
    MicrosandboxUtilsError(#[from] MicrosandboxUtilsError),

    /// 数据库迁移错误
    #[error("迁移错误：{0}")]
    MigrationError(#[from] MigrateError),

    /// 功能尚未实现
    #[error("功能尚未实现：{0}")]
    NotImplemented(String),

    /// 在配置中找不到指定的沙箱
    #[error("在 '{1}' 中找不到沙箱：'{0}'")]
    SandboxNotFoundInConfig(String, PathBuf),

    /// 使用了无效的日志级别
    #[error("无效的日志级别：{0}")]
    InvalidLogLevel(u8),

    /// 路径段为空
    #[error("路径段为空")]
    EmptyPathSegment,

    /// 路径组件无效（如 "."、".."、"/"）
    #[error("无效的路径组件：{0}")]
    InvalidPathComponent(String),

    /// 在沙箱配置中找不到指定的脚本
    #[error("在沙箱配置 '{1}' 中找不到脚本 '{0}'")]
    ScriptNotFoundInSandbox(String, String),

    /// 运行沙箱服务器时发生错误
    #[error("沙箱服务器错误：{0}")]
    SandboxServerError(String),

    /// 使用了无效的网络范围
    ///
    /// 网络范围定义了沙箱可以访问的网络地址范围
    #[error("无效的网络范围：{0}")]
    InvalidNetworkScope(String),

    /// 缺少启动脚本、exec 命令或 shell
    #[error("缺少启动脚本或 exec 命令或 shell")]
    MissingStartOrExecOrShell,

    /// 尝试安装与现有命令同名的脚本
    #[error("命令已存在：{0}")]
    CommandExists(String),

    /// 命令未找到
    #[error("命令未找到：{0}")]
    CommandNotFound(String),

    /// 解析 OCI 规范失败
    #[error("解析 OCI 规范失败：{0}")]
    SpecError(#[from] oci_spec::OciSpecError),

    /// 解析 OCI 引用失败
    #[error("解析 OCI 引用失败：{0}")]
    ParseError(#[from] oci_client::ParseError),
}

/// MicroVm 配置无效时的详细错误
///
/// 这个枚举提供了更具体的 MicroVm 配置错误原因
#[derive(Debug, Error)]
pub enum InvalidMicroVMConfigError {
    /// root 路径不存在
    #[error("root 路径不存在：{0}")]
    RootPathDoesNotExist(String),

    /// 应该挂载的主机路径不存在
    #[error("主机路径不存在：{0}")]
    HostPathDoesNotExist(String),

    /// vCPU 数量为零
    ///
    /// vCPU (virtual CPU) 是分配给虚拟机的虚拟处理器数量
    #[error("vCPU 数量为零")]
    NumVCPUsIsZero,

    /// 内存大小为零
    #[error("内存大小为零")]
    MemoryIsZero,

    /// 命令行包含无效字符
    ///
    /// 只允许使用可打印的 ASCII 字符（空格到波浪线之间的字符）
    #[error("命令行包含无效字符（只允许使用空格到波浪线之间的 ASCII 字符）：{0}")]
    InvalidCommandLineString(String),

    /// 当检测到冲突的访客路径时发生
    ///
    /// 例如，如果一个挂载点是 /app，另一个是 /app/data，就会产生冲突
    #[error("冲突的访客路径：'{0}' 和 '{1}' 重叠")]
    ConflictingGuestPaths(String, String),
}

/// 可以表示任何错误的类型
///
/// 这是一个包装器，内部使用 `anyhow::Error`
#[derive(Debug)]
pub struct AnyError {
    error: anyhow::Error,
}

//--------------------------------------------------------------------------------------------------
// 方法
//--------------------------------------------------------------------------------------------------

impl AnyError {
    /// 将错误向下转换为类型 `T`
    ///
    /// ## 参数
    /// * `T` - 要转换到的目标错误类型
    ///
    /// ## 返回
    /// 如果内部错误是指定的类型 `T`，返回 `Some(&T)`，否则返回 `None`
    pub fn downcast<T>(&self) -> Option<&T>
    where
        T: Display + fmt::Debug + Send + Sync + 'static,
    {
        self.error.downcast_ref::<T>()
    }
}

//--------------------------------------------------------------------------------------------------
// 函数
//--------------------------------------------------------------------------------------------------

/// 创建一个 `Ok` 的 `MicrosandboxResult`
///
/// 这个函数只是一个辅助函数，使代码更加一致
#[allow(non_snake_case)]
pub fn Ok<T>(value: T) -> MicrosandboxResult<T> {
    Result::Ok(value)
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl PartialEq for AnyError {
    fn eq(&self, other: &Self) -> bool {
        self.error.to_string() == other.error.to_string()
    }
}

impl Display for AnyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl Error for AnyError {}
