//! # 错误处理模块
//!
//! 本模块定义了 microsandbox-cli 库中使用的错误类型。
//! Rust 中的错误处理是核心特性之一，本模块使用 `thiserror` crate 来简化错误定义。
//!
//! ## Rust 错误处理基础
//!
//! - `Result<T, E>`: Rust 的标准错误处理类型，表示操作可能成功（Ok）或失败（Err）
//! - `thiserror::Error`: 一个派生宏（derive macro），自动生成 Error trait 的实现
//! - `#[from]`: 自动实现 From trait，允许使用 `?` 操作符进行错误转换

//--------------------------------------------------------------------------------------------------
// Types - 类型定义
//--------------------------------------------------------------------------------------------------

use thiserror::Error;

/// ## 微沙箱 CLI 操作结果类型
///
/// 这是一个类型别名（type alias），简化了返回类型的书写。
///
/// ### 语法说明
/// - `Result<T, MicrosandboxCliError>`: Rust 标准库的枚举类型
///   - `Ok(T)`: 操作成功，包含返回值 T
///   - `Err(MicrosandboxCliError)`: 操作失败，包含错误信息
///
/// ### 泛型参数 T
/// - T 是一个泛型类型参数，表示成功时返回的数据类型
/// - 例如：`MicosandboxCliResult<String>` 等价于 `Result<String, MicrosandboxCliError>`
pub type MicrosandboxCliResult<T> = Result<T, MicrosandboxCliError>;

/// ## 微沙箱 CLI 错误枚举
///
/// 这个枚举定义了 microsandbox-cli 可能遇到的所有错误类型。
/// 使用 `#[derive(Error)]` 宏自动实现 std::error::Error trait。
///
/// ### 关键概念
///
/// **`#[from]` 属性的作用：**
/// - 自动生成 `From<OtherError> for MicrosandboxCliError` 的实现
/// - 允许使用 `?` 操作符自动将子错误转换为父错误
/// - 例如：当一个函数返回 `std::io::Result` 时，可以直接用 `?` 转换为 `MicosandboxCliResult`
///
/// **`#[error("...")]` 属性的作用：**
/// - 定义错误转换为字符串时的显示格式
/// - `{0}` 表示第一个字段的值，`{field}` 命名字段的值
///
/// **`#[error(transparent)]` 的作用：**
/// - 透明地包装另一个错误类型
/// - 错误消息直接来自被包装的错误
#[derive(pretty_error_debug::Debug, Error)]
pub enum MicrosandboxCliError {
    /// ### I/O 错误
    ///
    /// 包装了标准库的 `std::io::Error` 类型。
    /// `#[from]` 属性允许自动转换，例如：
    /// ```rust,ignore
    /// // 当调用 std::fs::File::open() 失败时
    /// let file = std::fs::File::open("config.yaml")?;  // 自动转换为 MicrosandboxCliError::Io
    /// ```
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// ### 微沙箱服务端错误
    ///
    /// 包装来自 `microsandbox-server` crate 的错误。
    /// `transparent` 表示错误消息直接来自内部错误，不添加额外前缀。
    #[error(transparent)]
    Server(#[from] microsandbox_server::MicrosandboxServerError),

    /// ### 微沙箱核心库错误
    ///
    /// 包装来自 `microsandbox-core` crate 的错误。
    /// 这是沙箱核心功能的错误，如 VM 创建、镜像处理等。
    #[error(transparent)]
    Core(#[from] microsandbox_core::MicosandboxError),

    /// ### 无效参数错误
    ///
    /// 当用户提供的命令行参数无效时使用。
    /// 包含一个 String 字段，描述具体的错误原因。
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// ### 未找到错误
    ///
    /// 当请求的资源（如沙箱、配置文件）不存在时使用。
    #[error("not found: {0}")]
    NotFound(String),

    /// ### 进程等待错误
    ///
    /// 当等待子进程结束时发生错误。
    /// 例如：进程被信号终止、等待超时等。
    #[error("process wait error: {0}")]
    ProcessWaitError(String),

    /// ### 配置错误
    ///
    /// 当配置文件格式错误或包含无效设置时使用。
    #[error("configuration error: {0}")]
    ConfigError(String),
}
