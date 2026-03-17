//! # 数据结构模块 - 请求和响应载荷定义
//!
//! 本模块定义了微沙箱服务器 API 使用的所有数据结构和载荷类型。
//!
//! ## 模块结构
//!
//! ```text
//! payload.rs
//! ├── JSON-RPC 基础结构
//! │   ├── JsonRpcRequest - JSON-RPC 请求
//! │   ├── JsonRpcResponse - JSON-RPC 响应
//! │   ├── JsonRpcNotification - JSON-RPC 通知 (无需响应)
//! │   ├── JsonRpcError - JSON-RPC 错误
//! │   └── JsonRpcResponseOrNotification - 响应或通知的枚举
//! │
//! ├── 沙箱操作参数
//! │   ├── SandboxStartParams - 启动沙箱参数
//! │   ├── SandboxStopParams - 停止沙箱参数
//! │   └── SandboxMetricsGetParams - 获取指标参数
//! │
//! ├── 沙箱配置
//! │   └── SandboxConfig - 沙箱配置结构
//! │
//! ├── Portal RPC 参数
//! │   ├── SandboxReplRunParams - REPL 代码执行参数
//! │   ├── SandboxReplGetOutputParams - 获取 REPL 输出参数
//! │   ├── SandboxCommandRunParams - 命令执行参数
//! │   └── SandboxCommandGetOutputParams - 获取命令输出参数
//! │
//! └── 响应结构
//!     ├── RegularMessageResponse - 普通消息响应
//!     ├── SystemStatusResponse - 系统状态响应
//!     ├── SandboxStatusResponse - 沙箱状态响应
//!     ├── SandboxConfigResponse - 沙箱配置响应
//!     └── SandboxStatus - 单个沙箱状态
//! ```
//!
//! ## JSON-RPC 协议说明
//!
//! 本服务器遵循 [JSON-RPC 2.0](https://www.jsonrpc.org/specification) 规范。

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// JSON-RPC 协议版本
///
/// 根据 JSON-RPC 2.0 规范，此字段必须精确为 "2.0"。
pub const JSONRPC_VERSION: &str = "2.0";

//--------------------------------------------------------------------------------------------------
// 类型定义：JSON-RPC 载荷
//--------------------------------------------------------------------------------------------------

/// # JSON-RPC 请求结构
///
/// 遵循 JSON-RPC 2.0 规范的请求格式。
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC 版本号，必须为 "2.0"
    pub jsonrpc: String,

    /// 方法名称
    pub method: String,

    /// 方法参数
    ///
    /// 使用 `#[serde(default)]` 属性，如果请求中缺少此字段，
    /// 则使用默认值（对于 Value 是 Null）。
    #[serde(default)]
    pub params: Value,

    /// 请求 ID
    ///
    /// - 如果存在：这是一个常规请求，需要返回响应
    /// - 如果不存在：这是一个通知，不需要响应
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

/// # JSON-RPC 通知结构
///
/// 通知是一种特殊的 JSON-RPC 请求，它不需要服务器返回响应。
/// 通知没有 `id` 字段，这是区分通知和常规请求的唯一方式。
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcNotification {
    /// JSON-RPC 版本号，必须为 "2.0"
    pub jsonrpc: String,

    /// 方法名称
    pub method: String,

    /// 方法参数
    #[serde(default)]
    pub params: Value,
}

/// # JSON-RPC 响应结构
///
/// 遵循 JSON-RPC 2.0 规范的响应格式。
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    /// JSON-RPC 版本号，固定为 "2.0"
    pub jsonrpc: String,

    /// 方法执行结果（如果成功）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// 错误详情（如果失败）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,

    /// 响应 ID，与请求 ID 相同
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
}

/// # JSON-RPC 错误结构
///
/// 遵循 JSON-RPC 2.0 规范的错误格式。
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcError {
    /// 错误码
    ///
    /// 标准错误码：
    /// - -32700: 解析错误
    /// - -32600: 无效请求
    /// - -32601: 方法不存在
    /// - -32602: 无效参数
    /// - -32603: 内部错误
    pub code: i32,

    /// 错误消息
    pub message: String,

    /// 可选的错误数据
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// # JSON-RPC 响应或通知枚举
///
/// 此枚举允许处理器返回两种结果：
/// - 常规响应（对于有 id 的请求）
/// - 通知处理结果（对于无 id 的通知）
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum JsonRpcResponseOrNotification {
    /// 常规 JSON-RPC 响应
    Response(JsonRpcResponse),

    /// 已处理的通知（无需响应）
    Notification(ProcessedNotification),
}

/// # 已处理的通知标记
///
/// 此结构表示一个 JSON-RPC 通知已被处理，不需要返回响应。
#[derive(Debug, Serialize)]
pub struct ProcessedNotification {
    /// 表示此通知已被处理
    #[serde(skip)]
    pub processed: bool,
}

//--------------------------------------------------------------------------------------------------
// 类型定义：服务器操作
//--------------------------------------------------------------------------------------------------

/// # 启动沙箱请求参数
#[derive(Debug, Deserialize)]
pub struct SandboxStartParams {
    /// 沙箱名称
    pub sandbox: String,

    /// 可选的沙箱配置
    pub config: Option<SandboxConfig>,
}

/// # 停止沙箱请求参数
#[derive(Debug, Deserialize)]
pub struct SandboxStopParams {
    /// 沙箱名称
    pub sandbox: String,
}

/// # 获取沙箱指标请求参数
#[derive(Debug, Deserialize)]
pub struct SandboxMetricsGetParams {
    /// 可选的沙箱名称
    ///
    /// - 如果提供：只返回指定沙箱的指标
    /// - 如果省略或为 null：返回所有沙箱的指标
    pub sandbox: Option<String>,
}

/// # 沙箱配置结构
///
/// 定义沙箱的运行参数，与 `microsandbox-core` 中的 Sandbox 结构类似，
/// 但所有字段都是可选的，便于更新操作。
#[derive(Debug, Deserialize)]
pub struct SandboxConfig {
    /// Docker 镜像名称
    pub image: Option<String>,

    /// 内存限制（MiB）
    pub memory: Option<u32>,

    /// CPU 核心数
    pub cpus: Option<u8>,

    /// 卷挂载列表
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub volumes: Vec<String>,

    /// 端口映射列表
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub ports: Vec<String>,

    /// 环境变量列表
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub envs: Vec<String>,

    /// 依赖的沙箱名称列表
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub depends_on: Vec<String>,

    /// 工作目录
    pub workdir: Option<String>,

    /// Shell 程序路径
    pub shell: Option<String>,

    /// 预定义脚本
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub scripts: std::collections::HashMap<String, String>,

    /// 启动命令
    pub exec: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// 类型定义：Portal 镜像 RPC 载荷
//--------------------------------------------------------------------------------------------------

/// # REPL 代码执行参数
#[derive(Debug, Deserialize, Serialize)]
pub struct SandboxReplRunParams {
    /// 沙箱名称
    pub sandbox: String,

    /// 要执行的代码
    pub code: String,

    /// 编程语言
    pub language: String,
}

/// # 获取 REPL 输出参数
#[derive(Debug, Deserialize, Serialize)]
pub struct SandboxReplGetOutputParams {
    /// 执行 ID
    pub execution_id: String,
}

/// # 命令执行参数
#[derive(Debug, Deserialize, Serialize)]
pub struct SandboxCommandRunParams {
    /// 沙箱名称
    pub sandbox: String,

    /// 要执行的命令
    pub command: String,

    /// 命令参数
    #[serde(default)]
    pub args: Vec<String>,

    /// 超时时间（秒）
    pub timeout: Option<i32>,
}

/// # 获取命令输出参数
#[derive(Debug, Deserialize, Serialize)]
pub struct SandboxCommandGetOutputParams {
    /// 执行 ID
    pub execution_id: String,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl JsonRpcRequest {
    /// 创建新的 JSON-RPC 请求
    pub fn new(method: String, params: Value, id: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
            id: Some(id),
        }
    }

    /// 创建新的 JSON-RPC 通知
    pub fn new_notification(method: String, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method,
            params,
            id: None,
        }
    }

    /// 检查是否为通知
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

impl ProcessedNotification {
    /// 创建已处理的通知标记
    pub fn processed() -> Self {
        Self { processed: true }
    }
}

impl JsonRpcResponseOrNotification {
    /// 创建成功响应
    pub fn success(result: Value, id: Option<Value>) -> Self {
        Self::Response(JsonRpcResponse::success(result, id))
    }

    /// 创建错误响应
    pub fn error(error: JsonRpcError, id: Option<Value>) -> Self {
        Self::Response(JsonRpcResponse::error(error, id))
    }

    /// 从 JsonRpcResponse 创建响应
    pub fn response(response: JsonRpcResponse) -> Self {
        Self::Response(response)
    }

    /// 创建通知结果
    pub fn notification(notification: ProcessedNotification) -> Self {
        Self::Notification(notification)
    }

    /// 创建无响应结果（已废弃）
    pub fn no_response() -> Self {
        Self::Notification(ProcessedNotification::processed())
    }
}

impl JsonRpcResponse {
    /// 创建成功的 JSON-RPC 响应
    pub fn success(result: Value, id: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// 创建错误的 JSON-RPC 响应
    pub fn error(error: JsonRpcError, id: Option<Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// 类型定义：响应
//--------------------------------------------------------------------------------------------------

/// # 普通消息响应
#[derive(Debug, Serialize)]
pub struct RegularMessageResponse {
    /// 沙箱操作的状态消息
    pub message: String,
}

/// # 系统状态响应
#[derive(Debug, Serialize)]
pub struct SystemStatusResponse {}

/// # 沙箱状态响应
#[derive(Debug, Serialize)]
pub struct SandboxStatusResponse {
    /// 沙箱状态列表
    pub sandboxes: Vec<SandboxStatus>,
}

/// # 沙箱配置响应
#[derive(Debug, Serialize)]
pub struct SandboxConfigResponse {}

/// # 单个沙箱的状态
#[derive(Debug, Serialize)]
pub struct SandboxStatus {
    /// 沙箱名称
    pub name: String,

    /// 是否正在运行
    pub running: bool,

    /// CPU 使用率（百分比）
    pub cpu_usage: Option<f32>,

    /// 内存使用量（字节）
    pub memory_usage: Option<u64>,

    /// 磁盘使用量（字节）
    pub disk_usage: Option<u64>,
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

/// 将响应转换为 Axum HTTP 响应
impl axum::response::IntoResponse for JsonRpcResponseOrNotification {
    fn into_response(self) -> axum::response::Response {
        match self {
            // 常规响应：返回 JSON
            JsonRpcResponseOrNotification::Response(response) => {
                (axum::http::StatusCode::OK, axum::Json(response)).into_response()
            }
            // 通知：返回空响应（JSON-RPC 规范：通知不需要响应）
            JsonRpcResponseOrNotification::Notification(_notification) => {
                axum::http::StatusCode::OK.into_response()
            }
        }
    }
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// # 反序列化 null 为默认值
///
/// 此函数用于处理 JSON 中显式为 `null` 的字段，将其转换为目标类型的默认值。
///
/// ## 使用示例
///
/// ```rust,ignore
/// #[derive(Deserialize)]
/// struct Config {
///     // 如果客户端发送 "volumes": null，会转换为 Vec::new()
///     #[serde(default, deserialize_with = "deserialize_null_as_default")]
///     volumes: Vec<String>,
/// }
/// ```
fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    // 尝试反序列化为 Option<T>
    // 如果是 null，返回 None；如果是值，返回 Some(value)
    // 然后使用 unwrap_or_default() 将 None 转换为 T::default()
    Ok(Option::deserialize(deserializer)?.unwrap_or_default())
}
