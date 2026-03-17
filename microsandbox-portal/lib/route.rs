//! # 路由模块 (Router Configuration)
//!
//! 本模块定义了 microsandbox portal 的 HTTP 路由配置。
//!
//! ## Axum 路由系统简介
//!
//! Axum 是 Tokio 团队开发的 Web 框架，基于 Tower 和 Hyper 构建。
//! 它的路由系统具有以下特点：
//! - **类型安全**: 路由在编译时检查
//! - **组合式**: 可以嵌套和组合路由
//! - **中间件支持**: 轻松添加日志、认证等中间件
//!
//! ## API 端点设计
//!
//! 本服务定义了以下端点：
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET | `/health` | 健康检查端点 |
//! | POST | `/api/v1/rpc/` | JSON-RPC 请求处理 |
//!
//! ## 路由层次结构
//!
//! ```text
//! /
//! ├── health (GET)              # 健康检查
//! └── api/v1/rpc/
//!     └── / (POST)              # JSON-RPC 入口
//! ```
//!
//! ## 版本化 API 设计
//!
//! 使用 `/api/v1/` 前缀是 REST API 版本化的常见做法：
//! - **好处**: 可以在不破坏现有客户端的情况下演进 API
//! - **示例**: 未来可以有 `/api/v2/` 提供不同的接口
//!
//! ## Tower HTTP 中间件
//!
//! 本模块使用 `tower-http` crate 提供的 `TraceLayer` 中间件：
//! - 自动记录所有 HTTP 请求的日志
//! - 包括方法、路径、状态码、响应时间等
//! - 便于调试和监控

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::trace::TraceLayer;

use crate::{handler, state::SharedState};

//--------------------------------------------------------------------------------------------------
// 函数 (Functions)
//--------------------------------------------------------------------------------------------------

/// # 创建路由配置
///
/// 此函数创建并配置应用程序的 Axum 路由器。
///
/// ## 路由配置流程
///
/// 1. **创建 JSON-RPC 子路由**: 定义处理 RPC 请求的路由
/// 2. **创建主路由**: 组合健康检查和 RPC 路由
/// 3. **添加中间件**: 添加日志追踪中间件
/// 4. **注入状态**: 将共享状态注入到路由器中
///
/// ## 参数说明
///
/// * `state: SharedState` - 共享状态，包含：
///   - 服务器就绪标志
///   - REPL 引擎句柄
///   - 命令执行器句柄
///
/// ## 返回值
///
/// 返回配置好的 `Router` 实例，可以直接传递给 Axum 服务器。
///
/// ## 代码解释
///
/// ### 路由构建器模式
///
/// ```rust
/// Router::new()
///     .route("/path", get(handler))  // 添加路由
///     .nest("/prefix", sub_router)   // 嵌套子路由
///     .layer(middleware)             // 添加中间件
///     .with_state(state)             // 注入状态
/// ```
///
/// ### 路由匹配顺序
///
/// Axum 按照添加的顺序匹配路由：
/// - 更具体的路由应该先添加
/// - 通配符路由应该后添加
///
/// ### 状态共享
///
/// `.with_state(state)` 将状态注入路由器：
/// - 所有处理函数可以通过 `State<T>` 提取器访问状态
/// - 状态在请求间共享（通过 Arc）
///
/// ## 使用示例
///
/// ```rust
/// let state = SharedState::default();
/// let app = create_router(state);
///
/// // 启动服务器
/// let listener = TcpListener::bind("0.0.0.0:4444").await?;
/// axum::serve(listener, app).await?;
/// ```
pub fn create_router(state: SharedState) -> Router {
    // ================================================================================
    // 步骤 1: 创建 JSON-RPC 子路由
    // ================================================================================
    // JSON-RPC 使用单一路由处理所有 RPC 方法
    // 具体的方法分发在 json_rpc_handler 中完成
    //
    // post(handler::json_rpc_handler) 指定 POST 方法和处理函数
    // Axum 自动将请求体反序列化为 JsonRpcRequest
    let rpc_api = Router::new().route("/", post(handler::json_rpc_handler));

    // ================================================================================
    // 步骤 2: 组合所有路由
    // ================================================================================
    // 使用链式调用组合路由：
    //
    // 1. .route("/health", get(handler::health_check_handler))
    //    - 添加 GET /health 端点
    //    - 用于健康检查和负载均衡器探测
    //
    // 2. .nest("/api/v1/rpc", rpc_api)
    //    - 将 rpc_api 嵌套在 /api/v1/rpc 路径下
    //    - 结果：POST /api/v1/rpc/ 映射到 json_rpc_handler
    //
    // 3. .layer(TraceLayer::new_for_http())
    //    - 添加 HTTP 追踪中间件
    //    - 自动记录所有请求的日志
    //
    // 4. .with_state(state)
    //    - 将共享状态注入路由器
    //    - 处理函数可以通过 State<SharedState> 访问
    Router::new()
        // 健康检查端点 - GET /health
        // 返回 200 OK 如果服务器就绪，503 Service Unavailable 否则
        .route("/health", get(handler::health_check_handler))

        // 嵌套 JSON-RPC 路由
        // 所有 RPC 请求都发送到 /api/v1/rpc/
        // json_rpc_handler 根据 method 字段分发到具体处理函数
        .nest("/api/v1/rpc", rpc_api)

        // 添加追踪中间件层
        // TraceLayer 自动记录：
        // - 请求方法、路径、版本
        // - 响应状态码
        // - 处理延迟
        .layer(TraceLayer::new_for_http())

        // 注入共享状态
        // 状态被所有路由的处理函数共享
        // 通过 Axum 的 State 提取器访问
        .with_state(state)
}
