//! # MCP（Model Context Protocol）协议模块
//!
//! 本模块实现了 Model Context Protocol (MCP) 协议，用于 AI 助手与微沙箱的交互。
//!
//! ## 什么是 MCP？
//!
//! MCP 是 Anthropic 定义的一种协议，基于 JSON-RPC 2.0，用于 AI 模型与外部工具和服务的交互。
//! 它定义了标准化的方法来：
//! - 发现可用工具（tools/list）
//! - 调用工具（tools/call）
//! - 获取提示模板（prompts/list, prompts/get）
//! - 初始化会话（initialize）
//!
//! ## MCP 架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        AI 助手 (Claude 等)                        │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              │ MCP 协议
//!                              │ (基于 JSON-RPC 2.0)
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    microsandbox-server                           │
//! │  ┌─────────────────────────────────────────────────────────┐    │
//! │  │                      /mcp 端点                            │    │
//! │  │                                                           │    │
//! │  │  ┌─────────────────────────────────────────────────────┐ │    │
//! │  │  │              mcp_handler()                           │ │    │
//! │  │  │                                                       │ │    │
//! │  │  │  方法路由：                                           │ │    │
//! │  │  │  • initialize → handle_mcp_initialize()              │ │    │
//! │  │  │  • tools/list → handle_mcp_list_tools()              │ │    │
//! │  │  │  • tools/call → handle_mcp_call_tool()               │ │    │
//! │  │  │  • prompts/list → handle_mcp_list_prompts()          │ │    │
//! │  │  │  • prompts/get → handle_mcp_get_prompt()             │ │    │
//! │  │  │  • notifications/initialized → ...                   │ │    │
//! │  │  └─────────────────────────────────────────────────────┘ │    │
//! │  └─────────────────────────────────────────────────────────┘    │
//! │                              │                                   │
//! │                              ▼                                   │
//! │  ┌─────────────────────────────────────────────────────────┐    │
//! │  │                  handler.rs                              │    │
//! │  │  • sandbox_start_impl()                                  │    │
//! │  │  • sandbox_stop_impl()                                   │    │
//! │  │  • forward_rpc_to_portal()                               │    │
//! │  └─────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 支持的工具
//!
//! | 工具名 | 功能 | 对应 JSON-RPC 方法 |
//! |--------|------|-------------------|
//! | `sandbox_start` | 启动沙箱 | `sandbox.start` |
//! | `sandbox_stop` | 停止沙箱 | `sandbox.stop` |
//! | `sandbox_run_code` | 执行代码 | `sandbox.repl.run` |
//! | `sandbox_run_command` | 执行命令 | `sandbox.command.run` |
//! | `sandbox_get_metrics` | 获取指标 | `sandbox.metrics.get` |
//!
//! ## 支持的提示模板
//!
//! | 提示名 | 功能 |
//! |--------|------|
//! | `create_python_sandbox` | 创建 Python 沙箱的提示 |
//! | `create_node_sandbox` | 创建 Node.js 沙箱的提示 |
//!
//! ## MCP 消息格式
//!
//! ### 初始化请求
//! ```json
//! {
//!     "jsonrpc": "2.0",
//!     "method": "initialize",
//!     "params": {...},
//!     "id": 1
//! }
//! ```
//!
//! ### 初始化响应
//! ```json
//! {
//!     "jsonrpc": "2.0",
//!     "result": {
//!         "protocolVersion": "2024-11-05",
//!         "capabilities": {...},
//!         "serverInfo": {
//!             "name": "microsandbox-server",
//!             "version": "0.1.0"
//!         }
//!     },
//!     "id": 1
//! }
//! ```
//!
//! ### 工具调用
//! ```json
//! {
//!     "jsonrpc": "2.0",
//!     "method": "tools/call",
//!     "params": {
//!         "name": "sandbox_start",
//!         "arguments": {
//!             "sandbox": "my-sandbox",
//!             "config": {...}
//!         }
//!     },
//!     "id": 2
//! }
//! ```

use serde_json::json;
use tracing::{debug, error};

use crate::{
    ServerResult,
    error::ServerError,
    handler::{
        forward_rpc_to_portal, sandbox_get_metrics_impl, sandbox_start_impl, sandbox_stop_impl,
    },
    payload::{
        JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
        JsonRpcResponseOrNotification, ProcessedNotification, SandboxMetricsGetParams,
        SandboxStartParams, SandboxStopParams,
    },
    state::AppState,
};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// MCP 协议版本
///
/// 遵循 MCP 规范定义的版本格式（日期格式）
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// 服务器信息
const SERVER_NAME: &str = "microsandbox-server";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

//--------------------------------------------------------------------------------------------------
// 函数定义：处理器
//--------------------------------------------------------------------------------------------------

/// # 处理 MCP 初始化请求
///
/// 这是 MCP 会话的第一个请求，用于协商协议版本和能力。
///
/// ## MCP 初始化流程
///
/// 1. 客户端发送 `initialize` 请求
/// 2. 服务器返回协议版本和支持的能力
/// 3. 客户端发送 `notifications/initialized` 通知
/// 4. 开始正常的方法调用
///
/// ## 返回的能力
///
/// ### tools
/// - `listChanged: false` - 工具列表不会动态变化
///
/// ### prompts
/// - `listChanged: false` - 提示列表不会动态变化
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `_state` | `AppState` | 应用状态（当前未使用） |
/// | `request` | `JsonRpcRequest` | 初始化请求 |
///
/// ## 返回值
///
/// 包含服务器信息和能力的 JSON-RPC 响应
pub async fn handle_mcp_initialize(
    _state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<JsonRpcResponse> {
    debug!("处理 MCP 初始化请求");

    // 构建初始化响应
    let result = json!({
        "protocolVersion": MCP_PROTOCOL_VERSION,
        "capabilities": {
            "tools": {
                "listChanged": false  // 工具列表固定，不会动态变化
            },
            "prompts": {
                "listChanged": false  // 提示列表固定，不会动态变化
            }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": SERVER_VERSION
        }
    });

    Ok(JsonRpcResponse::success(result, request.id))
}

/// # 处理 MCP 列出工具请求
///
/// 返回服务器支持的所有工具及其 schema 定义。
///
/// ## 工具 Schema 结构
///
/// 每个工具定义包含：
/// - `name`: 工具名称
/// - `description`: 详细描述（AI 会阅读此描述来决定是否使用工具）
/// - `inputSchema`: JSON Schema 定义输入参数格式
///
/// ## 返回值
///
/// 包含工具列表的 JSON-RPC 响应
pub async fn handle_mcp_list_tools(
    _state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<JsonRpcResponse> {
    debug!("处理 MCP 列出工具请求");

    // 定义所有可用工具
    let tools = json!({
        "tools": [
            {
                "name": "sandbox_start",
                "description": "启动一个新的沙箱，具有指定的配置。这会创建一个隔离的代码执行环境。重要提示：完成后务必停止沙箱，防止其无限运行并消耗资源。支持的镜像：仅支持 'microsandbox/python'（用于 Python 代码）和 'microsandbox/node'（用于 Node.js 代码）。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sandbox": {
                            "type": "string",
                            "description": "要启动的沙箱名称"
                        },
                        "config": {
                            "type": "object",
                            "description": "沙箱配置",
                            "properties": {
                                "image": {
                                    "type": "string",
                                    "description": "要使用的 Docker 镜像。仅支持 'microsandbox/python' 和 'microsandbox/node'。",
                                    "enum": ["microsandbox/python", "microsandbox/node"]
                                },
                                "memory": {
                                    "type": "integer",
                                    "description": "内存限制（MiB）"
                                },
                                "cpus": {
                                    "type": "integer",
                                    "description": "CPU 核心数"
                                },
                                "volumes": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "卷挂载"
                                },
                                "ports": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "端口映射"
                                },
                                "envs": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "description": "环境变量"
                                }
                            }
                        }
                    },
                    "required": ["sandbox"]
                }
            },
            {
                "name": "sandbox_stop",
                "description": "停止正在运行的沙箱并清理其资源。关键提示：完成后务必调用此方法，防止资源泄漏和沙箱无限运行。不停止沙箱会导致它们不必要地消耗系统资源。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sandbox": {
                            "type": "string",
                            "description": "要停止的沙箱名称"
                        }
                    },
                    "required": ["sandbox"]
                }
            },
            {
                "name": "sandbox_run_code",
                "description": "在运行中的沙箱中执行代码。前提条件：目标沙箱必须已使用 sandbox_start 启动 - 如果沙箱未运行，此调用将失败。时序说明：代码执行是同步的，可能需要时间，具体取决于代码复杂度。长时间运行的代码会阻塞直到完成或超时。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sandbox": {
                            "type": "string",
                            "description": "沙箱名称（必须已启动）"
                        },
                        "code": {
                            "type": "string",
                            "description": "要执行的代码"
                        },
                        "language": {
                            "type": "string",
                            "description": "编程语言（如 'python'、'nodejs'）"
                        }
                    },
                    "required": ["sandbox", "code", "language"]
                }
            },
            {
                "name": "sandbox_run_command",
                "description": "在运行中的沙箱中执行 shell 命令。前提条件：目标沙箱必须已使用 sandbox_start 启动 - 如果沙箱未运行，此调用将失败。时序说明：命令执行是同步的，可能需要时间，具体取决于命令复杂度。长时间运行的命令会阻塞直到完成或超时。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sandbox": {
                            "type": "string",
                            "description": "沙箱名称（必须已启动）"
                        },
                        "command": {
                            "type": "string",
                            "description": "要执行的命令"
                        },
                        "args": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "命令参数"
                        }
                    },
                    "required": ["sandbox", "command"]
                }
            },
            {
                "name": "sandbox_get_metrics",
                "description": "获取沙箱的指标和状态，包括 CPU 使用率、内存消耗和运行状态。此工具可以检查任何沙箱的状态，无论其是否正在运行。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "sandbox": {
                            "type": "string",
                            "description": "可选的特定沙箱名称，用于获取其指标"
                        }
                    },
                    "required": []
                }
            }
        ]
    });

    Ok(JsonRpcResponse::success(tools, request.id))
}

/// # 处理 MCP 列出提示请求
///
/// 返回所有可用的提示模板。提示是预定义的指令，用于帮助 AI 助手
/// 执行常见任务。
pub async fn handle_mcp_list_prompts(
    _state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<JsonRpcResponse> {
    debug!("处理 MCP 列出提示请求");

    let prompts = json!({
        "prompts": [
            {
                "name": "create_python_sandbox",
                "description": "创建一个 Python 开发沙箱",
                "arguments": [
                    {
                        "name": "sandbox_name",
                        "description": "新沙箱的名称",
                        "required": true
                    }
                ]
            },
            {
                "name": "create_node_sandbox",
                "description": "创建一个 Node.js 开发沙箱",
                "arguments": [
                    {
                        "name": "sandbox_name",
                        "description": "新沙箱的名称",
                        "required": true
                    }
                ]
            }
        ]
    });

    Ok(JsonRpcResponse::success(prompts, request.id))
}

/// # 处理 MCP 获取提示请求
///
/// 返回指定提示模板的详细内容，包括描述和消息。
///
/// ## 参数验证
///
/// 此函数会验证：
/// 1. 请求参数是对象类型
/// 2. 包含必需的 `name` 字段
/// 3. 提示名称存在
pub async fn handle_mcp_get_prompt(
    _state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<JsonRpcResponse> {
    debug!("处理 MCP 获取提示请求");

    // 验证参数是对象类型
    let params = request.params.as_object().ok_or_else(|| {
        ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
            "请求参数必须是对象类型".to_string(),
        ))
    })?;

    // 提取提示名称
    let prompt_name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
        ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
            "缺少必需的 'name' 参数".to_string(),
        ))
    })?;

    // 获取参数（如果有）
    let arguments = params.get("arguments").and_then(|v| v.as_object());

    // 根据提示名称返回相应的内容
    let result = match prompt_name {
        "create_python_sandbox" => {
            // 提取 sandbox_name 参数，使用默认值
            let sandbox_name = arguments
                .and_then(|args| args.get("sandbox_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("python-sandbox");

            json!({
                "description": "创建一个 Python 开发沙箱",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "使用 sandbox_start 工具创建一个名为 '{}' 的 Python 沙箱，配置如下：\n\n\
                                - 镜像：microsandbox/python\n\
                                - 内存：512 MiB\n\
                                - CPU：1 核心\n\
                                - 工作目录：/workspace\n\n\
                                这将设置一个准备好进行代码执行的 Python 开发环境。",
                                sandbox_name
                            )
                        }
                    }
                ]
            })
        }
        "create_node_sandbox" => {
            // 提取 sandbox_name 参数，使用默认值
            let sandbox_name = arguments
                .and_then(|args| args.get("sandbox_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("node-sandbox");

            json!({
                "description": "创建一个 Node.js 开发沙箱",
                "messages": [
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "使用 sandbox_start 工具创建一个名为 '{}' 的 Node.js 沙箱，配置如下：\n\n\
                                - 镜像：microsandbox/node\n\
                                - 内存：512 MiB\n\
                                - CPU：1 核心\n\
                                - 工作目录：/workspace\n\n\
                                这将设置一个准备好进行 JavaScript 执行的 Node.js 开发环境。",
                                sandbox_name
                            )
                        }
                    }
                ]
            })
        }
        _ => {
            return Err(ServerError::NotFound(format!(
                "提示 '{}' 不存在",
                prompt_name
            )));
        }
    };

    Ok(JsonRpcResponse::success(result, request.id))
}

/// # 处理 MCP 调用工具请求
///
/// 这是 MCP 协议的核心方法，用于实际执行工具调用。
///
/// ## 工具路由
///
/// | MCP 工具名 | 内部 JSON-RPC 方法 | 处理方式 |
/// |------------|-------------------|----------|
/// | `sandbox_start` | `sandbox.start` | 本地处理 |
/// | `sandbox_stop` | `sandbox.stop` | 本地处理 |
/// | `sandbox_get_metrics` | `sandbox.metrics.get` | 本地处理 |
/// | `sandbox_run_code` | `sandbox.repl.run` | 转发到 Portal |
/// | `sandbox_run_command` | `sandbox.command.run` | 转发到 Portal |
///
/// ## 实现说明
///
/// 此函数将 MCP 工具调用转换为内部的 JSON-RPC 方法调用，
/// 然后复用现有的处理逻辑。
pub async fn handle_mcp_call_tool(
    state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<JsonRpcResponse> {
    debug!("处理 MCP 调用工具请求");

    // 验证参数是对象类型
    let params = request.params.as_object().ok_or_else(|| {
        ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
            "请求参数必须是对象类型".to_string(),
        ))
    })?;

    // 提取工具名称
    let tool_name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
        ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
            "缺少必需的 'name' 参数".to_string(),
        ))
    })?;

    // 提取参数
    let arguments = params.get("arguments").ok_or_else(|| {
        ServerError::ValidationError(crate::error::ValidationError::InvalidInput(
            "缺少必需的 'arguments' 参数".to_string(),
        ))
    })?;

    // ==================== 1. MCP 工具名到内部方法名的映射 ====================
    let internal_method = match tool_name {
        "sandbox_start" => "sandbox.start",
        "sandbox_stop" => "sandbox.stop",
        "sandbox_run_code" => "sandbox.repl.run",
        "sandbox_run_command" => "sandbox.command.run",
        "sandbox_get_metrics" => "sandbox.metrics.get",
        _ => {
            return Err(ServerError::NotFound(format!(
                "工具 '{}' 不存在",
                tool_name
            )));
        }
    };

    // ==================== 2. 创建内部 JSON-RPC 请求 ====================
    let internal_request = JsonRpcRequest {
        jsonrpc: JSONRPC_VERSION.to_string(),
        method: internal_method.to_string(),
        params: arguments.clone(),
        id: request.id.clone(),
    };

    // ==================== 3. 处理请求 ====================
    // 需要转发到 Portal 的方法
    let internal_response = if matches!(internal_method, "sandbox.repl.run" | "sandbox.command.run")
    {
        // 转发到 Portal
        match forward_rpc_to_portal(state, internal_request).await {
            Ok((_, json_response)) => json_response.0,
            Err(e) => {
                error!("转发请求到 Portal 失败：{}", e);
                return Ok(JsonRpcResponse::error(
                    JsonRpcError {
                        code: -32603,
                        message: format!("内部错误：{}", e),
                        data: None,
                    },
                    request.id,
                ));
            }
        }
    } else {
        // 本地处理的方法
        match internal_method {
            "sandbox.start" => {
                // 解析参数
                let params: SandboxStartParams = serde_json::from_value(arguments.clone())
                    .map_err(|e| {
                        JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32602,
                                message: format!("参数无效：{}", e),
                                data: None,
                            },
                            request.id.clone(),
                        )
                    })
                    .unwrap();

                // 调用实现函数
                match sandbox_start_impl(state, params).await {
                    Ok(result) => JsonRpcResponse::success(json!(result), request.id.clone()),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32603,
                            message: format!("沙箱启动失败：{}", e),
                            data: None,
                        },
                        request.id.clone(),
                    ),
                }
            }
            "sandbox.stop" => {
                // 解析参数
                let params: SandboxStopParams = serde_json::from_value(arguments.clone())
                    .map_err(|e| {
                        JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32602,
                                message: format!("参数无效：{}", e),
                                data: None,
                            },
                            request.id.clone(),
                        )
                    })
                    .unwrap();

                // 调用实现函数
                match sandbox_stop_impl(state, params).await {
                    Ok(result) => JsonRpcResponse::success(json!(result), request.id.clone()),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32603,
                            message: format!("沙箱停止失败：{}", e),
                            data: None,
                        },
                        request.id.clone(),
                    ),
                }
            }
            "sandbox.metrics.get" => {
                // 解析参数
                let params: SandboxMetricsGetParams = serde_json::from_value(arguments.clone())
                    .map_err(|e| {
                        JsonRpcResponse::error(
                            JsonRpcError {
                                code: -32602,
                                message: format!("参数无效：{}", e),
                                data: None,
                            },
                            request.id.clone(),
                        )
                    })
                    .unwrap();

                // 调用实现函数
                match sandbox_get_metrics_impl(state, params).await {
                    Ok(result) => JsonRpcResponse::success(json!(result), request.id.clone()),
                    Err(e) => JsonRpcResponse::error(
                        JsonRpcError {
                            code: -32603,
                            message: format!("获取指标失败：{}", e),
                            data: None,
                        },
                        request.id.clone(),
                    ),
                }
            }
            _ => JsonRpcResponse::error(
                JsonRpcError {
                    code: -32601,
                    message: format!("方法不存在：{}", internal_method),
                    data: None,
                },
                request.id.clone(),
            ),
        }
    };

    // ==================== 4. 转换为 MCP 响应格式 ====================
    // MCP 期望特定格式的结果
    let mcp_result = if let Some(result) = internal_response.result {
        // 成功：将结果包装为文本内容
        json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string())
                }
            ]
        })
    } else if let Some(error) = internal_response.error {
        // 失败：将错误包装为文本内容，并标记 isError
        json!({
            "content": [
                {
                    "type": "text",
                    "text": format!("错误：{}", error.message)
                }
            ],
            "isError": true
        })
    } else {
        // 无结果
        json!({
            "content": [
                {
                    "type": "text",
                    "text": "未返回结果"
                }
            ]
        })
    };

    Ok(JsonRpcResponse::success(mcp_result, request.id))
}

/// # 处理 MCP 初始化完成通知
///
/// 客户端在收到初始化响应后发送此通知，表示它已完成初始化
/// 并准备好进行正常的方法调用。
///
/// 注意：这是一个通知，不需要返回 JSON-RPC 响应。
pub async fn handle_mcp_notifications_initialized(
    _state: AppState,
    _request: JsonRpcRequest,
) -> ServerResult<ProcessedNotification> {
    debug!("处理 MCP 初始化完成通知");

    // 这是一个通知，不需要返回响应
    // 返回 ProcessedNotification 标记表示已处理
    Ok(ProcessedNotification::processed())
}

/// # 处理 MCP 方法路由
///
/// 此函数根据方法名将 MCP 请求路由到相应的处理器。
///
/// ## 支持的方法
///
/// | 方法 | 处理器 |
/// |------|--------|
/// | `initialize` | `handle_mcp_initialize` |
/// | `tools/list` | `handle_mcp_list_tools` |
/// | `tools/call` | `handle_mcp_call_tool` |
/// | `prompts/list` | `handle_mcp_list_prompts` |
/// | `prompts/get` | `handle_mcp_get_prompt` |
/// | `notifications/initialized` | `handle_mcp_notifications_initialized` |
pub async fn handle_mcp_method(
    state: AppState,
    request: JsonRpcRequest,
) -> ServerResult<JsonRpcResponseOrNotification> {
    match request.method.as_str() {
        "initialize" => {
            let response = handle_mcp_initialize(state, request).await?;
            Ok(JsonRpcResponseOrNotification::response(response))
        }
        "tools/list" => {
            let response = handle_mcp_list_tools(state, request).await?;
            Ok(JsonRpcResponseOrNotification::response(response))
        }
        "tools/call" => {
            let response = handle_mcp_call_tool(state, request).await?;
            Ok(JsonRpcResponseOrNotification::response(response))
        }
        "prompts/list" => {
            let response = handle_mcp_list_prompts(state, request).await?;
            Ok(JsonRpcResponseOrNotification::response(response))
        }
        "prompts/get" => {
            let response = handle_mcp_get_prompt(state, request).await?;
            Ok(JsonRpcResponseOrNotification::response(response))
        }
        "notifications/initialized" => {
            let notification = handle_mcp_notifications_initialized(state, request).await?;
            Ok(JsonRpcResponseOrNotification::notification(notification))
        }
        _ => Err(ServerError::NotFound(format!(
            "MCP 方法 '{}' 不存在",
            request.method
        ))),
    }
}
