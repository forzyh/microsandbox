//! # 错误处理模块 - 统一的错误类型定义
//!
//! 本模块定义了微沙箱服务器使用的所有错误类型，提供结构化的错误处理机制。
//!
//! ## 错误类型层次结构
//!
//! ```text
//! MicrosandboxServerError (服务器级别错误)
//! ├── StartError - 服务器启动失败
//! ├── StopError - 服务器停止失败
//! ├── KeyGenError - 密钥生成失败
//! ├── ConfigError - 配置错误
//! ├── IoError - I/O 错误（来自 std::io::Error）
//! └── Utils - 工具库错误（来自 microsandbox-utils）
//!
//! ServerError (应用级别错误)
//! ├── Authentication - 认证失败
//! │   ├── InvalidCredentials - 无效凭证
//! │   ├── EmailNotConfirmed - 邮箱未确认
//! │   ├── TooManyAttempts - 尝试次数过多
//! │   ├── InvalidToken - 无效令牌
//! │   ├── EmailAlreadyExists - 邮箱已注册
//! │   └── ... (其他认证错误)
//! ├── AuthorizationError - 授权失败
//! │   ├── AccessDenied - 访问被拒绝
//! │   └── InsufficientPermissions - 权限不足
//! ├── NotFound - 资源不存在
//! ├── DatabaseError - 数据库错误
//! ├── ValidationError - 验证错误
//! │   ├── InvalidInput - 输入无效
//! │   ├── PasswordTooWeak - 密码太弱
//! │   ├── EmailInvalid - 邮箱格式错误
//! │   └── InvalidConfirmationToken - 确认令牌无效
//! └── InternalError - 内部服务器错误
//! ```
//!
//! ## 错误码设计
//!
//! 错误码用于前端程序化地处理不同类型的错误：
//!
//! | 错误码范围 | 类别 | 示例 |
//! |------------|------|------|
//! | 1001-1099 | 认证错误 | 1001=无效凭证，1005=令牌过期 |
//! | 2001-2099 | 验证错误 | 2001=输入无效，2003=邮箱无效 |
//! | 3001-3099 | 授权错误 | 3001=访问拒绝，3002=权限不足 |
//! | 4001-4099 | 资源错误 | 4001=资源不存在 |
//! | 5001-5999 | 服务器错误 | 5001=数据库错误，5002=内部错误 |
//!
//! ## HTTP 状态码映射
//!
//! 错误会自动映射到适当的 HTTP 状态码：
//!
//! | 错误类型 | HTTP 状态码 | 说明 |
//! |----------|-------------|------|
//! | Authentication | 401 Unauthorized | 认证失败 |
//! | AuthorizationError | 403 Forbidden | 授权失败 |
//! | NotFound | 404 Not Found | 资源不存在 |
//! | ValidationError | 400 Bad Request | 请求验证失败 |
//! | DatabaseError/InternalError | 500 Internal Server Error | 服务器错误 |
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_server::{ServerError, AuthenticationError, ErrorCode};
//!
//! // 返回认证错误
//! fn authenticate_user(credentials: &str) -> Result<(), ServerError> {
//!     if credentials.is_empty() {
//!         return Err(ServerError::Authentication(
//!             AuthenticationError::InvalidCredentials("Empty credentials".to_string())
//!         ));
//!     }
//!     Ok(())
//! }
//!
//! // 错误会自动转换为 JSON 响应
//! // {
//! //     "error": "Authentication failed: Invalid credentials",
//! //     "code": 1001
//! // }
//! ```
//!
//! ## thiserror 库说明
//!
//! 本模块使用 [`thiserror`](https://docs.rs/thiserror) 库来定义错误类型：
//!
//! - `#[derive(Error)]`: 自动实现 `std::error::Error` trait
//! - `#[error("format string")]`: 定义错误的显示格式
//! - `#[from]`: 自动实现 From trait，便于错误转换
//! - `#[source]`: 标记底层错误源

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use microsandbox_utils::MicrosandboxUtilsError;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// # 服务器操作结果类型别名
///
/// 这是 `Result` 类型的简化写法，专门用于服务器操作。
/// 使用此类型别名可以使函数签名更简洁：
///
/// ```rust,ignore
/// // 使用类型别名
/// pub fn do_something() -> MicrosandboxServerResult<String> {
///     Ok("success".to_string())
/// }
///
/// // 等同于完整写法
/// pub fn do_something() -> Result<String, MicrosandboxServerError> {
///     Ok("success".to_string())
/// }
/// ```
pub type MicrosandboxServerResult<T> = Result<T, MicrosandboxServerError>;

/// # 应用操作结果类型别名
///
/// 专门用于应用级别操作的结果类型。
/// 与 `MicrosandboxServerResult` 的区别：
/// - `MicrosandboxServerResult`: 服务器生命周期相关的错误（启动、停止、配置等）
/// - `ServerResult`: 请求处理相关的错误（认证、授权、验证等）
pub type ServerResult<T> = Result<T, ServerError>;

/// # 微沙箱服务器错误枚举
///
/// 此枚举表示服务器级别（而非请求级别）的错误。
/// 主要用于服务器的启动、停止、配置等操作。
///
/// ## 变体说明
///
/// ### `StartError(String)`
/// 服务器启动失败时返回此错误。
/// 可能的原因：
/// - 端口已被占用
/// - 无法创建 PID 文件
/// - 子进程启动失败
///
/// ### `StopError(String)`
/// 服务器停止失败时返回此错误。
/// 可能的原因：
/// - PID 文件不存在
/// - 无法向进程发送信号
/// - 进程已不存在
///
/// ### `KeyGenError(String)`
/// 密钥生成失败时返回此错误。
/// 可能的原因：
/// - 随机数生成器失败
/// - 无法写入密钥文件
/// - 密钥文件权限问题
///
/// ### `ConfigError(String)`
/// 配置相关错误。
/// 可能的原因：
/// - 缺少必需的配置文件
/// - 配置文件格式错误
/// - 配置值无效
///
/// ### `IoError(std::io::Error)`
/// I/O 操作错误，包装了标准库的 `std::io::Error`。
/// 使用 `#[from]` 属性自动实现从 `std::io::Error` 的转换。
///
/// ### `Utils(MicrosandboxUtilsError)`
/// 来自 `microsandbox-utils` 工具库的错误。
/// 同样使用 `#[from]` 自动实现转换。
#[derive(Error, Debug)]
pub enum MicrosandboxServerError {
    /// 服务器启动失败
    ///
    /// 包含详细的错误信息字符串
    #[error("Server failed to start: {0}")]
    StartError(String),

    /// 服务器停止失败
    ///
    /// 包含详细的错误信息字符串
    #[error("Server failed to stop: {0}")]
    StopError(String),

    /// 密钥生成失败
    ///
    /// 包含详细的错误信息字符串
    #[error("Server key failed to generate: {0}")]
    KeyGenError(String),

    /// 服务器配置失败
    ///
    /// 包含详细的错误信息字符串
    #[error("Server configuration failed: {0}")]
    ConfigError(String),

    /// I/O 错误
    ///
    /// 使用 `#[error(transparent)]` 使错误消息直接来自底层错误
    /// 使用 `#[from]` 自动实现 From<std::io::Error>
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    /// 工具库错误
    ///
    /// 来自 microsandbox-utils crate 的错误
    /// 同样使用 transparent 和 from 属性
    #[error(transparent)]
    Utils(#[from] MicrosandboxUtilsError),
}

/// # 应用级服务错误枚举
///
/// 此枚举表示应用程序级别的错误，主要用于 HTTP 请求处理。
/// 每个变体都对应特定的业务场景。
///
/// ## 设计原则
///
/// 1. **错误分类**：将错误按类型分类，便于针对性处理
/// 2. **信息封装**：将详细的错误信息封装在变体中，不直接暴露给客户端
/// 3. **安全考虑**：认证错误返回通用消息，防止信息泄露
///
/// ## 使用场景
///
/// | 变体 | 使用场景 |
/// |------|----------|
/// | `Authentication` | JWT 令牌验证失败、凭证无效 |
/// | `AuthorizationError` | 用户无权访问某资源 |
/// | `NotFound` | 请求的资源（沙箱、配置等）不存在 |
/// | `DatabaseError` | 数据库操作失败 |
/// | `ValidationError` | 输入数据验证失败 |
/// | `InternalError` | 未预期的内部错误 |
#[derive(Error, Debug)]
pub enum ServerError {
    /// 认证失败
    ///
    /// 包装了 `AuthenticationError`，包含具体的认证失败原因
    /// 此错误会映射到 HTTP 401 Unauthorized
    #[error("Authentication failed: {0}")]
    Authentication(AuthenticationError),

    /// 授权失败
    ///
    /// 用户已通过认证，但没有访问特定资源的权限
    /// 此错误会映射到 HTTP 403 Forbidden
    #[error("Authorization failed: {0}")]
    AuthorizationError(AuthorizationError),

    /// 资源不存在
    ///
    /// 请求的资源（如沙箱、配置等）找不到
    /// 此错误会映射到 HTTP 404 Not Found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// 数据库错误
    ///
    /// 数据库操作失败
    /// 此错误会映射到 HTTP 500 Internal Server Error
    /// 注意：详细的错误信息不会暴露给客户端，以防泄露数据库结构
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// 验证错误
    ///
    /// 请求数据验证失败（如格式错误、缺少必填字段等）
    /// 此错误会映射到 HTTP 400 Bad Request
    #[error("Validation error: {0}")]
    ValidationError(ValidationError),

    /// 内部服务器错误
    ///
    /// 未预期的错误，通常是程序 bug 或异常情况
    /// 此错误会映射到 HTTP 500 Internal Server Error
    /// 注意：详细的错误信息不会暴露给客户端
    #[error("Internal server error: {0}")]
    InternalError(String),
}

/// # 错误码枚举
///
/// 为前端提供程序化的错误处理方式。
/// 前端可以根据错误码显示不同的 UI 或执行特定的重试逻辑。
///
/// ## 命名规范
///
/// 错误码命名采用 `PascalCase`，格式为 `<错误类别><具体错误>`：
/// - `InvalidCredentials`: 无效凭证
/// - `EmailNotConfirmed`: 邮箱未确认
/// - `AccessDenied`: 访问拒绝
///
/// ## 序列化
///
/// 此枚举实现了 `Serialize` trait，可以直接序列化为 JSON：
/// ```json
/// {
///     "error": "Invalid credentials",
///     "code": 1001
/// }
/// ```
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // ==================== 认证错误码 (1001-1099) ====================
    /// 凭证无效
    ///
    /// 用户名/密码错误或令牌无效
    InvalidCredentials = 1001,

    /// 邮箱未确认
    ///
    /// 用户注册后未点击确认邮件中的链接
    EmailNotConfirmed = 1002,

    /// 登录尝试次数过多
    ///
    /// 触发速率限制，需要等待一段时间后重试
    TooManyLoginAttempts = 1003,

    /// 令牌无效
    ///
    /// JWT 令牌格式错误或签名验证失败
    InvalidToken = 1004,

    /// 令牌过期
    ///
    /// JWT 令牌已超过有效期
    ExpiredToken = 1005,

    /// 需要提供令牌
    ///
    /// 请求缺少认证信息
    TokenRequired = 1006,

    /// 邮箱已注册
    ///
    /// 尝试用已注册的邮箱注册新账户
    EmailAlreadyExists = 1007,

    /// 应使用 Google 登录
    ///
    /// 该邮箱通过 Google OAuth 注册，应使用"使用 Google 登录"
    UseGoogleLogin = 1008,

    /// 应使用 GitHub 登录
    ///
    /// 该邮箱通过 GitHub OAuth 注册，应使用"使用 GitHub 登录"
    UseGithubLogin = 1009,

    /// 应使用邮箱/密码登录
    ///
    /// 该邮箱通过传统方式注册，应使用邮箱/密码登录
    UseEmailLogin = 1010,

    /// 邮箱在 OAuth 提供商处未验证
    ///
    /// OAuth 返回的邮箱未经验证
    EmailNotVerified = 1011,

    // ==================== 验证错误码 (2001-2099) ====================
    /// 输入无效
    ///
    /// 通用验证失败错误
    InvalidInput = 2001,

    /// 密码强度不足
    ///
    /// 密码不符合复杂度要求（长度、字符种类等）
    PasswordTooWeak = 2002,

    /// 邮箱格式无效
    ///
    /// 邮箱地址不符合标准格式
    EmailInvalid = 2003,

    /// 确认令牌无效或已过期
    ///
    /// 邮箱确认链接中的令牌无效或已过期
    InvalidOrExpiredConfirmationToken = 2004,

    // ==================== 授权错误码 (3001-3099) ====================
    /// 访问被拒绝
    ///
    /// 用户无权访问此资源
    AccessDenied = 3001,

    /// 权限不足
    ///
    /// 用户权限不足以执行此操作
    InsufficientPermissions = 3002,

    // ==================== 资源错误码 (4001-4099) ====================
    /// 资源不存在
    ///
    /// 请求的资源找不到
    ResourceNotFound = 4001,

    // ==================== 服务器错误码 (5001-5999) ====================
    /// 数据库错误
    ///
    /// 数据库操作失败
    DatabaseError = 5001,

    /// 内部服务器错误
    ///
    /// 未预期的服务器错误
    InternalServerError = 5002,
}

/// # 认证错误详细类型
///
/// 此枚举提供了认证失败的详细原因分类。
/// 与 `ServerError::Authentication` 配合使用。
///
/// ## 安全考虑
///
/// 为了安全，某些错误（如 `InvalidCredentials`）对外返回通用消息，
/// 防止攻击者通过错误信息判断用户名是否存在。
#[derive(Error, Debug)]
pub enum AuthenticationError {
    /// 无效凭证（安全敏感）
    ///
    /// 此错误包含详细信息用于日志记录，
    /// 但返回给客户端的消息是通用的"Invalid credentials"
    #[error("Invalid credentials")]
    InvalidCredentials(String),

    /// 客户端错误（可直接显示给用户）
    ///
    /// 此类错误的安全敏感度较低，可以直接显示给用户
    #[error("{0}")]
    ClientError(String),

    /// 邮箱未确认
    #[error("Email not confirmed")]
    EmailNotConfirmed,

    /// 尝试次数过多（触发速率限制）
    #[error("Too many login attempts")]
    TooManyAttempts,

    /// 令牌无效或过期
    #[error("Invalid or expired token")]
    InvalidToken(String),

    /// 邮箱已注册
    #[error("Email already registered")]
    EmailAlreadyExists,

    /// 应使用 Google 登录
    #[error("Use Google login")]
    UseGoogleLogin,

    /// 应使用 GitHub 登录
    #[error("Use GitHub login")]
    UseGithubLogin,

    /// 应使用邮箱/密码登录
    #[error("Use email/password login")]
    UseEmailLogin,

    /// 邮箱在提供商处未验证
    #[error("Email not verified")]
    EmailNotVerified,
}

/// # 验证错误详细类型
///
/// 此枚举提供了输入验证失败的详细原因。
#[derive(Error, Debug)]
pub enum ValidationError {
    /// 通用输入无效
    ///
    /// 包含详细的验证失败信息
    #[error("{0}")]
    InvalidInput(String),

    /// 密码强度不足
    ///
    /// 包含具体的密码策略要求
    #[error("Password is too weak")]
    PasswordTooWeak(String),

    /// 邮箱格式无效
    ///
    /// 包含验证失败的原因
    #[error("Email is invalid")]
    EmailInvalid(String),

    /// 确认令牌无效或已过期
    #[error("Invalid or expired confirmation token")]
    InvalidConfirmationToken,
}

/// # 授权错误详细类型
///
/// 此枚举提供了授权失败的详细原因。
#[derive(Error, Debug)]
pub enum AuthorizationError {
    /// 访问被拒绝
    ///
    /// 用户无权访问此资源
    #[error("Access denied")]
    AccessDenied(String),

    /// 权限不足
    ///
    /// 用户权限不足以执行此操作
    #[error("Insufficient permissions")]
    InsufficientPermissions(String),
}

/// # 错误响应结构体
///
/// 这是发送给客户端的 JSON 错误响应格式。
///
/// ## JSON 示例
///
/// ```json
/// {
///     "error": "Authentication failed: Invalid credentials",
///     "code": 1001
/// }
/// ```
///
/// ## 字段说明
///
/// - `error`: 人类可读的错误描述
/// - `code`: 可选的错误码，便于前端程序化处理
#[derive(Serialize)]
struct ErrorResponse {
    /// 错误描述消息
    error: String,
    /// 可选的错误码（来自 ErrorCode 枚举）
    code: Option<u32>,
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

/// # 将 ServerError 转换为 HTTP 响应
///
/// 此实现是 Axum 框架的核心集成点。
/// `IntoResponse` trait 允许将自定义类型转换为 HTTP 响应。
///
/// ## 工作流程
///
/// 1. **记录错误日志**：使用 `tracing::error!` 记录错误详情（仅服务器可见）
/// 2. **匹配错误类型**：根据错误类型确定 HTTP 状态码和错误码
/// 3. **构建响应**：创建包含错误消息和错误码的 JSON 响应
/// 4. **返回响应**：Axum 自动将响应发送给客户端
///
/// ## 安全设计
///
/// - 认证错误返回通用消息，防止信息泄露
/// - 详细错误信息仅记录到服务器日志
/// - 数据库错误返回通用消息，防止数据库结构泄露
///
/// ## HTTP 状态码映射
///
/// | ServerError 变体 | HTTP 状态码 |
/// |------------------|-------------|
/// | Authentication | 401 Unauthorized |
/// | AuthorizationError | 403 Forbidden |
/// | NotFound | 404 Not Found |
/// | ValidationError | 400 Bad Request |
/// | DatabaseError | 500 Internal Server Error |
/// | InternalError | 500 Internal Server Error |
impl IntoResponse for ServerError {
    /// 将 ServerError 转换为带有适当状态码和 JSON 错误消息的 HTTP 响应
    ///
    /// ## 返回值
    ///
    /// 返回一个 HTTP 响应，包含：
    /// - 根据错误类型设置的适当 HTTP 状态码
    /// - JSON 格式的错误响应体，包含 "error" 字段和可选的 "code" 字段
    fn into_response(self) -> Response {
        // 使用 tracing 记录错误详情（仅服务器日志可见）
        // ?self 语法使用 Debug trait 格式化错误
        error!(error = ?self, "API error occurred");

        // 模式匹配处理所有错误类型
        let (status, error_message, error_code) = match self {
            // ==================== 认证错误处理 ====================
            ServerError::Authentication(auth_error) => {
                match auth_error {
                    // 无效凭证：返回通用消息（安全考虑）
                    AuthenticationError::InvalidCredentials(_details) => {
                        // 详细日志（服务器端）
                        error!(details = ?_details, "Authentication error");
                        // 通用客户端消息（防止信息泄露）
                        (StatusCode::UNAUTHORIZED, "Invalid credentials".to_string(), Some(ErrorCode::InvalidCredentials as u32))
                    }
                    // 客户端错误：可以直接显示给用户
                    AuthenticationError::ClientError(details) => {
                        error!(details = ?details, "User-facing authentication error");
                        (StatusCode::UNAUTHORIZED, details, None)
                    }
                    // 邮箱未确认
                    AuthenticationError::EmailNotConfirmed => {
                        (StatusCode::UNAUTHORIZED, "Email not confirmed".to_string(), Some(ErrorCode::EmailNotConfirmed as u32))
                    }
                    // 尝试次数过多（速率限制）
                    AuthenticationError::TooManyAttempts => {
                        (StatusCode::TOO_MANY_REQUESTS, "Too many login attempts, please try again later".to_string(), Some(ErrorCode::TooManyLoginAttempts as u32))
                    }
                    // 令牌无效
                    AuthenticationError::InvalidToken(details) => {
                        error!(details = ?details, "Invalid token");
                        (StatusCode::UNAUTHORIZED, "Invalid or expired token".to_string(), Some(ErrorCode::InvalidToken as u32))
                    }
                    // 邮箱已注册
                    AuthenticationError::EmailAlreadyExists => {
                        (StatusCode::CONFLICT, "Email already registered".to_string(), Some(ErrorCode::EmailAlreadyExists as u32))
                    }
                    // OAuth 相关错误
                    AuthenticationError::UseGoogleLogin => {
                        (StatusCode::UNAUTHORIZED, "This email is registered with Google. Please use 'Sign in with Google' instead.".to_string(), Some(ErrorCode::UseGoogleLogin as u32))
                    }
                    AuthenticationError::UseGithubLogin => {
                        (StatusCode::UNAUTHORIZED, "This email is registered with GitHub. Please use 'Sign in with GitHub' instead.".to_string(), Some(ErrorCode::UseGithubLogin as u32))
                    }
                    AuthenticationError::UseEmailLogin => {
                        (StatusCode::UNAUTHORIZED, "This email is already registered. Please login with your password.".to_string(), Some(ErrorCode::UseEmailLogin as u32))
                    }
                    AuthenticationError::EmailNotVerified => {
                        (StatusCode::UNAUTHORIZED, "Email not verified with the provider".to_string(), Some(ErrorCode::EmailNotVerified as u32))
                    }
                }
            }
            // ==================== 授权错误处理 ====================
            ServerError::AuthorizationError(auth_error) => match auth_error {
                AuthorizationError::AccessDenied(details) => {
                    error!(details = ?details, "Access denied");
                    (
                        StatusCode::FORBIDDEN,
                        "Access denied".to_string(),
                        Some(ErrorCode::AccessDenied as u32),
                    )
                }
                AuthorizationError::InsufficientPermissions(details) => {
                    error!(details = ?details, "Insufficient permissions");
                    (
                        StatusCode::FORBIDDEN,
                        "Insufficient permissions".to_string(),
                        Some(ErrorCode::InsufficientPermissions as u32),
                    )
                }
            },
            // ==================== 资源不存在 ====================
            ServerError::NotFound(details) => (
                StatusCode::NOT_FOUND,
                details,
                Some(ErrorCode::ResourceNotFound as u32),
            ),
            // ==================== 数据库错误 ====================
            // 注意：返回通用消息，防止泄露数据库结构
            ServerError::DatabaseError(details) => {
                error!(details = ?details, "Database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                    Some(ErrorCode::DatabaseError as u32),
                )
            }
            // ==================== 验证错误处理 ====================
            ServerError::ValidationError(validation_error) => match validation_error {
                ValidationError::InvalidInput(details) => (
                    StatusCode::BAD_REQUEST,
                    details,
                    Some(ErrorCode::InvalidInput as u32),
                ),
                ValidationError::PasswordTooWeak(details) => (
                    StatusCode::BAD_REQUEST,
                    details,
                    Some(ErrorCode::PasswordTooWeak as u32),
                ),
                ValidationError::EmailInvalid(details) => (
                    StatusCode::BAD_REQUEST,
                    details,
                    Some(ErrorCode::EmailInvalid as u32),
                ),
                ValidationError::InvalidConfirmationToken => (
                    StatusCode::BAD_REQUEST,
                    "Invalid or expired confirmation token".to_string(),
                    Some(ErrorCode::InvalidOrExpiredConfirmationToken as u32),
                ),
            },
            // ==================== 内部错误 ====================
            // 注意：返回通用消息，防止泄露内部实现细节
            ServerError::InternalError(details) => {
                error!(details = ?details, "Internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                    Some(ErrorCode::InternalServerError as u32),
                )
            }
        };

        // 构建 JSON 错误响应体
        let body = Json(ErrorResponse {
            error: error_message,
            code: error_code,
        });

        // 组合状态码和响应体
        (status, body).into_response()
    }
}
