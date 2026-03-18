//! # 错误处理模块
//!
//! 这个模块定义了 Microsandbox SDK 使用的错误类型。Rust 的错误处理
//! 是其类型系统的重要组成部分，通过明确的错误类型让开发者清楚地
//! 知道可能发生的各种错误。
//!
//! ## Rust 错误处理基础
//!
//! Rust 使用 `Result<T, E>` 类型来处理错误：
//! - `Ok(T)` - 操作成功，返回值 T
//! - `Err(E)` - 操作失败，返回错误 E
//!
//! 这与其他语言的异常处理不同：
//! - Rust 的错误是**显式的**，必须在类型签名中声明
//! - 错误是**值**，可以被传递、检查和转换
//! - 编译器**强制**你处理错误
//!
//! ## 错误类型层次结构
//!
//! ```text
//! SandboxError (本枚举)
//! ├── NotStarted - 沙箱未启动
//! ├── RequestFailed - 请求失败
//! ├── ServerError - 服务器错误
//! ├── Timeout - 超时错误
//! ├── HttpError - HTTP 客户端错误
//! ├── InvalidResponse - 响应格式错误
//! └── General - 通用错误
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_sdk::{PythonSandbox, BaseSandbox, SandboxError};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut sandbox = PythonSandbox::create("test").await?;
//!
//!     // 尝试在未启动时执行代码
//!     match sandbox.run("print('hello')").await {
//!         Ok(result) => println!("输出：{}", result.output().await?),
//!         Err(e) => {
//!             // 检查具体错误类型
//!             if let Some(sandbox_err) = e.downcast_ref::<SandboxError>() {
//!                 match sandbox_err {
//!                     SandboxError::NotStarted => {
//!                         println!("沙箱未启动，先启动...");
//!                         sandbox.start(None).await?;
//!                         let result = sandbox.run("print('hello')").await?;
//!                         println!("输出：{}", result.output().await?);
//!                     }
//!                     _ => return Err(e),
//!                 }
//!             }
//!         }
//!     }
//!
//!     Ok(())
//! }
//! ```

use std::error::Error;
use std::fmt;

/// # Microsandbox SDK 通用错误类型
///
/// `SandboxError` 枚举定义了 SDK 中所有可能的错误情况。
/// 每个变体都代表了 SDK 使用过程中可能遇到的一种错误。
///
/// ## 错误处理策略
///
/// ### 1. 使用 `?` 操作符向上传播错误
///
/// ```rust,no_run
/// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// fn process_sandbox() -> Result<(), Box<dyn std::error::Error>> {
///     let sandbox = PythonSandbox::create("test").await?; // ? 传播错误
///     // ...
///     Ok(())
/// }
/// # }
/// ```
///
/// ### 2. 使用 `match` 进行详细错误处理
///
/// ```rust
/// # use microsandbox_sdk::SandboxError;
/// fn handle_error(err: SandboxError) {
///     match err {
///         SandboxError::NotStarted => println!("请先启动沙箱"),
///         SandboxError::Timeout(msg) => println!("超时：{}", msg),
///         _ => println!("其他错误：{}", err),
///     }
/// }
/// ```
///
/// ### 3. 使用 `if let` 处理特定错误
///
/// ```rust
/// # use microsandbox_sdk::SandboxError;
/// # fn example(err: Box<dyn std::error::Error>) {
/// if let Some(SandboxError::NotStarted) = err.downcast_ref::<SandboxError>() {
///     println!("沙箱未启动");
/// }
/// # }
/// ```
///
/// ## 实现的 Trait
///
/// - `Debug` - 调试格式化，用于 `{:?}` 打印
/// - `Display` - 用户友好的错误消息，用于 `{}` 打印
/// - `Error` - 标准错误 trait，允许与其他 Rust 错误处理工具互操作
#[derive(Debug)]
pub enum SandboxError {
    /// ## 沙箱未启动错误
    ///
    /// 当尝试在沙箱启动之前执行操作时返回此错误。
    ///
    /// ### 常见场景
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn wrong() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = PythonSandbox::create("test").await?;
    /// // 错误：忘记启动沙箱
    /// let result = sandbox.run("print('hello')").await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ### 解决方法
    ///
    /// 在执行任何操作前调用 `start()`：
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn right() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?; // 先启动
    /// let result = sandbox.run("print('hello')").await?;
    /// # Ok(())
    /// # }
    /// ```
    NotStarted,

    /// ## 请求失败错误
    ///
    /// 当向 Microsandbox 服务器发送请求失败时返回此错误。
    ///
    /// ### 常见原因
    ///
    /// - 网络连接问题
    /// - 服务器拒绝连接
    /// - DNS 解析失败
    /// - SSL/TLS 握手失败
    ///
    /// ### 包含信息
    ///
    /// String 字段包含详细的错误描述，通常来自底层 HTTP 客户端。
    RequestFailed(String),

    /// ## 服务器错误
    ///
    /// 当 Microsandbox 服务器返回错误响应时返回此错误。
    ///
    /// ### 常见原因
    ///
    /// - 认证失败（API 密钥无效）
    /// - 权限不足
    /// - 资源限制（配额用尽）
    /// - 服务器内部错误
    /// - 无效的请求参数
    ///
    /// ### 包含信息
    ///
    /// String 字段包含服务器返回的错误消息，有助于诊断问题。
    ServerError(String),

    /// ## 超时错误
    ///
    /// 当操作超过指定的时间限制时返回此错误。
    ///
    /// ### 常见场景
    ///
    /// - 启动沙箱超时（镜像拉取慢）
    /// - 命令执行超时（计算密集型任务）
    /// - 网络请求超时（服务器响应慢）
    ///
    /// ### 包含信息
    ///
    /// String 字段包含超时详情，包括超时时间和操作类型。
    ///
    /// ### 解决方法
    ///
    /// 增加 `StartOptions` 中的 `timeout` 值：
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox, StartOptions};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut sandbox = PythonSandbox::create("test").await?;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.timeout = 300.0; // 增加到 5 分钟
    ///
    /// sandbox.start(Some(opts)).await?;
    /// # Ok(())
    /// # }
    /// ```
    Timeout(String),

    /// ## HTTP 客户端错误
    ///
    /// 当 HTTP 客户端（reqwest）发生错误时返回此错误。
    ///
    /// ### 常见原因
    ///
    /// - 客户端配置错误
    /// - URL 格式无效
    /// - 请求头过大
    /// - 重定向循环
    ///
    /// ### 包含信息
    ///
    /// String 字段包含来自 reqwest 的错误详情。
    HttpError(String),

    /// ## 无效响应错误
    ///
    /// 当服务器的响应格式不符合预期时返回此错误。
    ///
    /// ### 常见原因
    ///
    /// - 服务器返回了非 JSON 响应
    /// - JSON 结构不符合预期
    /// - 缺少必需的字段
    /// - 字段类型不匹配
    ///
    /// ### 包含信息
    ///
    /// String 字段包含具体的解析错误信息。
    InvalidResponse(String),

    /// ## 通用错误
    ///
    /// 用于其他错误类型不适用的情况。
    ///
    /// ### 使用场景
    ///
    /// - 自定义错误消息
    /// - 未来扩展的错误类型
    /// - 无法分类的错误
    General(String),
}

/// # 实现 Display trait
///
/// `Display` trait 用于用户友好的错误消息格式化。
/// 当你使用 `{}` 格式说明符打印错误时，会使用这个实现。
///
/// ## 格式说明
///
/// 每个错误变体都有清晰的中文消息：
/// - 说明错误类型
/// - 提供上下文信息（如果有）
/// - 有时包含解决建议
impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::NotStarted => write!(f, "Sandbox is not started. Call start() first."),
            SandboxError::RequestFailed(msg) => {
                write!(f, "Failed to communicate with Microsandbox server: {}", msg)
            }
            SandboxError::ServerError(msg) => write!(f, "Server error: {}", msg),
            SandboxError::Timeout(msg) => write!(f, "Timeout error: {}", msg),
            SandboxError::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            SandboxError::InvalidResponse(msg) => {
                write!(f, "Invalid response from server: {}", msg)
            }
            SandboxError::General(msg) => write!(f, "{}", msg),
        }
    }
}

/// # 实现 Error trait
///
/// `Error` trait 是 Rust 标准库中所有错误类型的公共接口。
/// 实现这个 trait 允许 `SandboxError`：
///
/// 1. 与 `Box<dyn Error>` 一起使用
/// 2. 使用 `?` 操作符自动转换
/// 3. 与其他错误处理库互操作
///
/// ## 空白板实现
///
/// `impl Error for SandboxError {}` 是一个"空白板"实现，
/// 意思是使用默认行为。`Error` trait 有默认方法实现，
/// 所以通常不需要额外代码。
impl Error for SandboxError {}
