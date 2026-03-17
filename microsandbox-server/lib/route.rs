//! # 路由模块 - HTTP 路由配置
//!
//! 本模块负责定义和配置微沙箱服务器的所有 HTTP 路由端点。
//!
//! ## API 端点概览
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        微沙箱服务器路由                          │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  /api/v1/health        GET     健康检查端点                      │
//! │                                                                 │
//! │  /api/v1/rpc           POST    JSON-RPC 接口                     │
//! │                          ├─ sandbox.start        启动沙箱       │
//! │                          ├─ sandbox.stop         停止沙箱       │
//! │                          ├─ sandbox.metrics.get  获取指标       │
//! │                          ├─ sandbox.repl.run     执行代码       │
//! │                          └─ sandbox.command.run  执行命令       │
//! │                                                                 │
//! │  /mcp                  POST    Model Context Protocol 接口       │
//! │                          ├─ initialize           初始化握手     │
//! │                          ├─ tools/list           列出工具       │
//! │                          ├─ tools/call           调用工具       │
//! │                          ├─ prompts/list         列出提示       │
//! │                          ├─ prompts/get          获取提示       │
//! │                          └─ notifications/initialized 初始化通知 │
//! │                                                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 路由层次结构
//!
//! ```text
//! Router (根路由)
//! │
//! ├─ 日志中间件 (logging_middleware) - 应用于所有请求
//! │
//! ├─ /api/v1 (REST API)
//! │   └─ /health (GET) - 健康检查
//! │
//! ├─ /api/v1/rpc (JSON-RPC)
//! │   ├─ 认证中间件 (auth_middleware) - 验证 JWT 令牌
//! │   └─ / (POST) - JSON-RPC 处理器
//! │
//! └─ /mcp (MCP 协议)
//!     ├─ 智能认证中间件 (mcp_smart_auth_middleware)
//!     └─ / (POST) - MCP 处理器
//! ```
//!
//! ## 中间件说明
//!
//! ### 日志中间件 (logging_middleware)
//! - **作用范围**: 所有路由
//! - **功能**: 记录请求方法、URI 和响应状态码
//! - **实现**: 在 `middleware.rs` 中定义
//!
//! ### 认证中间件 (auth_middleware)
//! - **作用范围**: `/api/v1/rpc` 路由
//! - **功能**: 验证 JWT 令牌，开发模式下跳过
//! - **实现**: 在 `middleware.rs` 中定义
//!
//! ### MCP 智能认证中间件 (mcp_smart_auth_middleware)
//! - **作用范围**: `/mcp` 路由
//! - **功能**: 与认证中间件类似，专为 MCP 协议设计
//! - **实现**: 在 `middleware.rs` 中定义

use axum::{
    Router, middleware,
    routing::{get, post},
};

// 导入处理器函数和中间件
// crate::handler: 请求处理器模块
// crate::middleware: 中间件模块
// crate::state::AppState: 应用状态类型
use crate::{handler, middleware as app_middleware, state::AppState};

//--------------------------------------------------------------------------------------------------
// 函数定义
//--------------------------------------------------------------------------------------------------

/// # 创建 HTTP 路由器
///
/// 此函数创建并配置服务器的根路由器，包含所有 API 端点和中间件。
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `AppState` | 应用状态，包含配置和端口管理器 |
///
/// ## 返回值
///
/// 返回配置好的 `Router` 实例，可用于启动 HTTP 服务器。
///
/// ## 路由详解
///
/// ### 1. REST API 路由 (`/api/v1`)
///
/// ```rust,ignore
/// let rest_api = Router::new()
///     .route("/health", get(handler::health));
/// ```
///
/// - **端点**: `GET /api/v1/health`
/// - **用途**: 健康检查，用于负载均衡器和监控系统
/// - **认证**: 不需要
/// - **响应示例**:
///   ```json
///   {
///       "message": "Service is healthy"
///   }
///   ```
///
/// ### 2. JSON-RPC 路由 (`/api/v1/rpc`)
///
/// ```rust,ignore
/// let rpc_api = Router::new()
///     .route("/", post(handler::json_rpc_handler))
///     .layer(middleware::from_fn_with_state(
///         state.clone(),
///         app_middleware::auth_middleware,
///     ));
/// ```
///
/// - **端点**: `POST /api/v1/rpc`
/// - **用途**: 处理所有 JSON-RPC 请求
/// - **认证**: 需要有效的 JWT 令牌
/// - **方法**:
///   - `sandbox.start`: 启动沙箱
///   - `sandbox.stop`: 停止沙箱
///   - `sandbox.metrics.get`: 获取沙箱指标
///   - `sandbox.repl.run`: 在沙箱中执行代码
///   - `sandbox.command.run`: 在沙箱中执行命令
///
/// ### 3. MCP 路由 (`/mcp`)
///
/// ```rust,ignore
/// let mcp_api = Router::new()
///     .route("/", post(handler::mcp_handler))
///     .layer(middleware::from_fn_with_state(
///         state.clone(),
///         app_middleware::mcp_smart_auth_middleware,
///     ));
/// ```
///
/// - **端点**: `POST /mcp`
/// - **用途**: 实现 Model Context Protocol
/// - **认证**: 需要有效的 JWT 令牌
/// - **方法**:
///   - `initialize`: MCP 初始化握手
///   - `tools/list`: 列出可用工具
///   - `tools/call`: 调用工具
///   - `prompts/list`: 列出提示模板
///   - `prompts/get`: 获取提示模板
///   - `notifications/initialized`: 初始化完成通知
///
/// ## Axum Router 概念解释
///
/// ### `.route(path, method_handler)`
/// 为特定路径和方法注册处理器。
/// ```rust,ignore
/// .route("/health", get(handler::health))
/// // 等同于：GET /health -> handler::health()
/// ```
///
/// ### `.layer(middleware)`
/// 为路由添加中间件层。
/// ```rust,ignore
/// .layer(middleware::from_fn_with_state(state, auth_middleware))
/// // 所有经过此路由的请求都会先通过 auth_middleware 处理
/// ```
///
/// ### `.nest(prefix, router)`
/// 将子路由嵌套到指定前缀下。
/// ```rust,ignore
/// .nest("/api/v1", rest_api)
/// // rest_api 中的 /health 变为 /api/v1/health
/// ```
///
/// ### `.with_state(state)`
/// 为路由器注入应用状态，处理器可通过 `State` 提取器访问。
/// ```rust,ignore
/// async fn handler(State(state): State<AppState>) { ... }
/// ```
///
/// ## 使用示例
///
/// ```rust,no_run
/// use microsandbox_server::{AppState, Config, PortManager, route};
/// use std::sync::Arc;
/// use tokio::sync::RwLock;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // 创建应用状态
/// let config = Arc::new(Config::new(
///     Some("key".to_string()),
///     "127.0.0.1".to_string(),
///     8080,
///     None,
///     true,
/// )?);
/// let port_manager = Arc::new(RwLock::new(
///     PortManager::new(config.get_project_dir()).await?
/// ));
/// let state = AppState::new(config, port_manager);
///
/// // 创建路由器
/// let router = route::create_router(state);
///
/// // 启动服务器
/// let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
/// axum::serve(listener, router).await?;
/// # Ok(())
/// # }
/// ```
pub fn create_router(state: AppState) -> Router {
    // ==================== 创建 REST API 路由 ====================
    // 只包含健康检查端点，其他功能都通过 JSON-RPC 提供
    let rest_api = Router::new().route("/health", get(handler::health));

    // ==================== 创建 JSON-RPC 路由 ====================
    // 单个端点处理所有 RPC 方法，通过 method 字段区分
    // 这种设计类似于 microsandbox-portal 的结构
    let rpc_api = Router::new()
        // 所有 POST 请求都发送到 json_rpc_handler
        .route("/", post(handler::json_rpc_handler))
        // 添加认证中间件层
        // from_fn_with_state 允许中间件访问应用状态
        .layer(middleware::from_fn_with_state(
            state.clone(),  // 克隆状态以便中间件使用
            app_middleware::auth_middleware,  // 认证中间件函数
        ));

    // ==================== 创建 MCP 路由 ====================
    // 单独的端点用于 Model Context Protocol
    // 使用智能认证中间件，对协议方法和工具方法有不同的处理策略
    let mcp_api =
        Router::new()
            // 所有 POST 请求都发送到 mcp_handler
            .route("/", post(handler::mcp_handler))
            // 添加 MCP 专用的认证中间件
            .layer(middleware::from_fn_with_state(
                state.clone(),
                app_middleware::mcp_smart_auth_middleware,
            ));

    // ==================== 组合所有路由 ====================
    // 使用 nest 将子路由挂载到指定前缀下
    // 最后添加日志中间件，记录所有请求
    Router::new()
        // 挂载 REST API 到 /api/v1 前缀下
        .nest("/api/v1", rest_api)
        // 挂载 JSON-RPC 到 /api/v1/rpc 前缀下
        .nest("/api/v1/rpc", rpc_api)
        // 挂载 MCP 到 /mcp 前缀下
        .nest("/mcp", mcp_api)
        // 添加日志中间件层（应用于所有路由）
        .layer(middleware::from_fn(app_middleware::logging_middleware))
        // 注入应用状态，使处理器可以访问
        .with_state(state)
}
