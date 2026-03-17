//! # 中间件模块 - 请求处理的中间层组件
//!
//! 本模块实现了 Axum 框架的中间件组件，用于在请求到达处理器之前或响应返回之前
//! 执行通用逻辑。
//!
//! ## 什么是中间件？
//!
//! 中间件是 Web 框架中的一种设计模式，它允许你在请求处理流程中插入通用的处理逻辑。
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         请求流程                                │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │   HTTP 请求 → 日志中间件 → 认证中间件 → 路由处理器 → 响应        │
//! │                 │              │              │                 │
//! │                 ▼              ▼              ▼                 │
//! │            记录请求日志    验证 JWT 令牌    业务逻辑处理          │
//! │                                                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 中间件类型
//!
//! ### 1. 日志中间件 (logging_middleware)
//! - **作用**: 记录所有请求的详细信息
//! - **执行时机**: 请求处理前和响应返回后
//! - **记录内容**: HTTP 方法、URI、响应状态码
//!
//! ### 2. 认证中间件 (auth_middleware)
//! - **作用**: 验证 JWT 令牌
//! - **执行时机**: 请求到达处理器之前
//! - **验证方式**: 从请求头提取令牌并验证
//!
//! ### 3. MCP 智能认证中间件 (mcp_smart_auth_middleware)
//! - **作用**: MCP 协议专用的认证
//! - **执行时机**: 请求到达 MCP 处理器之前
//! - **特点**: 与认证中间件类似，但专为 MCP 设计
//!
//! ### 4. 代理中间件 (proxy_middleware)
//! - **作用**: 请求转发（当前是占位实现）
//! - **扩展点**: 可以添加实际的代理逻辑
//!
//! ## Axum 中间件签名
//!
//! Axum 中间件是异步函数，具有以下签名：
//!
//! ```rust,ignore
//! async fn middleware(
//!     State(state): State<AppState>,  // 可选：应用状态
//!     req: Request<Body>,              // HTTP 请求
//!     next: Next,                      // 下一个处理器
//! ) -> Result<impl IntoResponse, Error> {
//!     // 前置处理
//!     let response = next.run(req).await;  // 调用下一个处理器
//!     // 后置处理
//!     Ok(response)
//! }
//! ```
//!
//! ## JWT 认证流程
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      JWT 认证流程                                │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  1. 客户端发送请求                                               │
//! │     Header: Proxy-Authorization: Bearer msb_<jwt_token>         │
//! │                                                                 │
//! │           │                                                      │
//! │           ▼                                                      │
//! │  2. 中间件提取令牌                                                │
//! │     - 检查 Proxy-Authorization 头                                │
//! │     - 检查 Authorization 头                                       │
//! │     - 去除 "Bearer " 前缀                                        │
//! │     - 去除 "msb_" 前缀，获取原始 JWT                              │
//! │                                                                 │
//! │           │                                                      │
//! │           ▼                                                      │
//! │  3. 验证 JWT 令牌                                                 │
//! │     - 使用服务器密钥验证签名 (HS256)                              │
//! │     - 检查令牌是否过期                                            │
//! │                                                                 │
//! │           │                                                      │
//! │           ▼                                                      │
//! │  4. 结果                                                         │
//! │     - 有效：继续处理请求                                          │
//! │     - 无效：返回 401 Unauthorized                                │
//! │                                                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode, Uri},
    middleware::Next,
    response::IntoResponse,
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

// 导入认证相关的类型
use crate::{
    Claims,  // JWT 令牌的声明结构
    config::PROXY_AUTH_HEADER,  // 代理认证请求头名称
    error::{AuthenticationError, ServerError},  // 错误类型
    management::API_KEY_PREFIX,  // API 密钥前缀 "msb_"
    state::AppState,  // 应用状态
};

//--------------------------------------------------------------------------------------------------
// 中间件函数
//--------------------------------------------------------------------------------------------------

/// # 代理中间件
///
/// 当前是一个占位实现，用于展示代理中间件的结构。
/// 可以扩展为实现实际的请求转发逻辑。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `_state` | `State<AppState>` | 应用状态（当前未使用） |
/// | `req` | `Request<Body>` | HTTP 请求 |
/// | `next` | `Next` | 下一个处理器 |
///
/// ## 返回值
///
/// 返回 `impl IntoResponse`，可以直接作为 HTTP 响应。
///
/// ## 扩展建议
///
/// 实际的代理中间件可以：
/// 1. 解析目标地址（从路径或请求头）
/// 2. 修改请求 URI
/// 3. 转发请求到目标服务
/// 4. 将响应返回给客户端
pub async fn proxy_middleware(
    State(_state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    // 默认直接将请求传递给下一个处理器
    // 此中间件可以扩展为实现实际的代理逻辑
    next.run(req).await
}

/// # URI 代理转换
///
/// 将原始 URI 转换为代理目标的 URI。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `original_uri` | `Uri` | 原始请求 URI |
/// | `sandbox_name` | `&str` | 目标沙箱名称 |
///
/// ## 返回值
///
/// 返回转换后的 `Uri`，指向沙箱的内部服务。
///
/// ## 实现说明
///
/// 这是一个演示实现，实际生产中应该：
/// 1. 从沙箱注册表查询实际地址
/// 2. 使用正确的端口
/// 3. 处理路径重写
///
/// 当前实现简单地将所有请求转发到 `sandbox-{name}.internal:8080`。
pub fn proxy_uri(original_uri: Uri, sandbox_name: &str) -> Uri {
    // 在实际实现中，应该：
    // 1. 从沙箱注册表或状态中查找沙箱地址
    // 2. 构建指向沙箱的新 URI
    // 3. 返回用于代理的新 URI

    // 演示目的：构建简单的 URI
    // 生产中应该从沙箱注册表获取
    let target_host = format!("sandbox-{}.internal", sandbox_name);

    // 构建完整 URI 字符串
    let uri_string = if let Some(path_and_query) = original_uri.path_and_query() {
        // 保留原始路径和查询参数
        format!("http://{}:{}{}", target_host, 8080, path_and_query)
    } else {
        // 没有路径和查询，使用根路径
        format!("http://{}:{}/", target_host, 8080)
    };

    // 尝试解析 URI 字符串
    // 如果解析失败，回退到默认的 localhost URI
    uri_string
        .parse()
        .unwrap_or_else(|_| "http://localhost:8080/".parse().unwrap())
}

/// # 日志中间件
///
/// 记录所有请求的详细信息，包括请求方法、URI 和响应状态码。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `req` | `Request<Body>` | HTTP 请求 |
/// | `next` | `Next` | 下一个处理器 |
///
/// ## 返回值
///
/// - `Ok(response)`: 请求处理成功，返回响应
/// - `Err((StatusCode, String))`: 处理失败（当前实现不会失败）
///
/// ## 日志示例
///
/// ```text
/// INFO Request: GET /api/v1/health
/// INFO Response: GET /api/v1/health: 200 OK
/// ```
///
/// ## 使用 tracing 库
///
/// 本中间件使用 `tracing` 库进行日志记录：
/// - `tracing::info!`: 记录信息级别日志
/// - 结构化日志：可以添加更多字段如请求 ID、用户 ID 等
pub async fn logging_middleware(
    req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // 克隆请求信息用于日志（因为请求会被移动）
    let method = req.method().clone();
    let uri = req.uri().clone();

    // 记录请求
    tracing::info!("Request: {} {}", method, uri);

    // 处理请求（调用下一个处理器）
    let response = next.run(req).await;

    // 记录响应状态码
    tracing::info!("Response: {} {}: {}", method, uri, response.status());

    Ok(response)
}

/// # 认证中间件
///
/// 验证请求中的 JWT 令牌，确保请求是经过授权的。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `State<AppState>` | 应用状态（用于获取配置） |
/// | `req` | `Request<Body>` | HTTP 请求 |
/// | `next` | `Next` | 下一个处理器 |
///
/// ## 返回值
///
/// - `Ok(response)`: 认证成功或开发模式，继续处理请求
/// - `Err(ServerError)`: 认证失败，返回 401 Unauthorized
///
/// ## 认证流程
///
/// 1. **开发模式检查**: 如果启用开发模式，跳过认证
/// 2. **提取令牌**: 从请求头中提取 API 密钥
/// 3. **格式转换**: 将 `msb_<jwt>` 转换为原始 JWT
/// 4. **验证令牌**: 使用服务器密钥验证 JWT
/// 5. **继续处理**: 验证通过，继续处理请求
///
/// ## 安全考虑
///
/// - 开发模式下跳过认证，仅用于本地调试
/// - 生产环境必须禁用开发模式
/// - 错误的认证信息不会暴露详细原因（防止信息泄露）
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, ServerError> {
    // 如果配置为开发模式，跳过认证
    // 开发模式用于本地调试，生产环境不应该使用
    if *state.get_config().get_dev_mode() {
        return Ok(next.run(req).await);
    }

    // 从请求头中提取 API 密钥
    let api_key = extract_api_key_from_headers(req.headers())?;

    // 验证令牌
    validate_token(&api_key, &state)?;

    // 验证通过，继续处理请求
    Ok(next.run(req).await)
}

/// # MCP 智能认证中间件
///
/// MCP（Model Context Protocol）专用的认证中间件。
/// 当前的实现与普通认证中间件相同，但保留用于未来扩展。
///
/// ## 与普通认证中间件的区别
///
/// 当前实现逻辑相同，但：
/// - 专用于 `/mcp` 端点
/// - 未来可以根据 MCP 方法类型采用不同的认证策略
/// - 协议方法（如 `initialize`）和工具方法（如 `tools/call`）可以有不同的处理
///
/// ## 参数和返回值
///
/// 与 `auth_middleware` 相同。
pub async fn mcp_smart_auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, ServerError> {
    // 如果配置为开发模式，跳过认证
    if *state.get_config().get_dev_mode() {
        return Ok(next.run(req).await);
    }

    // 从请求头中提取 API 密钥
    let api_key = extract_api_key_from_headers(req.headers())?;

    // 验证令牌
    validate_token(&api_key, &state)?;

    // 验证通过，继续处理请求
    Ok(next.run(req).await)
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// # 从请求头提取 API 密钥
///
/// 尝试从多个可能的请求头位置提取 API 密钥。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `headers` | `&HeaderMap` | HTTP 请求头 |
///
/// ## 返回值
///
/// - `Ok(String)`: 成功提取 API 密钥（不含前缀）
/// - `Err(ServerError::Authentication)`: 缺少或格式错误的认证头
///
/// ## 支持的请求头格式
///
/// 1. **Proxy-Authorization 头（优先）**
///    ```http
///    Proxy-Authorization: Bearer msb_<jwt_token>
///    ```
///    或
///    ```http
///    Proxy-Authorization: msb_<jwt_token>
///    ```
///
/// 2. **标准 Authorization 头（备选）**
///    ```http
///    Authorization: Bearer msb_<jwt_token>
///    ```
///    或
///    ```http
///    Authorization: msb_<jwt_token>
///    ```
///
/// ## 实现细节
///
/// 1. 首先检查 `Proxy-Authorization` 头
/// 2. 如果不存在，检查标准 `Authorization` 头
/// 3. 支持带或不带 `Bearer ` 前缀的格式
/// 4. 返回的令牌包含 `msb_` 前缀（后续由 `convert_api_key_to_jwt` 处理）
fn extract_api_key_from_headers(headers: &HeaderMap) -> Result<String, ServerError> {
    // 首先检查 Proxy-Authorization 头
    if let Some(auth_header) = headers.get(PROXY_AUTH_HEADER) {
        // 将头值转换为字符串
        let auth_value = auth_header.to_str().map_err(|_| {
            ServerError::Authentication(AuthenticationError::InvalidCredentials(
                "Invalid authorization header format".to_string(),
            ))
        })?;

        // 检查是否有 Bearer 前缀
        if let Some(token) = auth_value.strip_prefix("Bearer ") {
            return Ok(token.to_string());
        }

        // 或者是原始令牌
        return Ok(auth_value.to_string());
    }

    // 然后检查标准 Authorization 头
    if let Some(auth_header) = headers.get("Authorization") {
        let auth_value = auth_header.to_str().map_err(|_| {
            ServerError::Authentication(AuthenticationError::InvalidCredentials(
                "Invalid authorization header format".to_string(),
            ))
        })?;

        // 检查是否有 Bearer 前缀
        if let Some(token) = auth_value.strip_prefix("Bearer ") {
            return Ok(token.to_string());
        }

        // 或者是原始令牌
        return Ok(auth_value.to_string());
    }

    // 都没有找到，返回认证错误
    Err(ServerError::Authentication(
        AuthenticationError::InvalidCredentials("Missing authorization header".to_string()),
    ))
}

/// # 将 API 密钥转换为 JWT 格式
///
/// 将自定义的 API 密钥格式（`msb_<jwt>`）转换为标准 JWT 格式。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `api_key` | `&str` | API 密钥（格式：`msb_<jwt_token>`） |
///
/// ## 返回值
///
/// - `Ok(String)`: 原始 JWT 令牌（不含 `msb_` 前缀）
/// - `Err(ServerError::Authentication)`: 格式错误
///
/// ## API 密钥格式
///
/// 自定义 API 密钥格式的设计考虑：
/// - **前缀标识**: `msb_` 前缀便于识别密钥类型
/// - **安全性**: 与标准 JWT 兼容，保持安全性
/// - **易用性**: 前端可以轻松识别和处理
///
/// ```text
/// API 密钥：msb_eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...
///            └──┘└────────────────────────────────┘
///           前缀            原始 JWT
/// ```
fn convert_api_key_to_jwt(api_key: &str) -> Result<String, ServerError> {
    // 检查 API 密钥是否有预期的前缀
    if !api_key.starts_with(API_KEY_PREFIX) {
        return Err(ServerError::Authentication(
            AuthenticationError::InvalidCredentials(
                "Invalid API key format: missing prefix".to_string(),
            ),
        ));
    }

    // 移除前缀，返回原始 JWT
    // API_KEY_PREFIX.len() 是 "msb_" 的长度 (4)
    Ok(api_key[API_KEY_PREFIX.len()..].to_string())
}

/// # 从应用状态获取服务器密钥
///
/// 从 AppState 的配置中获取 JWT 签名密钥。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `&AppState` | 应用状态 |
///
/// ## 返回值
///
/// - `Ok(String)`: 服务器密钥
/// - `Err(ServerError::Authentication)`: 密钥未配置
///
/// ## 注意
///
/// 此函数假设调用者已经确认不在开发模式下。
/// 在开发模式下，服务器密钥可以是 `None`。
fn get_server_key(state: &AppState) -> Result<String, ServerError> {
    // 从配置中获取密钥
    // 此时已经确认不在开发模式下
    match state.get_config().get_key() {
        Some(key) => Ok(key.clone()),
        None => Err(ServerError::Authentication(
            AuthenticationError::InvalidCredentials(
                "Server key not found in configuration".to_string(),
            ),
        )),
    }
}

/// # 验证 JWT 令牌
///
/// 解码并验证 JWT 令牌的有效性。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `api_key` | `&str` | API 密钥（`msb_<jwt>` 格式） |
/// | `state` | `&AppState` | 应用状态 |
///
/// ## 返回值
///
/// - `Ok(Claims)`: 验证成功，返回令牌中的声明
/// - `Err(ServerError::Authentication)`: 验证失败
///
/// ## 验证步骤
///
/// 1. **格式转换**: 将 API 密钥转换为原始 JWT
/// 2. **获取密钥**: 从状态中获取服务器密钥
/// 3. **配置验证器**: 设置算法为 HS256
/// 4. **解码令牌**: 使用 `jsonwebtoken::decode`
/// 5. **错误处理**: 根据错误类型提供友好的错误消息
///
/// ## JWT 算法说明
///
/// 本实现使用 `HS256`（HMAC-SHA256）算法：
/// - **类型**: 对称加密（签名和验证使用相同密钥）
/// - **安全性**: 256 位 HMAC，足够安全
/// - **性能**: 比非对称加密快
///
/// ## 常见错误
///
/// | 错误类型 | 原因 | 返回消息 |
/// |----------|------|----------|
/// | `ExpiredSignature` | 令牌已过期 | "Token expired" |
/// | `InvalidSignature` | 签名不匹配 | "Invalid token signature" |
/// | 其他 | 各种验证失败 | "Token validation error: ..." |
fn validate_token(api_key: &str, state: &AppState) -> Result<Claims, ServerError> {
    // 将 API 密钥转换回 JWT 格式
    let jwt = convert_api_key_to_jwt(api_key)?;

    // 获取服务器密钥用于验证
    let server_key = get_server_key(state)?;

    // 解码并验证 JWT
    let token_data = decode::<Claims>(
        &jwt,
        &DecodingKey::from_secret(server_key.as_bytes()),  // 使用服务器密钥创建解码密钥
        &Validation::new(Algorithm::HS256),  // 使用 HS256 算法验证
    )
    .map_err(|e| {
        // 根据错误类型提供友好的错误消息
        let error_message = match e.kind() {
            // 令牌过期
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => "Token expired".to_string(),
            // 签名无效
            jsonwebtoken::errors::ErrorKind::InvalidSignature => {
                "Invalid token signature".to_string()
            }
            // 其他错误
            _ => format!("Token validation error: {}", e),
        };
        ServerError::Authentication(AuthenticationError::InvalidToken(error_message))
    })?;

    // 返回解析后的声明（包含 exp 和 iat 字段）
    Ok(token_data.claims)
}
