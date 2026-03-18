//! # 监控指标模块
//!
//! 这个模块提供了 `Metrics` 结构体，用于获取沙箱的资源使用情况和运行状态。
//! 通过监控指标，你可以：
//!
//! - 了解沙箱的资源消耗（CPU、内存、磁盘）
//! - 判断沙箱是否正在运行
//! - 优化资源分配和成本
//! - 诊断性能问题
//!
//! ## 监控指标说明
//!
//! | 指标 | 类型 | 说明 |
//! |------|------|------|
//! | `cpu_usage` | `f32` | CPU 使用率（百分比，0-100） |
//! | `memory_usage` | `u64` | 内存使用量（MiB） |
//! | `disk_usage` | `u64` | 磁盘使用量（字节） |
//! | `running` | `bool` | 沙箱是否正在运行 |
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_sdk::{PythonSandbox, BaseSandbox};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut sandbox = PythonSandbox::create("test").await?;
//!     sandbox.start(None).await?;
//!
//!     // 获取监控接口
//!     let metrics = sandbox.metrics().await?;
//!
//!     // 检查沙箱是否运行
//!     if metrics.is_running().await? {
//!         println!("沙箱正在运行");
//!     }
//!
//!     // 获取 CPU 使用率
//!     if let Some(cpu) = metrics.cpu().await? {
//!         println!("CPU 使用率：{:.1}%", cpu);
//!     }
//!
//!     // 获取内存使用量
//!     if let Some(memory) = metrics.memory().await? {
//!         println!("内存使用：{} MiB", memory);
//!     }
//!
//!     // 获取磁盘使用量
//!     if let Some(disk) = metrics.disk().await? {
//!         println!("磁盘使用：{:.2} MB", disk as f64 / 1024.0 / 1024.0);
//!     }
//!
//!     // 获取所有指标
//!     let all = metrics.all().await?;
//!     println!("完整指标：{:?}", all);
//!
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```

use std::{error::Error, sync::Arc};

use serde_json::json;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::base::SandboxBase;

/// # 监控指标接口
///
/// `Metrics` 结构体提供了获取沙箱资源使用情况和运行状态的接口。
/// 它通过向 Microsandbox 服务器发送查询请求来获取实时指标。
///
/// ## 架构说明
///
/// ```text
/// ┌─────────────────┐         HTTP 请求         ┌─────────────────┐
/// │   Metrics       │ ───────────────────────►  │  Microsandbox   │
/// │                 │                          │     Server      │
/// │ - cpu()         │ ◄───────────────────────  │                 │
/// │ - memory()      │         JSON 响应          │ - 收集指标数据  │
/// │ - disk()        │                          │ - 返回指标      │
/// │ - is_running()  │                          │                 │
/// │ - all()         │                          │                 │
/// └─────────────────┘                          └─────────────────┘
/// ```
///
/// ## 设计说明
///
/// `Metrics` 使用 `Arc<Mutex<SandboxBase>>` 来共享对底层沙箱的访问，
/// 这与 `Command` 结构体的设计一致。
///
/// ## 字段说明
pub struct Metrics {
    /// 基础沙箱实例的共享引用
    ///
    /// 通过这个引用来：
    /// - 检查沙箱是否已启动
    /// - 获取服务器 URL 和认证信息
    /// - 发送指标查询请求
    base: Arc<Mutex<SandboxBase>>,
}

impl Metrics {
    /// # 创建新的 Metrics 实例
    ///
    /// 这是一个内部方法，通常通过沙箱的 `metrics()` 方法获取 `Metrics` 实例。
    ///
    /// ## 参数
    ///
    /// * `base` - 沙箱的共享引用
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `Metrics` 实例。
    pub fn new(base: Arc<Mutex<SandboxBase>>) -> Self {
        Self { base }
    }

    /// # 获取沙箱指标数据
    ///
    /// 这是一个内部方法，负责向服务器发送指标查询请求。
    /// 所有公共方法（`cpu()`、`memory()`、`disk()` 等）都使用这个方法。
    ///
    /// ## 请求流程
    ///
    /// ```text
    /// 1. 检查沙箱是否已启动
    ///    └── 未启动 → 返回 NotStarted 错误
    ///
    /// 2. 获取沙箱连接信息
    ///    - server_url: 服务器地址
    ///    - sandbox_name: 沙箱名称
    ///    - api_key: API 密钥（如果有）
    ///
    /// 3. 构建 JSON-RPC 请求
    ///    {
    ///      "jsonrpc": "2.0",
    ///      "method": "sandbox.metrics.get",
    ///      "params": {
    ///        "sandbox": "sandbox-name"
    ///      },
    ///      "id": "uuid-here"
    ///    }
    ///
    /// 4. 发送 HTTP POST 请求
    ///
    /// 5. 解析响应
    ///    ├── 检查 HTTP 状态码
    ///    ├── 检查 JSON-RPC 错误
    ///    └── 提取沙箱指标数据
    ///
    /// 6. 返回指标 JSON
    /// ```
    ///
    /// ## 错误处理
    ///
    /// 可能返回的错误：
    /// - [`SandboxError::NotStarted`] - 沙箱未启动
    /// - [`SandboxError::RequestFailed`] - 请求失败
    /// - [`SandboxError::InvalidResponse`] - 响应格式错误
    ///
    /// ## 返回值
    ///
    /// * `Ok(serde_json::Value)` - 沙箱指标数据
    /// * `Err(...)` - 获取失败
    ///
    /// ## 返回数据结构
    ///
    /// 服务器返回的指标数据格式：
    ///
    /// ```json
    /// {
    ///   "name": "sandbox-name",
    ///   "running": true,
    ///   "cpu_usage": 12.5,
    ///   "memory_usage": 256,
    ///   "disk_usage": 1048576
    /// }
    /// ```
    async fn get_metrics(&self) -> Result<serde_json::Value, Box<dyn Error + Send + Sync>> {
        // 检查沙箱是否已启动
        let is_started = {
            let base = self.base.lock().await;
            base.is_started
        };

        // 如果沙箱未启动，返回错误
        if !is_started {
            return Err(Box::new(crate::SandboxError::NotStarted));
        }

        // 提取沙箱连接信息
        // 使用块作用域来获取锁，锁会在块结束时自动释放
        let (server_url, sandbox_name, api_key) = {
            let base = self.base.lock().await;
            (
                base.server_url.clone(),
                base.name.clone(),
                base.api_key.clone(),
            )
        };

        // 构建 JSON-RPC 2.0 请求 payload
        // 使用 UUID v4 生成唯一的请求 ID
        let request_id = Uuid::new_v4().to_string();
        let payload = json!({
            "jsonrpc": "2.0",
            "method": "sandbox.metrics.get",
            "params": {
                "sandbox": sandbox_name,
            },
            "id": request_id,
        });

        // 创建 HTTP 客户端
        // 注意：这里每次都创建新的客户端，可以考虑优化为复用客户端
        let client = reqwest::Client::new();
        let mut req_builder = client
            .post(&format!("{}/api/v1/rpc", server_url))
            .json(&payload)
            .header("Content-Type", "application/json");

        // 如果有 API 密钥，添加认证头
        if let Some(key) = api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", key));
        }

        // 发送请求并处理错误
        let response = req_builder
            .send()
            .await
            .map_err(|e| Box::new(crate::SandboxError::RequestFailed(e.to_string())))?;

        // 检查 HTTP 响应状态码
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(Box::new(crate::SandboxError::RequestFailed(format!(
                "Failed to get sandbox metrics: {} - {}",
                status, error_text
            ))));
        }

        // 解析 JSON 响应
        let response_data: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Box::new(crate::SandboxError::InvalidResponse(e.to_string())))?;

        // 检查 JSON-RPC 错误字段
        if let Some(error) = response_data.get("error") {
            let message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(Box::new(crate::SandboxError::RequestFailed(format!(
                "Failed to get sandbox metrics: {}",
                message
            ))));
        }

        // 提取 result 字段
        let result = response_data.get("result").ok_or_else(|| {
            crate::SandboxError::InvalidResponse("Missing 'result' field".to_string())
        })?;

        // 提取 sandboxes 数组
        // 服务器可能返回多个沙箱的指标，我们只关心自己的
        let sandboxes = result
            .get("sandboxes")
            .and_then(|s| s.as_array())
            .ok_or_else(|| {
                crate::SandboxError::InvalidResponse("Missing 'sandboxes' array".to_string())
            })?;

        // 处理空响应（沙箱可能已被销毁）
        if sandboxes.is_empty() {
            return Ok(json!({}));
        }

        // 返回第一个（也是唯一一个）沙箱的数据
        // 在响应中，我们的沙箱应该在第一个位置
        Ok(sandboxes[0].clone())
    }

    /// # 获取所有指标
    ///
    /// 返回沙箱的完整指标数据，包括所有可用的资源使用情况和状态信息。
    ///
    /// ## 返回值
    ///
    /// * `Ok(serde_json::Value)` - 包含所有指标的 JSON 对象
    /// * `Err(...)` - 获取失败
    ///
    /// ## 返回数据格式
    ///
    /// ```json
    /// {
    ///   "name": "sandbox-name",
    ///   "running": true,
    ///   "cpu_usage": 12.5,
    ///   "memory_usage": 256,
    ///   "disk_usage": 1048576
    /// }
    /// ```
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let metrics = sandbox.metrics().await?;
    /// let all = metrics.all().await?;
    ///
    /// println!("沙箱名称：{}", all["name"]);
    /// println!("运行状态：{}", all["running"]);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn all(&self) -> Result<serde_json::Value, Box<dyn Error + Send + Sync>> {
        self.get_metrics().await
    }

    /// # 获取 CPU 使用率
    ///
    /// 返回沙箱的 CPU 使用率百分比。
    ///
    /// ## 返回值
    ///
    /// * `Ok(Some(f32))` - CPU 使用率百分比（0-100）
    /// * `Ok(None)` - 指标不可用（沙箱未运行或服务器未提供）
    /// * `Err(...)` - 获取失败
    ///
    /// ## 说明
    ///
    /// - `0.0` - 完全空闲
    /// - `100.0` - 完全占用
    /// - `50.0` - 一半的 CPU 资源正在使用
    ///
    /// 注意：对于 idle 的沙箱，CPU 使用率可能为 0.0 或不精确。
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let metrics = sandbox.metrics().await?;
    ///
    /// if let Some(cpu) = metrics.cpu().await? {
    ///     println!("CPU 使用率：{:.1}%", cpu);
    /// } else {
    ///     println!("CPU 指标不可用");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn cpu(&self) -> Result<Option<f32>, Box<dyn Error + Send + Sync>> {
        let metrics = self.get_metrics().await?;
        // as_f64() 尝试将 JSON 值转换为 f64
        // map(|v| v as f32) 将 f64 转换为 f32
        Ok(metrics
            .get("cpu_usage")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32))
    }

    /// # 获取内存使用量
    ///
    /// 返回沙箱的内存使用量（以 MiB 为单位）。
    ///
    /// ## 返回值
    ///
    /// * `Ok(Some(u64))` - 内存使用量（MiB）
    /// * `Ok(None)` - 指标不可用
    /// * `Err(...)` - 获取失败
    ///
    /// ## 单位说明
    ///
    /// - 1 MiB = 1024 KiB = 1,048,576 字节
    /// - 例如：`256` 表示 256 MiB
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let metrics = sandbox.metrics().await?;
    ///
    /// if let Some(memory) = metrics.memory().await? {
    ///     println!("内存使用：{} MiB", memory);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn memory(&self) -> Result<Option<u64>, Box<dyn Error + Send + Sync>> {
        let metrics = self.get_metrics().await?;
        // as_u64() 尝试将 JSON 值转换为 u64
        Ok(metrics.get("memory_usage").and_then(|v| v.as_u64()))
    }

    /// # 获取磁盘使用量
    ///
    /// 返回沙箱的磁盘使用量（以字节为单位）。
    ///
    /// ## 返回值
    ///
    /// * `Ok(Some(u64))` - 磁盘使用量（字节）
    /// * `Ok(None)` - 指标不可用
    /// * `Err(...)` - 获取失败
    ///
    /// ## 单位转换
    ///
    /// ```rust
    /// # fn example(disk: u64) {
    /// // 字节 → KB
    /// let kb = disk as f64 / 1024.0;
    ///
    /// // 字节 → MB
    /// let mb = disk as f64 / 1024.0 / 1024.0;
    ///
    /// // 字节 → GB
    /// let gb = disk as f64 / 1024.0 / 1024.0 / 1024.0;
    /// # }
    /// ```
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let metrics = sandbox.metrics().await?;
    ///
    /// if let Some(disk) = metrics.disk().await? {
    ///     println!("磁盘使用：{:.2} MB", disk as f64 / 1024.0 / 1024.0);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn disk(&self) -> Result<Option<u64>, Box<dyn Error + Send + Sync>> {
        let metrics = self.get_metrics().await?;
        Ok(metrics.get("disk_usage").and_then(|v| v.as_u64()))
    }

    /// # 检查沙箱是否正在运行
    ///
    /// 返回沙箱的当前运行状态。
    ///
    /// ## 返回值
    ///
    /// * `Ok(true)` - 沙箱正在运行
    /// * `Ok(false)` - 沙箱已停止或未运行
    /// * `Err(...)` - 获取失败
    ///
    /// ## 与 `is_started()` 的区别
    ///
    /// - `is_started()` - 检查 SDK 内部状态（是否调用了 `start()`）
    /// - `is_running()` - 查询服务器获取实际运行状态
    ///
    /// 通常情况下两者应该一致，但如果沙箱在外部被停止（例如服务器重启），
    /// `is_running()` 会反映真实状态。
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let metrics = sandbox.metrics().await?;
    ///
    /// if metrics.is_running().await? {
    ///     println!("沙箱正在运行，可以执行代码");
    /// } else {
    ///     println!("沙箱未运行");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn is_running(&self) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let metrics = self.get_metrics().await?;
        // 获取 running 字段，如果不存在或不是布尔值则返回 false
        Ok(metrics
            .get("running")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }
}
