//! # 请求处理模块 (Request Handlers)
//!
//! 本模块实现了 microsandbox portal JSON-RPC 服务器的核心请求处理逻辑。
//!
//! ## 主要功能
//!
//! 本模块负责处理来自客户端的 JSON-RPC 请求，包括：
//! - **健康检查** (`health_check_handler`): 验证服务器是否就绪
//! - **JSON-RPC 请求处理** (`json_rpc_handler`): 路由和处理各种 RPC 方法
//! - **沙箱 REPL 执行** (`sandbox_run_impl`): 在沙箱环境中执行代码
//! - **沙箱命令执行** (`sandbox_command_run_impl`): 执行系统命令
//!
//! ## JSON-RPC 协议支持的方法
//!
//! 1. `sandbox.repl.run` - 在 REPL 环境中执行代码（支持 Python、Node.js 等）
//! 2. `sandbox.command.run` - 执行系统命令
//!
//! ## 架构说明
//!
//! 本模块采用 Axum 框架作为 Web 服务器，使用以下核心组件：
//! - `axum::State`: 共享状态管理，用于在不同请求间共享引擎句柄
//! - `serde_json`: JSON 序列化和反序列化
//! - `tokio::sync::Mutex`: 异步互斥锁，保护共享的引擎资源
//!
//! ## 错误处理
//!
//! 所有错误都通过 `PortalError` 类型统一处理，并转换为符合 JSON-RPC 2.0 规范的错误响应。
//! 标准错误码包括：
//! - `-32700`: 解析错误 (Parse error)
//! - `-32600`: 无效请求 (Invalid Request)
//! - `-32601`: 方法未找到 (Method not found)
//! - `-32603`: 内部错误 (Internal error)

use std::sync::atomic::Ordering;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::{Value, json};
use tracing::debug;

use crate::{
    error::PortalError,
    payload::{
        JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse, SandboxCommandRunParams,
        SandboxReplRunParams,
    },
    portal::command::create_command_executor,
    state::SharedState,
};

#[cfg(any(feature = "python", feature = "nodejs"))]
use crate::portal::repl::{Language, start_engines};

//--------------------------------------------------------------------------------------------------
// 函数 (Functions)
//--------------------------------------------------------------------------------------------------

/// # 健康检查处理函数
///
/// 此函数用于验证 portal 服务器是否已准备好接收请求。
///
/// ## 工作原理
///
/// 健康检查端点通过检查共享状态中的 `ready` 标志来判断服务器状态：
/// - 如果 `ready` 为 `true`，返回 HTTP 200 OK
/// - 如果 `ready` 为 `false`，返回 HTTP 503 Service Unavailable
///
/// ## 参数说明
///
/// * `State(state)` - Axum 的状态提取器，用于获取共享的 `SharedState`
///   - `SharedState` 包含服务器的全局状态，包括就绪标志和引擎句柄
///   - 使用 `State(state)` 语法是 Axum 框架提取共享状态的标准方式
///
/// ## 返回值
///
/// * `Ok((StatusCode::OK, "OK"))` - 服务器已就绪
/// * `Ok((StatusCode::SERVICE_UNAVAILABLE, "Not ready"))` - 服务器未就绪
///
/// ## AtomicBool 说明
///
/// `ready` 字段是 `Arc<AtomicBool>` 类型：
/// - `Arc` (Atomic Reference Counting) 允许多个线程安全地共享数据
/// - `AtomicBool` 提供原子布尔操作，无需锁即可安全读写
/// - `Ordering::Acquire` 确保在此读取之前的所有写操作对当前线程可见
pub async fn health_check_handler(
    State(state): State<SharedState>,
) -> Result<impl IntoResponse, PortalError> {
    // 使用原子操作的 Acquire 顺序加载 ready 标志
    // Acquire 顺序确保我们能看到服务器初始化时设置 ready=true 之前的所有操作
    if state.ready.load(Ordering::Acquire) {
        // 服务器已就绪，返回 HTTP 200 OK
        Ok((StatusCode::OK, "OK"))
    } else {
        // 服务器仍在初始化中，返回 HTTP 503 Service Unavailable
        Ok((StatusCode::SERVICE_UNAVAILABLE, "Not ready"))
    }
}

/// # JSON-RPC 请求处理函数
///
/// 这是 JSON-RPC 服务器的核心入口点，负责接收和分发所有 RPC 请求。
///
/// ## 工作流程
///
/// 1. **接收请求**: 从 HTTP POST 请求体中解析 JSON-RPC 请求
/// 2. **版本验证**: 检查 `jsonrpc` 字段是否为 "2.0"
/// 3. **方法路由**: 根据 `method` 字段将请求分发到相应的处理函数
/// 4. **返回响应**: 构造符合 JSON-RPC 2.0 规范的响应
///
/// ## JSON-RPC 2.0 请求格式
///
/// ```json
/// {
///     "jsonrpc": "2.0",
///     "method": "sandbox.repl.run",
///     "params": { ... },
///     "id": 1
/// }
/// ```
///
/// ## 支持的方法
///
/// | 方法名 | 说明 |
/// |--------|------|
/// | `sandbox.repl.run` | 在 REPL 环境中执行代码 |
/// | `sandbox.command.run` | 执行系统命令 |
///
/// ## 参数说明
///
/// * `State(state)` - 共享状态，包含引擎句柄等资源
/// * `req: Json<JsonRpcRequest>` - 解析后的 JSON-RPC 请求对象
///   - `Json<T>` 是 Axum 的提取器，自动将请求体反序列化为 T 类型
///
/// ## 返回值
///
/// 返回 `Result<impl IntoResponse, PortalError>`：
/// - 成功时返回 HTTP 200 和 JSON-RPC 响应
/// - 失败时返回适当的 HTTP 状态码和 JSON-RPC 错误响应
pub async fn json_rpc_handler(
    State(state): State<SharedState>,
    req: Json<JsonRpcRequest>,
) -> Result<impl IntoResponse, PortalError> {
    // 从 Axum 的 Json 提取器中获取实际的请求对象
    // req.0 是 newtype 模式，Json 是一个 tuple struct，.0 获取内部值
    let request = req.0;
    // 使用 tracing  crate 记录调试日志
    // ?request 语法使用 Debug trait 格式化整个请求对象
    debug!(?request, "Received JSON-RPC request");

    // ================================================================================
    // 步骤 1: 验证 JSON-RPC 版本
    // ================================================================================
    // JSON-RPC 2.0 规范要求 jsonrpc 字段必须为 "2.0"
    // 这是强制性要求，不符合此要求的请求应被拒绝
    if request.jsonrpc != JSONRPC_VERSION {
        let error = JsonRpcError {
            code: -32600,  // JSON-RPC 标准错误码：Invalid Request
            message: "Invalid or missing jsonrpc version field".to_string(),
            data: None,
        };
        return Ok((
            StatusCode::BAD_REQUEST,  // HTTP 400
            Json(JsonRpcResponse::error(error, request.id.clone())),
        ));
    }

    // ================================================================================
    // 步骤 2: 提取方法和 ID 用于后续处理
    // ================================================================================
    let method = request.method.as_str();
    let id = request.id.clone();  // 保存 ID 用于构造响应

    // ================================================================================
    // 步骤 3: 根据方法名路由到相应的处理函数
    // ================================================================================
    match method {
        // ----------------------------------------------------------------------------
        // 方法：sandbox.repl.run - 在 REPL 环境中执行代码
        // ----------------------------------------------------------------------------
        "sandbox.repl.run" => {
            // 调用沙箱 REPL 执行实现函数
            match sandbox_run_impl(state, request.params).await {
                Ok(result) => {
                    // 执行成功，构造成功的 JSON-RPC 响应
                    // result 是执行结果的 JSON 值
                    Ok((StatusCode::OK, Json(JsonRpcResponse::success(result, id))))
                }
                Err(e) => {
                    // 执行失败，使用辅助函数构造错误响应
                    Ok(create_error_response(e, id))
                }
            }
        }
        // ----------------------------------------------------------------------------
        // 方法：sandbox.command.run - 执行系统命令
        // ----------------------------------------------------------------------------
        "sandbox.command.run" => {
            // 调用沙箱命令执行实现函数
            match sandbox_command_run_impl(state, request.params).await {
                Ok(result) => {
                    // 执行成功，构造成功的 JSON-RPC 响应
                    Ok((StatusCode::OK, Json(JsonRpcResponse::success(result, id))))
                }
                Err(e) => {
                    // 执行失败，使用辅助函数构造错误响应
                    Ok(create_error_response(e, id))
                }
            }
        }
        // ----------------------------------------------------------------------------
        // 默认：未知方法
        // ----------------------------------------------------------------------------
        _ => {
            // 方法未找到错误
            let error = PortalError::MethodNotFound(format!("Method not found: {}", method));
            Ok(create_error_response(error, id))
        }
    }
}

//--------------------------------------------------------------------------------------------------
// 函数实现 (Functions: Implementations)
//--------------------------------------------------------------------------------------------------

/// # 沙箱 REPL 执行实现函数
///
/// 此函数处理 `sandbox.repl.run` 方法的具体实现，负责在沙箱环境中执行代码。
///
/// ## 工作流程
///
/// 1. **参数反序列化**: 将 JSON 参数解析为 `SandboxReplRunParams` 结构
/// 2. **语言识别**: 将语言字符串转换为 `Language` 枚举
/// 3. **引擎获取/初始化**: 从共享状态获取或初始化 REPL 引擎
/// 4. **代码执行**: 在选定的语言引擎中执行代码
/// 5. **结果格式化**: 将输出格式化为 JSON 响应
///
/// ## 支持的语言
///
/// | 语言 | 特性标志 | 别名 |
/// |------|----------|------|
/// | Python | `python` | python |
/// | Node.js | `nodejs` | node, nodejs, javascript |
///
/// ## 特性标志 (Feature Flags)
///
/// Rust 的特性标志允许在编译时启用/禁用特定功能：
/// - `#[cfg(feature = "python")]` - 仅当启用 python 特性时编译此代码
/// - `#[cfg(any(feature = "python", feature = "nodejs"))]` - 任一特性启用时编译
///
/// ## 参数说明
///
/// * `_state` - 共享状态（未直接使用，但用于获取引擎句柄）
/// * `params` - JSON 格式的参数，包含：
///   - `code`: 要执行的代码字符串
///   - `language`: 编程语言名称
///   - `timeout`: 可选的执行超时时间（秒）
///
/// ## 返回值
///
/// * `Ok(Value)` - 执行结果，包含状态、语言和输出
/// * `Err(PortalError)` - 执行失败时的错误
///
/// ## Rust 概念解释
///
/// ### `#[allow(clippy::needless_late_init)]`
/// Clippy 是 Rust 的代码检查工具。此属性禁用关于"不必要的延迟初始化"的警告，
/// 因为我们在 match 表达式中初始化变量是合理的。
///
/// ### `#[cfg(...)]` 条件编译
/// 允许根据编译时的配置包含或排除代码，用于支持可选特性。
async fn sandbox_run_impl(_state: SharedState, params: Value) -> Result<Value, PortalError> {
    // 记录调试日志，?params 使用 Debug trait 格式化 params
    debug!(?params, "Sandbox run method called");

    // ================================================================================
    // 步骤 1: 反序列化参数
    // ================================================================================
    // 将原始 JSON Value 转换为强类型的 SandboxReplRunParams 结构
    // serde_json::from_value 执行反序列化
    // map_err 将反序列化错误转换为 PortalError::JsonRpc
    let params: SandboxReplRunParams = serde_json::from_value(params)
        .map_err(|e| PortalError::JsonRpc(format!("Invalid parameters: {}", e)))?;

    // ================================================================================
    // 步骤 2: 将语言字符串转换为 Language 枚举
    // ================================================================================
    // 延迟初始化变量（在 match 中赋值）
    #[cfg(any(feature = "python", feature = "nodejs"))]
    #[allow(clippy::needless_late_init)]
    let language;

    // 匹配语言字符串（转换为小写以支持大小写不敏感的匹配）
    match params.language.to_lowercase().as_str() {
        // Python 语言支持 - 仅当启用 python 特性时
        #[cfg(feature = "python")]
        "python" => language = Language::Python,

        // Node.js 语言支持 - 支持多种别名 - 仅当启用 nodejs 特性时
        #[cfg(feature = "nodejs")]
        "node" | "nodejs" | "javascript" => language = Language::Node,

        // 不支持的语言或特性未启用
        _ => {
            // 检查是否是支持但未启用特性的语言
            let error_msg = match params.language.to_lowercase().as_str() {
                "python" => {
                    "Python language support is not enabled. Recompile with --features python"
                        .to_string()
                }
                "node" | "nodejs" | "javascript" => {
                    "Node.js language support is not enabled. Recompile with --features nodejs"
                        .to_string()
                }
                _ => format!("Unsupported language: {}", params.language),
            };

            #[allow(clippy::needless_return)]
            return Err(PortalError::JsonRpc(error_msg));
        }
    };

    // ================================================================================
    // 步骤 3: 获取或初始化引擎句柄
    // ================================================================================
    // 使用 tokio::sync::Mutex 进行异步锁定
    // 与 std::sync::Mutex 不同，tokio 的 Mutex 可以在 .await 时安全地持有锁
    #[cfg(any(feature = "python", feature = "nodejs"))]
    let engine_handle = {
        // 获取当前引擎句柄（如果存在）
        // .lock().await 异步获取互斥锁
        let mut lock = _state.engine_handle.lock().await;

        if let Some(ref handle) = *lock {
            // 引擎已初始化，复用现有句柄
            // .clone() 增加 Arc 引用计数，不复制实际数据
            handle.clone()
        } else {
            // 首次请求，需要初始化新引擎
            let handle = start_engines()
                .await
                .map_err(|e| PortalError::Internal(format!("Failed to start engines: {}", e)))?;

            // 将新句柄存储到共享状态中
            // *lock 解引用 MutexGuard 以修改内部值
            *lock = Some(handle.clone());

            handle
        }
    };

    #[cfg(any(feature = "python", feature = "nodejs"))]
    debug!("Language: {}", params.language);

    // ================================================================================
    // 步骤 4: 生成临时执行 ID
    // ================================================================================
    // 使用 UUID v4 生成唯一的执行标识符
    // UUID (Universally Unique Identifier) 是一个 128 位的唯一标识符
    // v4 版本使用随机数生成，碰撞概率极低
    #[cfg(any(feature = "python", feature = "nodejs"))]
    let temp_id = uuid::Uuid::new_v4().to_string();

    // ================================================================================
    // 步骤 5: 在 REPL 中执行代码
    // ================================================================================
    // engine_handle.eval 方法：
    // - 将代码发送到相应语言的 REPL 引擎
    // - 等待执行完成或超时
    // - 返回输出行向量
    #[cfg(any(feature = "python", feature = "nodejs"))]
    let lines = engine_handle
        .eval(&params.code, language, &temp_id, params.timeout)
        .await
        .map_err(|e| PortalError::Internal(format!("REPL execution failed: {}", e)))?;

    #[cfg(any(feature = "python", feature = "nodejs"))]
    debug!("REPL execution produced {} output lines", lines.len());

    // ================================================================================
    // 步骤 6: 将输出行转换为 JSON 格式
    // ================================================================================
    // 遍历每一行输出，将其转换为 JSON 对象
    // line.stream 是 Stream 枚举（Stdout 或 Stderr）
    // 使用 match 将其转换为字符串 "stdout" 或 "stderr"
    #[cfg(any(feature = "python", feature = "nodejs"))]
    let output_lines: Vec<Value> = lines
        .iter()  // 创建迭代器
        .map(|line| {  // 对每行应用转换闭包
            json!({
                "stream": match line.stream {
                    crate::portal::repl::Stream::Stdout => "stdout",
                    crate::portal::repl::Stream::Stderr => "stderr",
                },
                "text": line.text,
            })
        })
        .collect();  // 收集为 Vec<Value>

    // ================================================================================
    // 步骤 7: 构造最终 JSON 响应
    // ================================================================================
    // 使用 json! 宏构造 JSON 对象
    // .to_string() 确保显式转换为 String 类型
    #[cfg(any(feature = "python", feature = "nodejs"))]
    let result = json!({
        "status": "success".to_string(),
        "language": params.language.to_string(),
        "output": output_lines,
    });

    #[cfg(any(feature = "python", feature = "nodejs"))]
    debug!("Returning result with output: {}", result);

    #[cfg(any(feature = "python", feature = "nodejs"))]
    Ok(result)
}

/// # 沙箱命令执行实现函数
///
/// 此函数处理 `sandbox.command.run` 方法的具体实现，负责在沙箱环境中执行系统命令。
///
/// ## 工作流程
///
/// 1. **参数反序列化**: 将 JSON 参数解析为 `SandboxCommandRunParams` 结构
/// 2. **命令执行器获取/初始化**: 从共享状态获取或初始化命令执行器
/// 3. **命令执行**: 执行指定的系统命令
/// 4. **结果格式化**: 将输出和退出码格式化为 JSON 响应
///
/// ## 参数说明
///
/// * `state` - 共享状态，包含命令执行器句柄
/// * `params` - JSON 格式的参数，包含：
///   - `command`: 要执行的命令（如 "ls", "echo"）
///   - `args`: 命令参数列表
///   - `timeout`: 可选的执行超时时间（秒）
///
/// ## 返回值
///
/// 返回的 JSON 对象包含：
/// ```json
/// {
///     "command": "ls",
///     "args": ["-la"],
///     "exit_code": 0,
///     "success": true,
///     "output": [
///         {"stream": "stdout", "text": "..."},
///         ...
///     ]
/// }
/// ```
async fn sandbox_command_run_impl(state: SharedState, params: Value) -> Result<Value, PortalError> {
    // 记录调试日志
    debug!(?params, "Sandbox command run method called");

    // ================================================================================
    // 步骤 1: 反序列化参数
    // ================================================================================
    let params: SandboxCommandRunParams = serde_json::from_value(params)
        .map_err(|e| PortalError::JsonRpc(format!("Invalid parameters: {}", e)))?;

    // ================================================================================
    // 步骤 2: 获取或初始化命令执行器句柄
    // ================================================================================
    let cmd_handle = {
        // 获取当前命令执行器句柄（如果存在）
        let mut lock = state.command_handle.lock().await;

        if let Some(ref handle) = *lock {
            // 命令执行器已初始化，复用现有句柄
            handle.clone()
        } else {
            // 首次请求，创建新的命令执行器
            let handle = create_command_executor();

            // 将新句柄存储到共享状态中
            *lock = Some(handle.clone());

            handle
        }
    };

    // ================================================================================
    // 步骤 3: 执行命令
    // ================================================================================
    // cmd_handle.execute 方法：
    // - 派生一个新的进程执行命令
    // - 捕获 stdout 和 stderr 输出
    // - 等待命令完成或超时
    // - 返回 (退出码，输出行向量)
    let (exit_code, output_lines) = cmd_handle
        .execute(&params.command, params.args.clone(), params.timeout)
        .await
        .map_err(|e| PortalError::Internal(format!("Command execution failed: {}", e)))?;

    // ================================================================================
    // 步骤 4: 格式化输出行
    // ================================================================================
    // 将 CommandLine 对象转换为 JSON 格式
    let formatted_lines = output_lines
        .iter()
        .map(|line| {
            json!({
                "stream": match line.stream {
                    crate::portal::repl::Stream::Stdout => "stdout",
                    crate::portal::repl::Stream::Stderr => "stderr",
                },
                "text": line.text,
            })
        })
        .collect::<Vec<Value>>();

    // ================================================================================
    // 步骤 5: 构造最终 JSON 响应
    // ================================================================================
    let result = json!({
        "command": params.command,
        "args": params.args,
        "exit_code": exit_code,
        "success": exit_code == 0,  // 退出码为 0 表示成功
        "output": formatted_lines,
    });

    debug!("Returning command result with output: {}", result);

    Ok(result)
}

//--------------------------------------------------------------------------------------------------
// 辅助函数 (Functions: Helpers)
//--------------------------------------------------------------------------------------------------

/// # JSON-RPC 错误响应辅助函数
///
/// 此辅助函数将 `PortalError` 转换为符合 JSON-RPC 2.0 规范的错误响应。
///
/// ## JSON-RPC 错误码说明
///
/// JSON-RPC 2.0 规范定义了以下标准错误码：
///
/// | 错误码 | 含义 | 说明 |
/// |--------|------|------|
/// | -32700 | Parse error | 无效的 JSON 输入，服务器无法解析 |
/// | -32600 | Invalid Request | JSON 不是有效的 JSON-RPC 请求 |
/// | -32601 | Method not found | 请求的方法不存在 |
/// | -32602 | Invalid params | 方法参数无效 |
/// | -32603 | Internal error | 其他内部错误 |
/// | -32000 到 -32099 | Server error | 保留给服务器特定的错误 |
///
/// ## 参数说明
///
/// * `error` - PortalError 枚举，表示发生的错误类型
/// * `id` - 原始请求的 ID，用于将错误响应与请求关联
///   - 通知（notification）请求没有 ID，此时为 None
///
/// ## 返回值
///
/// 返回元组 `(StatusCode, Json<JsonRpcResponse>)`：
/// - `StatusCode`: HTTP 状态码（如 400 Bad Request）
/// - `Json<JsonRpcResponse>`: JSON-RPC 格式的错误响应
///
/// ## 匹配表达式说明
///
/// `match &error` 使用引用匹配，避免转移错误值的所有权
/// 这样可以在构造错误消息后继续使用 error（如 error.to_string()）
fn create_error_response(
    error: PortalError,
    id: Option<Value>,
) -> (StatusCode, Json<JsonRpcResponse>) {
    // ================================================================================
    // 步骤 1: 根据 PortalError 类型确定 JSON-RPC 错误码
    // ================================================================================
    // 使用 match 表达式进行模式匹配
    // _ 是通配符，匹配剩余的所有情况
    let code = match &error {
        PortalError::JsonRpc(_) => -32600,        // Invalid Request - 无效请求
        PortalError::MethodNotFound(_) => -32601, // Method not found - 方法未找到
        PortalError::Parse(_) => -32700,          // Parse error - 解析错误
        PortalError::Internal(_) => -32603,       // Internal error - 内部错误
    };

    // ================================================================================
    // 步骤 2: 构造 JSON-RPC 错误对象
    // ================================================================================
    let json_rpc_error = JsonRpcError {
        code,  // 上面确定的错误码
        message: error.to_string(),  // 使用 Display trait 将错误转换为字符串
        data: None,  // 可选的额外错误数据，这里不使用
    };

    // ================================================================================
    // 步骤 3: 返回 HTTP 状态码和 JSON-RPC 错误响应
    // ================================================================================
    // 所有错误都返回 HTTP 400 Bad Request
    // JSON-RPC 的详细信息在响应体中
    (
        StatusCode::BAD_REQUEST,
        Json(JsonRpcResponse::error(json_rpc_error, id)),
    )
}
