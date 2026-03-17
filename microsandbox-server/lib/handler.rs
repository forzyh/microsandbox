//! # 请求处理器模块 - HTTP 请求处理逻辑
//!
//! 本模块实现了微沙箱服务器的所有 HTTP 请求处理器。
//!
//! ## 模块结构
//!
//! ```text
//! handler.rs
//! ├── REST API 处理器
//! │   └── health() - 健康检查
//! │
//! ├── JSON-RPC 处理器
//! │   ├── mcp_handler() - MCP 协议请求处理
//! │   ├── json_rpc_handler() - JSON-RPC 请求分发
//! │   └── forward_rpc_to_portal() - 转发 RPC 到 Portal
//! │
//! ├── 沙箱操作实现
//! │   ├── sandbox_start_impl() - 启动沙箱
//! │   ├── sandbox_stop_impl() - 停止沙箱
//! │   ├── sandbox_get_metrics_impl() - 获取指标
//! │   └── poll_sandbox_until_running() - 轮询沙箱状态
//! │
//! ├── 代理处理器
//! │   ├── proxy_request() - 代理请求
//! │   └── proxy_fallback() - 代理回退
//! │
//! └── 辅助函数
//!     └── validate_sandbox_name() - 验证沙箱名称
//! ```
//!
//! ## 请求处理流程
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      HTTP 请求                                   │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        中间件层                                  │
//! │  • logging_middleware: 记录请求日志                               │
//! │  • auth_middleware: JWT 认证验证                                  │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         路由器                                   │
//! │  /api/v1/health → health()                                      │
//! │  /api/v1/rpc    → json_rpc_handler()                            │
//! │  /mcp           → mcp_handler()                                 │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!              ┌───────────────┼───────────────┐
//!              ▼               ▼               ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │  health()       │ │json_rpc_handler │ │  mcp_handler()  │
//! │  返回健康状态    │ │  分发到具体方法   │ │  处理 MCP 协议    │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//!                              │
//!              ┌───────────────┼───────────────┐
//!              ▼               ▼               ▼
//!     sandbox.start    sandbox.repl.run   (MCP 方法)
//!     sandbox.stop     sandbox.command.run
//!     sandbox.metrics.get
//! ```
//!
//! ## JSON-RPC 方法分类
//!
//! ### 本地处理方法
//! 这些方法在服务器本地处理：
//! - `sandbox.start`: 启动沙箱
//! - `sandbox.stop`: 停止沙箱
//! - `sandbox.metrics.get`: 获取沙箱指标
//!
//! ### 转发到 Portal 的方法
//! 这些方法需要转发到沙箱内的 Portal 服务：
//! - `sandbox.repl.run`: 执行 REPL 代码
//! - `sandbox.command.run`: 执行 shell 命令

use axum::{
    Json,
    body::Body,
    debug_handler,  // 调试处理器宏，提供更详细的错误信息
    extract::{Path, State},
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use microsandbox_core::management::{menv, orchestra};
use microsandbox_utils::{DEFAULT_CONFIG, DEFAULT_PORTAL_GUEST_PORT, MICROSANDBOX_CONFIG_FILENAME};
use reqwest;
use serde_json::{self, json};
use serde_yaml;
use std::path::{Path as StdPath, PathBuf};
use tokio::{
    fs as tokio_fs,
    time::{Duration, sleep, timeout},
};
use tracing::{debug, trace, warn};

use crate::{
    SandboxStatus, SandboxStatusResponse, ServerResult,
    error::ServerError,
    mcp, middleware,
    payload::{
        JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
        JsonRpcResponseOrNotification, RegularMessageResponse, SandboxMetricsGetParams,
        SandboxStartParams, SandboxStopParams,
    },
    state::AppState,
};

//--------------------------------------------------------------------------------------------------
// 函数定义：REST API 处理器
//--------------------------------------------------------------------------------------------------

/// # 健康检查处理器
///
/// 用于负载均衡器、监控系统检查服务器健康状态。
///
/// ## 响应示例
///
/// ```json
/// HTTP 200 OK
/// {
///     "message": "Service is healthy"
/// }
/// ```
///
/// ## 返回值
///
/// - `Ok((StatusCode::OK, Json(...)))`: 服务健康
/// - `Err(ServerError)`: 服务异常（实际上此函数不会返回错误）
pub async fn health() -> ServerResult<impl IntoResponse> {
    Ok((
        StatusCode::OK,
        Json(RegularMessageResponse {
            message: "服务运行正常".to_string(),
        }),
    ))
}

//--------------------------------------------------------------------------------------------------
// 函数定义：JSON-RPC 处理器
//--------------------------------------------------------------------------------------------------

/// # MCP（Model Context Protocol）请求处理器
///
/// 处理所有发送到 `/mcp` 端点的 MCP 协议请求。
///
/// ## MCP 协议简介
///
/// MCP 是 Anthropic 定义的协议，用于 AI 助手与外部工具的交互。
/// 本质上是 JSON-RPC 2.0 的特定应用，定义了标准化的方法和参数。
///
/// ## 支持的 MCP 方法
///
/// | 方法 | 功能 |
/// |------|------|
/// | `initialize` | MCP 初始化握手 |
/// | `tools/list` | 列出可用工具 |
/// | `tools/call` | 调用工具 |
/// | `prompts/list` | 列出提示模板 |
/// | `prompts/get` | 获取提示模板 |
/// | `notifications/initialized` | 初始化完成通知 |
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `State<AppState>` | 应用状态（通过 Axum 提取器注入） |
/// | `request` | `Json<JsonRpcRequest>` | JSON-RPC 请求体 |
///
/// ## 返回值
///
/// - `Ok(JsonRpcResponseOrNotification)`: MCP 响应
/// - `Err(ServerError)`: 处理失败
///
/// ## `#[debug_handler]` 宏说明
///
/// 这是 Axum 提供的调试宏，为处理器函数添加更详细的编译时检查
/// 和错误信息，便于开发调试。
#[debug_handler]
pub async fn mcp_handler(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> ServerResult<impl IntoResponse> {
    debug!(?request, "收到 MCP 请求");

    // 验证 JSON-RPC 版本字段
    if request.jsonrpc != JSONRPC_VERSION {
        let error = JsonRpcError {
            code: -32600,  // JSON-RPC 标准错误码：无效请求
            message: "jsonrpc 版本字段无效或缺失".to_string(),
            data: None,
        };
        return Ok(JsonRpcResponseOrNotification::error(
            error,
            request.id.clone(),
        ));
    }

    // 提取请求 ID（在移动 request 之前）
    let request_id = request.id.clone();

    // 调用 MCP 模块处理方法
    match mcp::handle_mcp_method(state, request).await {
        Ok(response) => {
            // 枚举自动处理响应和通知两种情况
            Ok(response)
        }
        Err(e) => {
            let error = JsonRpcError {
                code: -32603,  // JSON-RPC 标准错误码：内部错误
                message: format!("MCP 方法错误：{}", e),
                data: None,
            };
            Ok(JsonRpcResponseOrNotification::error(error, request_id))
        }
    }
}

/// # JSON-RPC 主处理器
///
/// 这是 `/api/v1/rpc` 端点的主入口，负责分发所有 JSON-RPC 请求到具体的处理方法。
///
/// ## 方法路由
///
/// ```text
/// json_rpc_handler
/// │
/// ├─ sandbox.start → sandbox_start_impl()
/// ├─ sandbox.stop → sandbox_stop_impl()
/// ├─ sandbox.metrics.get → sandbox_get_metrics_impl()
/// ├─ sandbox.repl.run → forward_rpc_to_portal()
/// ├─ sandbox.command.run → forward_rpc_to_portal()
/// └─ 其他方法 → 返回"方法不存在"错误
/// ```
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `State<AppState>` | 应用状态 |
/// | `request` | `Json<JsonRpcRequest>` | JSON-RPC 请求 |
///
/// ## 返回值
///
/// 返回 Axum 响应，包含：
/// - HTTP 状态码
/// - JSON-RPC 响应体
#[debug_handler]
pub async fn json_rpc_handler(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> ServerResult<impl IntoResponse> {
    debug!(?request, "收到 JSON-RPC 请求");

    // 验证 JSON-RPC 版本字段
    if request.jsonrpc != JSONRPC_VERSION {
        let error = JsonRpcError {
            code: -32600,
            message: "jsonrpc 版本字段无效或缺失".to_string(),
            data: None,
        };
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(JsonRpcResponse::error(error, request.id.clone())),
        ));
    }

    // 提取方法名和请求 ID
    let method = request.method.as_str();
    let id = request.id.clone();

    // 根据方法名分发到不同的处理器
    match method {
        // ==================== 沙箱管理方法 ====================
        "sandbox.start" => {
            // 解析参数为 SandboxStartParams
            let start_params: SandboxStartParams =
                serde_json::from_value(request.params.clone()).map_err(|e| {
                    ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
                        format!("sandbox.start 参数无效：{}", e),
                    ))
                })?;

            // 调用启动沙箱的实现函数
            let result = sandbox_start_impl(state, start_params).await?;

            // 创建成功的 JSON-RPC 响应
            Ok((
                StatusCode::OK,
                Json(JsonRpcResponse::success(json!(result), id)),
            ))
        }
        "sandbox.stop" => {
            // 解析参数为 SandboxStopParams
            let stop_params: SandboxStopParams = serde_json::from_value(request.params.clone())
                .map_err(|e| {
                    ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
                        format!("sandbox.stop 参数无效：{}", e),
                    ))
                })?;

            // 调用停止沙箱的实现函数
            let result = sandbox_stop_impl(state, stop_params).await?;

            // 创建成功的 JSON-RPC 响应
            Ok((
                StatusCode::OK,
                Json(JsonRpcResponse::success(json!(result), id)),
            ))
        }
        "sandbox.metrics.get" => {
            // 解析参数为 SandboxMetricsGetParams
            let metrics_params: SandboxMetricsGetParams =
                serde_json::from_value(request.params.clone()).map_err(|e| {
                    ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
                        format!("sandbox.metrics.get 参数无效：{}", e),
                    ))
                })?;

            // 调用获取指标的实现函数
            let result = sandbox_get_metrics_impl(state.clone(), metrics_params).await?;

            // 创建成功的 JSON-RPC 响应
            Ok((
                StatusCode::OK,
                Json(JsonRpcResponse::success(json!(result), id)),
            ))
        }

        // ==================== 转发到 Portal 的方法 ====================
        // 这些方法需要转发到沙箱内部的 Portal 服务处理
        "sandbox.repl.run" | "sandbox.command.run" => {
            // 转发 RPC 到 Portal
            match forward_rpc_to_portal(state, request).await {
                Ok((status, json_response)) => Ok((status, json_response)),
                Err(e) => Err(e),
            }
        }

        // ==================== 未找到方法 ====================
        _ => {
            let error = JsonRpcError {
                code: -32601,  // JSON-RPC 标准错误码：方法不存在
                message: format!("方法不存在：{}", method),
                data: None,
            };
            Ok((
                StatusCode::NOT_FOUND,
                Json(JsonRpcResponse::error(error, id)),
            ))
        }
    }
}

/// # 转发 JSON-RPC 请求到 Portal 服务
///
/// 此函数将特定的 RPC 请求（如代码执行、命令执行）转发到沙箱内部的 Portal 服务。
///
/// ## Portal 架构
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                    外部客户端                                    │
/// └─────────────────────────────────────────────────────────────────┘
///                              │
///                              ▼
/// ┌─────────────────────────────────────────────────────────────────┐
/// │              microsandbox-server                                 │
/// │  ┌─────────────────────────────────────────────────────────┐    │
/// │  │                  forward_rpc_to_portal()                 │    │
/// │  │                                                           │    │
/// │  │  1. 从请求提取沙箱名称                                     │    │
/// │  │  2. 获取沙箱的 Portal URL（从 PortManager）                │    │
/// │  │  3. 重试连接 Portal（最多 300 次）                          │    │
/// │  │  4. 转发请求到 Portal                                      │    │
/// │  │  5. 返回 Portal 响应                                       │    │
/// │  └─────────────────────────────────────────────────────────┘    │
/// └─────────────────────────────────────────────────────────────────┘
///                              │
///                              ▼
/// ┌─────────────────────────────────────────────────────────────────┐
/// │                    Sandbox (Docker)                              │
/// │  ┌─────────────────────────────────────────────────────────┐    │
/// │  │                   Portal 服务                             │    │
/// │  │  • 接收 RPC 请求                                          │    │
/// │  │  • 执行代码/命令                                          │    │
/// │  │  • 返回执行结果                                           │    │
/// │  └─────────────────────────────────────────────────────────┘    │
/// └─────────────────────────────────────────────────────────────────┘
/// ```
///
/// ## 重试机制
///
/// Portal 服务启动需要时间，因此实现了重试逻辑：
/// - 最大重试次数：300 次
/// - 每次超时：50ms
/// - 重试间隔：10ms
/// - 总超时时间：约 30 秒
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `AppState` | 应用状态（用于获取 Portal URL） |
/// | `request` | `JsonRpcRequest` | 要转发的 JSON-RPC 请求 |
///
/// ## 返回值
///
/// - `Ok((StatusCode, Json<JsonRpcResponse>))`: Portal 响应
/// - `Err(ServerError)`: 转发失败
pub async fn forward_rpc_to_portal(
    state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<(StatusCode, Json<JsonRpcResponse>)> {
    // ==================== 1. 提取沙箱名称 ====================
    // 从请求参数中提取 sandbox 字段
    let sandbox_name = if let Some(params) = request.params.as_object() {
        params
            .get("sandbox")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
                    "Portal 请求缺少必需的 'sandbox' 参数".to_string(),
                ))
            })?
    } else {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput(
                "请求参数必须是包含 'sandbox' 的对象".to_string(),
            ),
        ));
    };

    // ==================== 2. 获取 Portal URL ====================
    // 从状态中获取沙箱的 Portal URL
    let portal_url = state.get_portal_url_for_sandbox(sandbox_name).await?;

    // 构建完整的 RPC 端点 URL 和健康检查 URL
    let portal_rpc_url = format!("{}/api/v1/rpc", portal_url);
    let portal_health_url = format!("{}/health", portal_url);

    debug!("转发 RPC 到 Portal: {}", portal_rpc_url);

    // ==================== 3. 创建 HTTP 客户端 ====================
    let client = reqwest::Client::new();

    // ==================== 4. 配置重试参数 ====================
    // Portal 启动需要时间，需要重试连接
    const MAX_RETRIES: u32 = 300;  // 最大重试次数
    const TIMEOUT_MS: u64 = 50;    // 每次请求超时（毫秒）
    const RETRY_DELAY_MS: u64 = 10; // 重试间隔（毫秒）

    // ==================== 5. 重试连接 Portal ====================
    let mut retry_count = 0;
    let mut last_error = None;

    // 循环尝试连接，直到成功或达到最大重试次数
    while retry_count < MAX_RETRIES {
        // 使用 HEAD 请求检查 Portal 健康状态
        match client
            .head(&portal_health_url)
            .timeout(Duration::from_millis(TIMEOUT_MS))
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                if status == reqwest::StatusCode::OK {
                    // Portal 就绪
                    debug!(
                        "在 {} 次重试后成功连接到 Portal (状态：{})",
                        retry_count, status
                    );
                    break;
                } else if status == reqwest::StatusCode::SERVICE_UNAVAILABLE {
                    // Portal 尚未就绪
                    last_error = Some(format!("Portal 尚未就绪 (状态：{})", status));
                    trace!("Portal 未就绪 (第 {} 次尝试)，重试中...", retry_count + 1);
                } else {
                    // 其他错误状态
                    last_error = Some(format!("Portal 返回错误状态：{}", status));
                    trace!("Portal 连接尝试 {} 返回 {}，重试中...", retry_count + 1, status);
                }
            }
            Err(e) => {
                // 连接失败，继续重试
                last_error = Some(e.to_string());
                trace!("连接尝试 {} 失败，重试中...", retry_count + 1);
            }
        }

        // 增加重试计数
        retry_count += 1;

        // 等待一段时间后再次尝试
        sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
    }

    // ==================== 6. 检查连接结果 ====================
    if retry_count >= MAX_RETRIES {
        // 达到最大重试次数，返回错误
        let error_msg = if let Some(e) = last_error {
            format!("在 {} 次重试后无法连接到 Portal: {}", MAX_RETRIES, e)
        } else {
            format!("在 {} 次重试后无法连接到 Portal", MAX_RETRIES)
        };
        return Err(ServerError::InternalError(error_msg));
    }

    // ==================== 7. 转发请求到 Portal ====================
    let response = client
        .post(&portal_rpc_url)
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            ServerError::InternalError(format!("转发 RPC 到 Portal 失败：{}", e))
        })?;

    // ==================== 8. 检查响应状态 ====================
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "未知错误".to_string());

        return Err(ServerError::InternalError(format!(
            "Portal 返回错误状态 {}: {}",
            status, error_text
        )));
    }

    // ==================== 9. 解析并返回 Portal 响应 ====================
    let portal_response: JsonRpcResponse = response.json().await.map_err(|e| {
        ServerError::InternalError(format!("解析 Portal 响应失败：{}", e))
    })?;

    // 直接返回 Portal 的响应
    Ok((StatusCode::OK, Json(portal_response)))
}

/// # 启动沙箱的实现函数
///
/// 此函数执行启动沙箱的完整流程，包括配置管理、端口分配和沙箱启动。
///
/// ## 启动流程
///
/// ```text
/// 1. 验证沙箱名称
///    │
///    ▼
/// 2. 检查/创建项目目录
///    │
///    ▼
/// 3. 加载或创建配置文件 (microsandbox.yaml)
///    │
///    ├─ 有现有配置 → 读取并验证
///    │
///    └─ 无配置 → 创建默认配置
///       │
///       ▼
/// 4. 更新沙箱配置（如果请求中提供了 config）
///    │
///    ▼
/// 5. 分配端口（从 PortManager）
///    │
///    ▼
/// 6. 更新配置中的端口映射
///    │
///    ▼
/// 7. 保存配置文件
///    │
///    ▼
/// 8. 启动沙箱（orchestra::up）
///    │
///    ▼
/// 9. 轮询等待沙箱运行
///    │
///    ├─ 首次拉取镜像 → 超时 180 秒
///    │
///    └─ 常规启动 → 超时 60 秒
///       │
///       ▼
/// 10. 返回结果
/// ```
///
/// ## 配置管理
///
/// 沙箱配置保存在 `~/.microsandbox/projects/microsandbox.yaml`：
///
/// ```yaml
/// sandboxes:
///   my-python-sandbox:
///     image: microsandbox/python
///     memory: 512
///     cpus: 1
///     ports:
///       - "54321:8080"  # 动态分配的 Portal 端口
/// ```
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `AppState` | 应用状态 |
/// | `params` | `SandboxStartParams` | 启动参数 |
///
/// ## 返回值
///
/// - `Ok(String)`: 成功消息
/// - `Err(ServerError)`: 启动失败
pub async fn sandbox_start_impl(
    state: AppState,
    params: SandboxStartParams,
) -> ServerResult<String> {
    // ==================== 1. 验证沙箱名称 ====================
    validate_sandbox_name(&params.sandbox)?;

    // ==================== 2. 准备路径和配置 ====================
    let project_dir = state.get_config().get_project_dir().clone();
    let config_file = MICROSANDBOX_CONFIG_FILENAME;
    let config_path = project_dir.join(config_file);
    let sandbox = &params.sandbox;

    // ==================== 3. 创建项目目录 ====================
    if !project_dir.exists() {
        tokio_fs::create_dir_all(&project_dir).await.map_err(|e| {
            ServerError::InternalError(format!("创建项目目录失败：{}", e))
        })?;

        // 初始化微沙箱环境
        menv::initialize(Some(project_dir.clone()))
            .await
            .map_err(|e| {
                ServerError::InternalError(format!(
                    "初始化微沙箱环境失败：{}",
                    e
                ))
            })?;
    }

    // ==================== 4. 检查配置来源 ====================
    // 检查请求中是否提供了配置（特别是 image 字段）
    let has_config_in_request = params
        .config
        .as_ref()
        .and_then(|c| c.image.as_ref())
        .is_some();
    // 检查是否存在现有配置文件
    let has_existing_config_file = config_path.exists();

    // 如果没有提供任何配置，返回错误
    if !has_config_in_request && !has_existing_config_file {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput(format!(
                "未提供配置，且未找到沙箱 '{}' 的现有配置",
                sandbox
            )),
        ));
    }

    // ==================== 5. 加载或创建配置 ====================
    let mut config_yaml: serde_yaml::Value;

    if has_existing_config_file {
        // 读取现有配置
        let config_content = tokio_fs::read_to_string(&config_path).await.map_err(|e| {
            ServerError::InternalError(format!("读取配置文件失败：{}", e))
        })?;

        // 解析 YAML
        config_yaml = serde_yaml::from_str(&config_content).map_err(|e| {
            ServerError::InternalError(format!("解析配置文件失败：{}", e))
        })?;

        // 如果依赖现有配置，验证沙箱是否存在于配置中
        if !has_config_in_request {
            let has_sandbox_config = config_yaml
                .get("sandboxes")
                .and_then(|sandboxes| sandboxes.get(sandbox))
                .is_some();

            if !has_sandbox_config {
                return Err(ServerError::ValidationError(
                    crate::error::ValidationError::InvalidInput(format!(
                        "在现有配置中未找到沙箱 '{}'",
                        sandbox
                    )),
                ));
            }
        }
    } else {
        // 创建新配置
        if !has_config_in_request {
            return Err(ServerError::ValidationError(
                crate::error::ValidationError::InvalidInput(
                    "未提供配置，且不存在配置文件".to_string(),
                ),
            ));
        }

        // 写入默认配置
        tokio_fs::write(&config_path, DEFAULT_CONFIG)
            .await
            .map_err(|e| {
                ServerError::InternalError(format!("创建配置文件失败：{}", e))
            })?;

        // 解析默认配置
        config_yaml = serde_yaml::from_str(DEFAULT_CONFIG).map_err(|e| {
            ServerError::InternalError(format!("解析默认配置失败：{}", e))
        })?;
    }

    // ==================== 6. 确保 sandboxes 字段存在 ====================
    if !config_yaml.is_mapping() {
        config_yaml = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }

    let config_map = config_yaml.as_mapping_mut().unwrap();
    if !config_map.contains_key(serde_yaml::Value::String("sandboxes".to_string())) {
        config_map.insert(
            serde_yaml::Value::String("sandboxes".to_string()),
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        );
    }

    // 获取 sandboxes 映射
    let sandboxes_key = serde_yaml::Value::String("sandboxes".to_string());
    let sandboxes_value = config_map.get_mut(&sandboxes_key).unwrap();

    // 确保 sandboxes 值是映射类型
    if !sandboxes_value.is_mapping() {
        *sandboxes_value = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }

    let sandboxes_map = sandboxes_value.as_mapping_mut().unwrap();

    // ==================== 7. 更新沙箱配置（如果提供了 config） ====================
    if let Some(config) = &params.config
        && config.image.is_some()
    {
        // 创建或更新沙箱条目
        let mut sandbox_map = serde_yaml::Mapping::new();

        // 设置必需的 image 字段
        if let Some(image) = &config.image {
            sandbox_map.insert(
                serde_yaml::Value::String("image".to_string()),
                serde_yaml::Value::String(image.clone()),
            );
        }

        // 设置可选字段
        if let Some(memory) = config.memory {
            sandbox_map.insert(
                serde_yaml::Value::String("memory".to_string()),
                serde_yaml::Value::Number(serde_yaml::Number::from(memory)),
            );
        }

        if let Some(cpus) = config.cpus {
            sandbox_map.insert(
                serde_yaml::Value::String("cpus".to_string()),
                serde_yaml::Value::Number(serde_yaml::Number::from(cpus)),
            );
        }

        if !config.volumes.is_empty() {
            let volumes_array = config
                .volumes
                .iter()
                .map(|v| serde_yaml::Value::String(v.clone()))
                .collect::<Vec<_>>();
            sandbox_map.insert(
                serde_yaml::Value::String("volumes".to_string()),
                serde_yaml::Value::Sequence(volumes_array),
            );
        }

        if !config.ports.is_empty() {
            let ports_array = config
                .ports
                .iter()
                .map(|p| serde_yaml::Value::String(p.clone()))
                .collect::<Vec<_>>();
            sandbox_map.insert(
                serde_yaml::Value::String("ports".to_string()),
                serde_yaml::Value::Sequence(ports_array),
            );
        }

        if !config.envs.is_empty() {
            let envs_array = config
                .envs
                .iter()
                .map(|e| serde_yaml::Value::String(e.clone()))
                .collect::<Vec<_>>();
            sandbox_map.insert(
                serde_yaml::Value::String("envs".to_string()),
                serde_yaml::Value::Sequence(envs_array),
            );
        }

        if !config.depends_on.is_empty() {
            let depends_on_array = config
                .depends_on
                .iter()
                .map(|d| serde_yaml::Value::String(d.clone()))
                .collect::<Vec<_>>();
            sandbox_map.insert(
                serde_yaml::Value::String("depends_on".to_string()),
                serde_yaml::Value::Sequence(depends_on_array),
            );
        }

        if let Some(workdir) = &config.workdir {
            sandbox_map.insert(
                serde_yaml::Value::String("workdir".to_string()),
                serde_yaml::Value::String(workdir.clone()),
            );
        }

        if let Some(shell) = &config.shell {
            sandbox_map.insert(
                serde_yaml::Value::String("shell".to_string()),
                serde_yaml::Value::String(shell.clone()),
            );
        }

        if !config.scripts.is_empty() {
            let mut scripts_map = serde_yaml::Mapping::new();
            for (script_name, script) in &config.scripts {
                scripts_map.insert(
                    serde_yaml::Value::String(script_name.clone()),
                    serde_yaml::Value::String(script.clone()),
                );
            }
            sandbox_map.insert(
                serde_yaml::Value::String("scripts".to_string()),
                serde_yaml::Value::Mapping(scripts_map),
            );
        }

        if let Some(exec) = &config.exec {
            sandbox_map.insert(
                serde_yaml::Value::String("exec".to_string()),
                serde_yaml::Value::String(exec.clone()),
            );
        }

        // 替换或添加沙箱配置
        sandboxes_map.insert(
            serde_yaml::Value::String(sandbox.clone()),
            serde_yaml::Value::Mapping(sandbox_map),
        );
    }

    // ==================== 8. 分配端口 ====================
    let sandbox_key = params.sandbox.clone();
    let port = {
        // 获取写锁，分配端口
        let mut port_manager = state.get_port_manager().write().await;
        port_manager.assign_port(&sandbox_key).await.map_err(|e| {
            ServerError::InternalError(format!("分配 Portal 端口失败：{}", e))
        })?
    };

    debug!("为沙箱 {} 分配 Portal 端口 {}", sandbox_key, port);

    // ==================== 9. 更新端口映射 ====================
    // 获取沙箱配置
    let sandbox_config = sandboxes_map
        .get_mut(serde_yaml::Value::String(sandbox.clone()))
        .ok_or_else(|| {
            ServerError::InternalError(format!("配置中未找到沙箱 '{}'", sandbox))
        })?
        .as_mapping_mut()
        .ok_or_else(|| {
            ServerError::InternalError(format!(
                "沙箱 '{}' 配置不是映射类型",
                sandbox
            ))
        })?;

    // 添加或更新端口映射
    let guest_port = DEFAULT_PORTAL_GUEST_PORT;  // Portal 在容器内的端口（通常 8080）
    let portal_port_mapping = format!("{}:{}", port, guest_port);  // host_port:guest_port

    let ports_key = serde_yaml::Value::String("ports".to_string());

    if let Some(ports) = sandbox_config.get_mut(&ports_key) {
        if let Some(ports_seq) = ports.as_sequence_mut() {
            // 过滤掉现有的 Portal 端口映射
            ports_seq.retain(|p| {
                p.as_str()
                    .map(|s| !s.ends_with(&format!(":{}", guest_port)))
                    .unwrap_or(true)
            });

            // 添加新的端口映射
            ports_seq.push(serde_yaml::Value::String(portal_port_mapping));
        }
    } else {
        // 创建新的 ports 列表
        let ports_seq = vec![serde_yaml::Value::String(portal_port_mapping)];
        sandbox_config.insert(ports_key, serde_yaml::Value::Sequence(ports_seq));
    }

    // ==================== 10. 保存配置 ====================
    let updated_config = serde_yaml::to_string(&config_yaml)
        .map_err(|e| ServerError::InternalError(format!("序列化配置失败：{}", e)))?;

    tokio_fs::write(&config_path, updated_config)
        .await
        .map_err(|e| ServerError::InternalError(format!("写入配置文件失败：{}", e)))?;

    // ==================== 11. 启动沙箱 ====================
    orchestra::up(
        vec![sandbox.clone()],
        Some(&project_dir),
        Some(config_file),
        true,  // 等待沙箱启动
    )
    .await
    .map_err(|e| {
        ServerError::InternalError(format!("启动沙箱 {} 失败：{}", params.sandbox, e))
    })?;

    // ==================== 12. 确定超时时间 ====================
    // 判断是否是首次拉取镜像（需要更长超时）
    let potentially_first_time_pull = if let Some(config) = &params.config {
        config.image.is_some()
    } else {
        false
    };

    // 设置超时时间
    let poll_timeout = if potentially_first_time_pull {
        Duration::from_secs(180)  // 首次拉取镜像：3 分钟
    } else {
        Duration::from_secs(60)   // 常规启动：1 分钟
    };

    // ==================== 13. 轮询等待沙箱运行 ====================
    debug!("等待沙箱 {} 启动...", sandbox);
    match timeout(
        poll_timeout,
        poll_sandbox_until_running(&params.sandbox, &project_dir, config_file),
    )
    .await
    {
        Ok(result) => match result {
            Ok(_) => {
                debug!("沙箱 {} 正在运行", sandbox);
                Ok(format!("沙箱 {} 启动成功", params.sandbox))
            }
            Err(e) => {
                // 沙箱已启动但轮询失败
                warn!("无法验证沙箱 {} 是否运行：{}", sandbox, e);
                Ok(format!(
                    "沙箱 {} 已启动，但无法验证运行状态：{}",
                    params.sandbox, e
                ))
            }
        },
        Err(_) => {
            // 超时，但沙箱可能仍在启动中
            warn!("等待沙箱 {} 启动超时", sandbox);
            Ok(format!(
                "沙箱 {} 已启动，但等待运行状态超时。可能仍在初始化中。",
                params.sandbox
            ))
        }
    }
}

/// # 轮询沙箱直到运行状态
///
/// 此函数定期检查沙箱状态，直到确认沙箱正在运行。
///
/// ## 轮询参数
///
/// - 轮询间隔：20ms
/// - 最大尝试次数：2500 次（约 50 秒）
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `sandbox_name` | `&str` | 沙箱名称 |
/// | `project_dir` | `&StdPath` | 项目目录 |
/// | `config_file` | `&str` | 配置文件名 |
///
/// ## 返回值
///
/// - `Ok(())`: 沙箱正在运行
/// - `Err(ServerError)`: 超过最大尝试次数
async fn poll_sandbox_until_running(
    sandbox_name: &str,
    project_dir: &StdPath,
    config_file: &str,
) -> ServerResult<()> {
    const POLL_INTERVAL: Duration = Duration::from_millis(20);
    const MAX_ATTEMPTS: usize = 2500;

    for attempt in 1..=MAX_ATTEMPTS {
        // 检查沙箱状态
        let statuses = orchestra::status(
            vec![sandbox_name.to_string()],
            Some(project_dir),
            Some(config_file),
        )
        .await
        .map_err(|e| ServerError::InternalError(format!("获取沙箱状态失败：{}", e)))?;

        // 查找目标沙箱
        if let Some(status) = statuses.iter().find(|s| s.name == sandbox_name)
            && status.running
        {
            // 沙箱正在运行
            debug!(
                "沙箱 {} 正在运行（第 {} 次尝试确认）",
                sandbox_name, attempt
            );
            return Ok(());
        }

        // 等待后再次尝试
        sleep(POLL_INTERVAL).await;
    }

    // 超过最大尝试次数
    Err(ServerError::InternalError(format!(
        "验证沙箱 {} 运行状态超过最大尝试次数",
        sandbox_name
    )))
}

/// # 停止沙箱的实现函数
///
/// 此函数执行停止沙箱的完整流程，包括沙箱停止和端口释放。
///
/// ## 停止流程
///
/// ```text
/// 1. 验证沙箱名称
///    │
///    ▼
/// 2. 检查项目目录是否存在
///    │
///    ▼
/// 3. 检查配置文件是否存在
///    │
///    ▼
/// 4. 停止沙箱（orchestra::down）
///    │
///    ▼
/// 5. 释放端口（PortManager）
///    │
///    ▼
/// 6. 返回成功消息
/// ```
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `AppState` | 应用状态 |
/// | `params` | `SandboxStopParams` | 停止参数 |
///
/// ## 返回值
///
/// - `Ok(String)`: 成功消息
/// - `Err(ServerError)`: 停止失败
pub async fn sandbox_stop_impl(state: AppState, params: SandboxStopParams) -> ServerResult<String> {
    // ==================== 1. 验证沙箱名称 ====================
    validate_sandbox_name(&params.sandbox)?;

    // ==================== 2. 准备路径 ====================
    let project_dir = state.get_config().get_project_dir().clone();
    let config_file = MICROSANDBOX_CONFIG_FILENAME;
    let sandbox = &params.sandbox;
    let sandbox_key = params.sandbox.clone();

    // ==================== 3. 检查项目目录 ====================
    if !project_dir.exists() {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput(
                "项目目录不存在".to_string(),
            ),
        ));
    }

    // ==================== 4. 检查配置文件 ====================
    let config_path = project_dir.join(config_file);
    if !config_path.exists() {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput("配置文件不存在".to_string()),
        ));
    }

    // ==================== 5. 停止沙箱 ====================
    orchestra::down(vec![sandbox.clone()], Some(&project_dir), Some(config_file))
        .await
        .map_err(|e| {
            ServerError::InternalError(format!("停止沙箱 {} 失败：{}", params.sandbox, e))
        })?;

    // ==================== 6. 释放端口 ====================
    {
        let mut port_manager = state.get_port_manager().write().await;
        port_manager.release_port(&sandbox_key).await.map_err(|e| {
            ServerError::InternalError(format!("释放 Portal 端口失败：{}", e))
        })?;
    }

    debug!("已释放沙箱 {} 的 Portal 端口", sandbox_key);

    // ==================== 7. 返回成功消息 ====================
    Ok(format!("沙箱 {} 停止成功", params.sandbox))
}

/// # 获取沙箱指标的实现函数
///
/// 此函数获取沙箱的运行状态和资源使用情况。
///
/// ## 指标类型
///
/// | 指标 | 类型 | 说明 |
/// |------|------|------|
/// | `running` | `bool` | 是否正在运行 |
/// | `cpu_usage` | `Option<f32>` | CPU 使用率（百分比） |
/// | `memory_usage` | `Option<u64>` | 内存使用量（字节） |
/// | `disk_usage` | `Option<u64>` | 磁盘使用量（字节） |
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `state` | `AppState` | 应用状态 |
/// | `params` | `SandboxMetricsGetParams` | 指标查询参数 |
///
/// ## 返回值
///
/// - `Ok(SandboxStatusResponse)`: 沙箱状态列表
/// - `Err(ServerError)`: 获取失败
pub async fn sandbox_get_metrics_impl(
    state: AppState,
    params: SandboxMetricsGetParams,
) -> ServerResult<SandboxStatusResponse> {
    // ==================== 1. 验证沙箱名称（如果提供） ====================
    if let Some(sandbox) = &params.sandbox {
        validate_sandbox_name(sandbox)?;
    }

    // ==================== 2. 获取项目目录 ====================
    let project_dir = state.get_config().get_project_dir().clone();

    // 检查项目目录是否存在
    if !project_dir.exists() {
        return Err(ServerError::InternalError(format!(
            "项目目录 '{}' 不存在",
            project_dir.display()
        )));
    }

    // ==================== 3. 确定要查询的沙箱列表 ====================
    let sandbox_names = if let Some(sandbox) = &params.sandbox {
        vec![sandbox.clone()]  // 只查询指定沙箱
    } else {
        vec![]  // 查询所有沙箱
    };

    // ==================== 4. 获取沙箱状态 ====================
    let mut all_statuses = Vec::new();

    match orchestra::status(sandbox_names, Some(&project_dir), None).await {
        Ok(statuses) => {
            // 转换状态格式
            for status in statuses {
                all_statuses.push(SandboxStatus {
                    name: status.name,
                    running: status.running,
                    cpu_usage: status.cpu_usage,
                    memory_usage: status.memory_usage,
                    disk_usage: status.disk_usage,
                });
            }
        }
        Err(e) => {
            return Err(ServerError::InternalError(format!(
                "获取指标时出错：{e}"
            )));
        }
    }

    Ok(SandboxStatusResponse {
        sandboxes: all_statuses,
    })
}

//--------------------------------------------------------------------------------------------------
// 函数定义：代理处理器
//--------------------------------------------------------------------------------------------------

/// # 代理请求处理器
///
/// 当前是演示实现，返回请求信息而不是实际转发。
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `_state` | `State<AppState>` | 应用状态（未使用） |
/// | `(sandbox, path)` | `Path<(String, PathBuf)>` | 沙箱名称和路径 |
/// | `req` | `Request<Body>` | HTTP 请求 |
pub async fn proxy_request(
    State(_state): State<AppState>,
    Path((sandbox, path)): Path<(String, PathBuf)>,
    req: Request<Body>,
) -> ServerResult<impl IntoResponse> {
    let path_str = path.display().to_string();

    // 计算目标 URI（演示用）
    let original_uri = req.uri().clone();
    let _target_uri = middleware::proxy_uri(original_uri, &sandbox);

    // 构建响应信息
    let response = format!(
        "Axum 代理请求\n\n沙箱：{}\n路径：{}\n方法：{}\n请求头：{:?}",
        sandbox,
        path_str,
        req.method(),
        req.headers()
    );

    let result = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "text/plain")
        .body(Body::from(response))
        .unwrap();

    Ok(result)
}

/// # 代理回退处理器
///
/// 当代理请求没有匹配到任何路由时返回 404。
pub async fn proxy_fallback() -> ServerResult<impl IntoResponse> {
    Ok((StatusCode::NOT_FOUND, "资源不存在"))
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// # 验证沙箱名称
///
/// 沙箱名称必须符合以下规则：
/// 1. 不能为空
/// 2. 长度不超过 63 字符
/// 3. 只能包含字母、数字、连字符（-）和下划线（_）
/// 4. 必须以字母或数字开头
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `name` | `&str` | 待验证的沙箱名称 |
///
/// ## 返回值
///
/// - `Ok(())`: 名称有效
/// - `Err(ServerError::ValidationError)`: 名称无效
fn validate_sandbox_name(name: &str) -> ServerResult<()> {
    // 检查是否为空
    if name.is_empty() {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput("沙箱名称不能为空".to_string()),
        ));
    }

    // 检查长度
    if name.len() > 63 {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput(
                "沙箱名称不能超过 63 个字符".to_string(),
            ),
        ));
    }

    // 检查字符合法性
    let valid_chars = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if !valid_chars {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput(
                "沙箱名称只能包含字母、数字、连字符或下划线".to_string(),
            ),
        ));
    }

    // 检查首字符
    if !name.chars().next().unwrap().is_ascii_alphanumeric() {
        return Err(ServerError::ValidationError(
            crate::error::ValidationError::InvalidInput(
                "沙箱名称必须以字母或数字开头".to_string(),
            ),
        ));
    }

    Ok(())
}
