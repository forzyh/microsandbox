//! # 错误处理模块 (Error Handling)
//!
//! 本模块定义了 microsandbox portal 使用的错误类型。
//!
//! ## 错误处理架构
//!
//! 本模块使用 `thiserror` crate 来简化错误类型的定义。`thiserror` 是一个
//! 用于定义错误类型的派生宏库，它会自动实现 `std::error::Error` trait。
//!
//! ## PortalError 类型
//!
//! `PortalError` 是一个枚举类型，表示 portal 服务可能遇到的各种错误：
//!
//! | 变体 | 说明 | HTTP 状态码 | JSON-RPC 错误码 |
//! |------|------|-------------|----------------|
//! | `JsonRpc` | JSON-RPC 协议相关错误 | 400 Bad Request | -32600 |
//! | `MethodNotFound` | 请求的方法不存在 | 404 Not Found | -32601 |
//! | `Internal` | 内部服务器错误 | 500 Internal Server Error | -32603 |
//! | `Parse` | JSON 解析错误 | 400 Bad Request | -32700 |
//!
//! ## thiserror 使用说明
//!
//! ```rust
//! #[derive(Debug, Error)]
//! pub enum PortalError {
//!     // #[error("...")] 属性定义 Display 实现
//!     // {0} 表示第一个字段
//!     #[error("JSON-RPC error: {0}")]
//!     JsonRpc(String),
//! }
//! ```
//!
//! ## IntoResponse trait
//!
//! `IntoResponse` 是 Axum 框架的 trait，用于将类型转换为 HTTP 响应。
//! 实现此 trait 后，`PortalError` 可以直接作为处理函数的返回值。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

use crate::payload::JsonRpcError;

//--------------------------------------------------------------------------------------------------
// 类型 (Types)
//--------------------------------------------------------------------------------------------------

/// # Portal 主要错误类型
///
/// 此枚举封装了 microsandbox portal 可能遇到的所有错误情况。
///
/// ## 变体说明
///
/// ### JsonRpc(String)
/// 与 JSON-RPC 协议相关的错误，如：
/// - 无效的请求格式
/// - 参数解析失败
/// - 版本不匹配
///
/// ### MethodNotFound(String)
/// 请求的方法不存在，如：
/// - 拼写错误的方法名
/// - 未实现的方法
///
/// ### Internal(String)
/// 服务器内部错误，如：
/// - 引擎初始化失败
/// - 资源分配失败
/// - 意外的运行时错误
///
/// ### Parse(String)
/// JSON 解析错误，如：
/// - 无效的 JSON 语法
/// - 类型不匹配
///
/// ## 使用示例
///
/// ```rust
/// // 创建错误
/// let err = PortalError::JsonRpc("Invalid params".to_string());
///
/// // 使用 ? 操作符传播错误
/// fn process() -> Result<(), PortalError> {
///     // ...
///     Err(PortalError::Internal("Something went wrong".to_string()))
/// }
/// ```
#[derive(Debug, Error)]
pub enum PortalError {
    /// JSON-RPC 协议相关错误
    ///
    /// 包含错误的详细描述信息
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),

    /// 方法未找到错误
    ///
    /// 包含请求的方法名称
    #[error("Method not found: {0}")]
    MethodNotFound(String),

    /// 内部服务器错误
    ///
    /// 包含错误的详细描述
    #[error("Internal server error: {0}")]
    Internal(String),

    /// 解析错误
    ///
    /// 包含解析失败的详细信息
    #[error("Parse error: {0}")]
    Parse(String),
}

//--------------------------------------------------------------------------------------------------
// Trait 实现 (Trait Implementations)
//--------------------------------------------------------------------------------------------------

/// # 将 PortalError 转换为 HTTP 响应
///
/// 此实现允许 `PortalError` 直接作为 Axum 处理函数的返回值。
///
/// ## IntoResponse trait 说明
///
/// `IntoResponse` 是 Axum 框架的核心 trait，定义了如何将一个类型
/// 转换为 HTTP 响应。实现此 trait 后，类型可以：
/// - 作为路由处理函数的返回值
/// - 自动设置适当的 HTTP 状态码
/// - 自动设置响应体和头部
///
/// ## 转换规则
///
/// | PortalError 变体 | HTTP 状态码 | JSON-RPC 错误码 |
/// |------------------|-------------|-----------------|
/// | JsonRpc | 400 Bad Request | -32600 (Invalid Request) |
/// | MethodNotFound | 404 Not Found | -32601 (Method not found) |
/// | Parse | 400 Bad Request | -32700 (Parse error) |
/// | Internal | 500 Internal Server Error | -32603 (Internal error) |
///
/// ## 实现细节
///
/// 1. 使用 `match` 模式匹配错误变体
/// 2. 为每种错误创建对应的 `JsonRpcError` 对象
/// 3. 返回 `(StatusCode, Json(error_response))` 元组
/// 4. Axum 自动将此元组转换为完整的 HTTP 响应
///
/// ## 使用示例
///
/// ```rust
/// // 处理函数可以直接返回 PortalError
/// async fn handler() -> Result<Json<Response>, PortalError> {
///     // 如果出错，自动转换为 HTTP 响应
///     Err(PortalError::Internal("Error".to_string()))
/// }
/// ```
impl IntoResponse for PortalError {
    /// 将 PortalError 转换为 HTTP 响应
    ///
    /// 此方法消耗 `self`（使用 `self` 而非 `&self`），因为错误在转换后不再需要。
    fn into_response(self) -> Response {
        // 使用 match 模式匹配不同的错误变体
        // self 被移动（moved）到 match 表达式中
        let (status, error_response) = match self {
            // ----------------------------------------------------------------------------
            // JsonRpc 错误 - 无效的 JSON-RPC 请求
            // ----------------------------------------------------------------------------
            PortalError::JsonRpc(message) => {
                let error = JsonRpcError {
                    code: -32600,  // JSON-RPC: Invalid Request
                    message,       // 使用错误消息
                    data: None,
                };
                (StatusCode::BAD_REQUEST, error)
            }
            // ----------------------------------------------------------------------------
            // MethodNotFound 错误 - 请求的方法不存在
            // ----------------------------------------------------------------------------
            PortalError::MethodNotFound(message) => {
                let error = JsonRpcError {
                    code: -32601,  // JSON-RPC: Method not found
                    message,
                    data: None,
                };
                (StatusCode::NOT_FOUND, error)
            }
            // ----------------------------------------------------------------------------
            // Parse 错误 - JSON 解析失败
            // ----------------------------------------------------------------------------
            PortalError::Parse(message) => {
                let error = JsonRpcError {
                    code: -32700,  // JSON-RPC: Parse error
                    message,
                    data: None,
                };
                (StatusCode::BAD_REQUEST, error)
            }
            // ----------------------------------------------------------------------------
            // Internal 错误 - 服务器内部错误
            // ----------------------------------------------------------------------------
            PortalError::Internal(message) => {
                let error = JsonRpcError {
                    code: -32603,  // JSON-RPC: Internal error
                    message,
                    data: None,
                };
                (StatusCode::INTERNAL_SERVER_ERROR, error)
            }
        };

        // 将状态码和 JSON 响应转换为 Axum 的 Response 类型
        // Json(error_response) 将 JsonRpcError 序列化为 JSON
        // .into_response() 将元组转换为 Response
        (status, Json(error_response)).into_response()
    }
}
