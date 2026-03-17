//! # JSON-RPC 载荷模块 (Payload Structures)
//!
//! 本模块定义了 microsandbox portal 使用的 JSON-RPC 协议相关的数据结构。
//!
//! ## JSON-RPC 2.0 协议简介
//!
//! JSON-RPC 是一种轻量级的远程过程调用 (RPC) 协议，使用 JSON 格式编码消息。
//! 它具有以下特点：
//! - **简单**: 使用标准的 JSON 格式
//! - **无状态**: 每个请求独立处理
//! - **语言无关**: 任何支持 JSON 的编程语言都可以使用
//!
//! ## 主要组件
//!
//! ### 请求结构 (JsonRpcRequest)
//! ```json
//! {
//!     "jsonrpc": "2.0",      // 协议版本，必须为 "2.0"
//!     "method": "method_name", // 要调用的方法名
//!     "params": { ... },       // 方法参数（可选）
//!     "id": 1                  // 请求 ID，用于匹配响应（通知没有 ID）
//! }
//! ```
//!
//! ### 响应结构 (JsonRpcResponse)
//! ```json
//! // 成功响应
//! {
//!     "jsonrpc": "2.0",
//!     "result": { ... },  // 方法执行结果
//!     "id": 1
//! }
//!
//! // 错误响应
//! {
//!     "jsonrpc": "2.0",
//!     "error": {
//!         "code": -32600,
//!         "message": "Invalid Request"
//!     },
//!     "id": 1
//! }
//! ```
//!
//! ### 错误结构 (JsonRpcError)
//! ```json
//! {
//!     "code": -32600,       // 错误码（整数）
//!     "message": "...",     // 错误描述
//!     "data": null          // 可选的额外信息
//! }
//! ```
//!
//! ## 标准错误码
//!
//! | 错误码 | 含义 |
//! |--------|------|
//! | -32700 | Parse error - JSON 解析失败 |
//! | -32600 | Invalid Request - 无效的 JSON-RPC 请求 |
//! | -32601 | Method not found - 方法不存在 |
//! | -32602 | Invalid params - 参数无效 |
//! | -32603 | Internal error - 内部错误 |
//!
//! ## Serde 序列化/反序列化
//!
//! 本模块使用 Serde crate 进行 JSON 序列化：
//! - `#[derive(Serialize)]` - 将 Rust 结构转换为 JSON
//! - `#[derive(Deserialize)]` - 将 JSON 解析为 Rust 结构
//! - `#[serde(...)]` - 自定义序列化行为

use serde::{Deserialize, Serialize};
use serde_json::Value;

//--------------------------------------------------------------------------------------------------
// 常量 (Constants)
//--------------------------------------------------------------------------------------------------

/// JSON-RPC 协议版本号
///
/// 根据 JSON-RPC 2.0 规范，此字段必须为字符串 "2.0"
/// 服务器会拒绝版本号不匹配的请求
pub const JSONRPC_VERSION: &str = "2.0";

//--------------------------------------------------------------------------------------------------
// 类型：JSON-RPC 结构 (Types: JSON-RPC Structures)
//--------------------------------------------------------------------------------------------------

/// # JSON-RPC 请求结构
///
/// 表示一个完整的 JSON-RPC 2.0 请求。
///
/// ## 字段说明
///
/// * `jsonrpc` - 协议版本，必须为 "2.0"
/// * `method` - 要调用的方法名称（如 "sandbox.repl.run"）
/// * `params` - 方法参数，可以是任何 JSON 值
/// * `id` - 请求 ID，用于将响应与请求匹配
///   - 如果为 `None`，表示这是一个"通知"(notification)，服务器不返回响应
///
/// ## Serde 属性说明
///
/// * `#[derive(Debug)]` - 自动实现 Debug trait，用于调试输出
/// * `#[derive(Deserialize)]` - 可以从 JSON 反序列化
/// * `#[derive(Serialize)]` - 可以序列化为 JSON
/// * `#[serde(default)]` - 如果字段缺失，使用类型的 Default 值
/// * `#[serde(skip_serializing_if = "Option::is_none")]` - 如果值为 None，序列化时省略该字段
///
/// ## 示例
///
/// ```json
/// {
///     "jsonrpc": "2.0",
///     "method": "sandbox.repl.run",
///     "params": {"code": "print('hello')", "language": "python"},
///     "id": 1
/// }
/// ```
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC 版本号，必须为 "2.0"
    pub jsonrpc: String,

    /// 方法名称 - 标识要调用的远程过程
    pub method: String,

    /// 方法参数 - 可选，默认为 null
    /// 使用 Value 类型可以接受任何 JSON 值
    #[serde(default)]
    pub params: Value,

    /// 请求 ID - 可选
    /// - 有 ID: 普通请求，服务器返回响应
    /// - 无 ID (None): 通知，服务器不返回响应
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

/// # JSON-RPC 响应结构
///
/// 表示一个完整的 JSON-RPC 2.0 响应。
///
/// ## 字段说明
///
/// * `jsonrpc` - 协议版本，始终为 "2.0"
/// * `result` - 方法执行结果，成功时包含返回值
/// * `error` - 错误信息，失败时包含错误详情
/// * `id` - 响应 ID，与请求 ID 相同
///
/// ## 响应规则
///
/// - 成功响应：`result` 有值，`error` 为 null
/// - 失败响应：`error` 有值，`result` 为 null
/// - 通知请求：不返回任何响应
///
/// ## Serde 属性说明
///
/// * `skip_serializing_if = "Option::is_none"` - 如果值为 None，序列化时省略
///   这使得成功响应不包含 error 字段，失败响应不包含 result 字段
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC 版本号，始终为 "2.0"
    pub jsonrpc: String,

    /// 执行结果 - 仅当请求成功时存在
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// 错误详情 - 仅当请求失败时存在
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,

    /// 响应 ID - 与请求 ID 相同，用于匹配请求和响应
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

/// # JSON-RPC 错误结构
///
/// 表示 JSON-RPC 响应中的错误信息。
///
/// ## 字段说明
///
/// * `code` - 错误码，负整数
///   - -32700 到 -32000: 预定义的错误码
///   - -32000 到 -32099: 保留给服务器特定的错误
/// * `message` - 错误的简短描述
/// * `data` - 可选的额外错误信息，可以是任何 JSON 值
///
/// ## 示例
///
/// ```json
/// {
///     "code": -32601,
///     "message": "Method not found",
///     "data": null
/// }
/// ```
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcError {
    /// 错误码 - 负整数，标识错误类型
    pub code: i32,

    /// 错误消息 - 人类可读的错误描述
    pub message: String,

    /// 可选的额外数据 - 可以包含调试信息等
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

//--------------------------------------------------------------------------------------------------
// 类型：REST API 请求参数 (Types: REST API Requests)
//--------------------------------------------------------------------------------------------------

/// # 沙箱 REPL 执行请求参数
///
/// 用于在沙箱 REPL 环境中执行代码的请求参数。
///
/// ## 字段说明
///
/// * `code` - 要执行的源代码
///   - 可以是任何有效的编程语言代码
///   - 对于 Python，可以是多行代码
///   - 对于 JavaScript，同样支持多行
///
/// * `language` - 编程语言名称
///   - 支持的值：`"python"`, `"node"`, `"nodejs"`, `"javascript"`
///   - 不区分大小写（处理时会转换为小写）
///
/// * `timeout` - 可选的执行超时时间（秒）
///   - `Some(n)`: n 秒后终止执行
///   - `None`: 不设置超时（可能无限期运行）
///
/// ## 使用示例
///
/// ```json
/// {
///     "code": "print('Hello, World!')",
///     "language": "python",
///     "timeout": 30
/// }
/// ```
///
/// ## Rust 概念解释
///
/// ### `String` vs `&str`
/// - `String`: 可拥有的、可变的字符串类型
/// - `&str`: 字符串切片，是对字符串的引用
/// 结构体通常使用 `String` 以拥有数据的所有权
///
/// ### `Option<T>`
/// 表示一个可选值：
/// - `Some(T)`: 包含值 T
/// - `None`: 不包含值
/// 这里用于表示 timeout 是可选的
#[derive(Debug, Deserialize, Serialize)]
pub struct SandboxReplRunParams {
    /// 要执行的代码
    pub code: String,

    /// 编程语言名称
    pub language: String,

    /// 可选的超时时间（秒）
    /// None 表示不设置超时
    pub timeout: Option<u64>,
}

/// # 沙箱命令执行请求参数
///
/// 用于在沙箱环境中执行系统命令的请求参数。
///
/// ## 字段说明
///
/// * `command` - 要执行的命令名称
///   - 如 `"ls"`, `"echo"`, `"python"` 等
///   - 必须在系统 PATH 中可找到
///
/// * `args` - 命令的参数列表
///   - `Vec<String>` 表示字符串的动态数组
///   - `#[serde(default)]` 表示如果 JSON 中缺少此字段，使用空数组
///
/// * `timeout` - 可选的执行超时时间（秒）
///   - 同 `SandboxReplRunParams::timeout`
///
/// ## 使用示例
///
/// ```json
/// {
///     "command": "ls",
///     "args": ["-la", "/home"],
///     "timeout": 10
/// }
/// ```
///
/// ## Vec<T> 说明
///
/// `Vec` 是 Rust 标准库提供的动态数组类型：
/// - 可以动态增长或缩小
/// - 存储相同类型的元素
/// - 类似其他语言的 ArrayList 或 List
#[derive(Debug, Deserialize, Serialize)]
pub struct SandboxCommandRunParams {
    /// 要执行的命令
    pub command: String,

    /// 命令参数列表 - 默认为空数组
    #[serde(default)]
    pub args: Vec<String>,

    /// 可选的超时时间（秒）
    pub timeout: Option<u64>,
}

//--------------------------------------------------------------------------------------------------
// 方法实现 (Methods)
//--------------------------------------------------------------------------------------------------

impl JsonRpcRequest {
    /// # 创建新的 JSON-RPC 请求
    ///
    /// 这是一个便捷构造函数，用于创建完整的 JSON-RPC 请求对象。
    ///
    /// ## 参数说明
    ///
    /// * `method` - 要调用的方法名称（如 "sandbox.repl.run"）
    /// * `params` - 方法参数，任何可序列化为 JSON 的值
    /// * `id` - 请求 ID，用于匹配响应
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `JsonRpcRequest` 实例，其中：
    /// - `jsonrpc` 自动设置为 "2.0"
    /// - 其他字段使用传入的参数
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// let request = JsonRpcRequest::new(
    ///     "sandbox.repl.run".to_string(),
    ///     json!({"code": "print('hi')", "language": "python"}),
    ///     json!(1)
    /// );
    /// ```
    pub fn new(method: String, params: Value, id: Value) -> Self {
        Self {
            // 使用常量 JSONRPC_VERSION 确保版本一致性
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
            id: Some(id),
        }
    }

    /// # 创建 JSON-RPC 通知（无需响应）
    ///
    /// 通知是一种特殊的请求，服务器处理但不返回响应。
    ///
    /// ## 与请求的区别
    ///
    /// | 特性 | 请求 (Request) | 通知 (Notification) |
    /// |------|----------------|---------------------|
    /// | id 字段 | 必须有 | 必须无 |
    /// | 响应 | 服务器返回响应 | 服务器不返回响应 |
    /// | 用途 | 需要结果的调用 | 单向操作/事件 |
    ///
    /// ## 参数说明
    ///
    /// * `method` - 方法名称
    /// * `params` - 方法参数
    ///
    /// ## 返回值
    ///
    /// 返回一个 `id` 为 `None` 的 `JsonRpcRequest`，表示通知
    pub fn new_notification(method: String, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
            id: None,  // 通知没有 ID
        }
    }

    /// # 检查是否为通知
    ///
    /// 通知的定义：没有 `id` 字段的请求。
    ///
    /// ## 返回值
    ///
    /// * `true` - 是通知（id 为 None）
    /// * `false` - 是普通请求（id 有值）
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// let notification = JsonRpcRequest::new_notification(...);
    /// assert!(notification.is_notification());
    ///
    /// let request = JsonRpcRequest::new(...);
    /// assert!(!request.is_notification());
    /// ```
    pub fn is_notification(&self) -> bool {
        // Option::is_none() 检查 Option 是否为 None
        self.id.is_none()
    }
}

impl JsonRpcResponse {
    /// # 创建成功的 JSON-RPC 响应
    ///
    /// 构造函数用于创建表示成功执行的响应。
    ///
    /// ## 参数说明
    ///
    /// * `result` - 方法执行的结果值
    /// * `id` - 响应 ID，应与请求 ID 相同
    ///
    /// ## 返回值
    ///
    /// 返回一个 `JsonRpcResponse` 实例：
    /// - `result` 设置为 `Some(result)`
    /// - `error` 设置为 `None`
    /// - `jsonrpc` 自动设置为 "2.0"
    ///
    /// ## 生成的 JSON 示例
    ///
    /// ```json
    /// {
    ///     "jsonrpc": "2.0",
    ///     "result": {"status": "ok"},
    ///     "id": 1
    /// }
    /// ```
    pub fn success(result: Value, id: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),  // 成功响应包含结果
            error: None,           // 成功响应没有错误
            id,
        }
    }

    /// # 创建错误的 JSON-RPC 响应
    ///
    /// 构造函数用于创建表示执行失败的响应。
    ///
    /// ## 参数说明
    ///
    /// * `error` - `JsonRpcError` 对象，包含错误详情
    /// * `id` - 响应 ID，应与请求 ID 相同
    ///
    /// ## 返回值
    ///
    /// 返回一个 `JsonRpcResponse` 实例：
    /// - `result` 设置为 `None`
    /// - `error` 设置为 `Some(error)`
    /// - `jsonrpc` 自动设置为 "2.0"
    ///
    /// ## 生成的 JSON 示例
    ///
    /// ```json
    /// {
    ///     "jsonrpc": "2.0",
    ///     "error": {
    ///         "code": -32601,
    ///         "message": "Method not found"
    ///     },
    ///     "id": 1
    /// }
    /// ```
    pub fn error(error: JsonRpcError, id: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,          // 错误响应没有结果
            error: Some(error),    // 错误响应包含错误信息
            id,
        }
    }
}
