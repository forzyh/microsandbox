//! # 沙箱基础实现模块
//!
//! 这个模块包含了 `SandboxBase` 结构体，它是所有沙箱类型的核心基础实现。
//! `SandboxBase` 负责处理与 Microsandbox 服务器的通信、沙箱的生命周期管理
//! 以及代码执行的底层逻辑。
//!
//! ## 架构说明
//!
//! Microsandbox 采用客户端 - 服务器架构：
//!
//! ```text
//! ┌─────────────────┐         HTTP/JSON-RPC         ┌─────────────────┐
//! │   Rust SDK      │ ◄──────────────────────────►  │  Microsandbox   │
//! │  (SandboxBase)  │                               │     Server      │
//! └─────────────────┘                               └─────────────────┘
//!      |                                                   |
//!      v                                                   v
//!  发送请求                                            管理 Docker 容器
//!  处理响应                                            执行代码
//! ```
//!
//! ## JSON-RPC 协议
//!
//! SDK 与服务器之间的通信使用 JSON-RPC 2.0 协议。每个请求包含：
//! - `jsonrpc`: 协议版本（"2.0"）
//! - `method`: 要调用的方法名
//! - `params`: 方法参数
//! - `id`: 请求的唯一标识符（用于匹配响应）
//!
//! 响应包含：
//! - `jsonrpc`: 协议版本
//! - `result`: 方法返回结果（成功时）
//! - `error`: 错误信息（失败时）
//! - `id`: 对应的请求 ID

use std::{collections::HashMap, env, error::Error, time::Duration};

use dotenv::dotenv;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{Execution, SandboxError, SandboxOptions};

/// # 沙箱基础结构体
///
/// `SandboxBase` 是所有沙箱类型的核心实现，封装了与 Microsandbox 服务器
/// 通信的所有细节。
///
/// ## 主要职责
///
/// 1. **服务器连接管理**：维护与 Microsandbox 服务器的连接
/// 2. **身份验证**：处理 API 密钥和认证头
/// 3. **沙箱生命周期**：启动和停止沙箱容器
/// 4. **代码执行**：发送代码执行请求并处理响应
/// 5. **请求封装**：统一的 JSON-RPC 请求构建和解析
///
/// ## 字段说明
///
/// 注意：所有字段都标记为 `pub(crate)`，这意味着它们只能在 crate 内部
/// 访问。这是封装的一种形式，外部用户应该通过公共方法操作沙箱。
pub struct SandboxBase {
    /// Microsandbox 服务器的 URL 地址
    ///
    /// 格式通常为：
    /// - 本地：`http://127.0.0.1:5555`
    /// - 远程：`https://api.mbs.example.com`
    ///
    /// 可以通过以下方式设置：
    /// 1. `SandboxOptions` 中的 `server_url` 字段
    /// 2. 环境变量 `MSB_SERVER_URL`
    /// 3. 默认值 `http://127.0.0.1:5555`
    pub(crate) server_url: String,

    /// 沙箱的名称
    ///
    /// 名称用于在服务器上唯一标识一个沙箱。如果创建时未指定名称，
    /// 系统会自动生成一个带随机前缀的名称，如 `sandbox-a1b2c3d4`。
    pub(crate) name: String,

    /// 用于 Microsandbox 服务器身份验证的 API 密钥
    ///
    /// API 密钥通过 `Authorization: Bearer <key>` 头发送。
    /// 可以通过以下方式设置：
    /// 1. `SandboxOptions` 中的 `api_key` 字段
    /// 2. 环境变量 `MSB_API_KEY`
    /// 3. `.env` 文件中的 `MSB_API_KEY` 配置
    ///
    /// 如果未设置 API 密钥，请求将以未认证方式发送（某些服务器可能要求认证）。
    pub(crate) api_key: Option<String>,

    /// 用于发送 HTTP 请求的 reqwest 客户端
    ///
    /// 使用 `reqwest::Client` 来发送异步 HTTP 请求。
    /// 客户端是可重用的，应该在整个应用生命周期内保持。
    pub(crate) client: reqwest::Client,

    /// 沙箱是否已启动的状态标志
    ///
    /// 这个布尔值用于跟踪沙箱的当前状态：
    /// - `true` - 沙箱已启动，可以执行代码
    /// - `false` - 沙箱未启动，需要先调用 `start_sandbox`
    pub(crate) is_started: bool,
}

impl SandboxBase {
    /// # 创建新的 SandboxBase 实例
    ///
    /// 这是创建沙箱的主要入口点。该方法会从多个来源解析配置，
    /// 优先级为：选项参数 > 环境变量 > 默认值。
    ///
    /// ## 参数
    ///
    /// * `options` - [`SandboxOptions`] 结构体，包含可选的配置项
    ///
    /// ## 配置解析流程
    ///
    /// ### 1. 加载 .env 文件
    ///
    /// 如果环境变量 `MSB_API_KEY` 未设置，会尝试加载项目根目录的 `.env` 文件。
    /// `.env` 文件格式示例：
    /// ```text
    /// MSB_SERVER_URL=https://api.example.com
    /// MSB_API_KEY=your-api-key-here
    /// ```
    ///
    /// ### 2. 服务器 URL 解析
    ///
    /// 按以下顺序查找：
    /// 1. `options.server_url`
    /// 2. 环境变量 `MSB_SERVER_URL`
    /// 3. 默认值 `http://127.0.0.1:5555`
    ///
    /// ### 3. API 密钥解析
    ///
    /// 按以下顺序查找：
    /// 1. `options.api_key`
    /// 2. 环境变量 `MSB_API_KEY`
    ///
    /// ### 4. 沙箱名称生成
    ///
    /// 如果 `options.name` 未提供，会生成一个带随机前缀的名称。
    /// 使用 UUID v4 生成随机字符串，取第一个分段（8 个字符）。
    ///
    /// ## 返回
    ///
    /// 返回一个配置好的 `SandboxBase` 实例，但沙箱容器尚未启动。
    /// 需要调用 `start_sandbox` 方法来实际启动容器。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::{SandboxBase, SandboxOptions};
    ///
    /// let options = SandboxOptions::builder()
    ///     .server_url("http://localhost:5555")
    ///     .name("my-sandbox")
    ///     .api_key("my-api-key")
    ///     .build();
    ///
    /// let base = SandboxBase::new(&options);
    /// ```
    pub fn new(options: &SandboxOptions) -> Self {
        // 如果 MSB_API_KEY 环境变量未设置，尝试加载 .env 文件
        // dotenv() 会查找当前目录及父目录中的 .env 文件
        // 如果文件不存在，返回 Err，我们使用 `let _ =` 忽略错误
        if env::var("MSB_API_KEY").is_err() {
            let _ = dotenv();
        }

        // 获取服务器 URL：从选项、环境变量或默认值
        // 使用 Option 的链式调用实现优先级选择
        let server_url = options
            .server_url
            .clone()
            .or_else(|| env::var("MSB_SERVER_URL").ok())
            .unwrap_or_else(|| "http://127.0.0.1:5555".to_string());

        // 获取 API 密钥：从选项或环境变量
        // Option::or_else 用于在第一个选项为 None 时尝试另一个来源
        let api_key = options
            .api_key
            .clone()
            .or_else(|| env::var("MSB_API_KEY").ok());

        // 如果未提供名称，生成一个随机名称
        // 使用 UUID v4 生成唯一标识符，取第一个分段作为名称后缀
        let name = options.name.clone().unwrap_or_else(|| {
            format!(
                "sandbox-{}",
                Uuid::new_v4().to_string().split('-').next().unwrap()
            )
        });

        Self {
            server_url,
            name,
            api_key,
            client: reqwest::Client::new(),
            is_started: false,
        }
    }

    /// # 向 Microsandbox 服务器发送 JSON-RPC 请求
    ///
    /// 这是一个通用的底层方法，用于发送任意 JSON-RPC 请求到服务器。
    /// 所有其他方法（启动、停止、执行代码等）都使用这个方法进行通信。
    ///
    /// ## 泛型参数
    ///
    /// * `T` - 期望的响应类型，必须实现 `serde::Deserialize` trait
    ///
    /// ## 参数
    ///
    /// * `method` - JSON-RPC 方法名，如 `"sandbox.start"`、`"sandbox.stop"`
    /// * `params` - 方法参数，使用 `serde_json::Value` 类型
    ///
    /// ## 请求流程
    ///
    /// ```text
    /// 1. 创建 HTTP 请求头
    ///    ├── Content-Type: application/json
    ///    └── Authorization: Bearer <api_key> (如果有)
    ///
    /// 2. 构建 JSON-RPC 请求体
    ///    {
    ///      "jsonrpc": "2.0",
    ///      "method": "sandbox.start",
    ///      "params": { ... },
    ///      "id": "uuid-here"
    ///    }
    ///
    /// 3. 发送 POST 请求到 <server_url>/api/v1/rpc
    ///
    /// 4. 检查响应状态
    ///    ├── 成功：解析 result 字段
    ///    └── 失败：检查 error 字段
    ///
    /// 5. 反序列化结果为类型 T
    /// ```
    ///
    /// ## 错误处理
    ///
    /// 可能返回的错误类型：
    /// - 认证失败（API 密钥无效）
    /// - 网络错误（服务器不可达）
    /// - JSON-RPC 错误（方法不存在、参数无效等）
    /// - 反序列化错误（响应格式不符合预期）
    ///
    /// ## 返回值
    ///
    /// * `Ok(T)` - 请求成功，返回反序列化后的结果
    /// * `Err(...)` - 请求失败，返回详细的错误信息
    pub(crate) async fn make_request<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, Box<dyn Error + Send + Sync>> {
        // 创建 HTTP 请求头
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // 如果有 API 密钥，添加认证头
        // Authorization: Bearer <api_key>
        if let Some(api_key) = &self.api_key {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))?,
            );
        }

        // 构建 JSON-RPC 2.0 请求体
        // json! 宏允许使用类似 JSON 的语法构建 serde_json::Value
        let request_data = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": Uuid::new_v4().to_string(),
        });

        // 发送 HTTP POST 请求
        // 请求 URL 格式：<server_url>/api/v1/rpc
        let response = self
            .client
            .post(&format!("{}/api/v1/rpc", self.server_url))
            .headers(headers)
            .json(&request_data)
            .send()
            .await?;

        // 检查 HTTP 响应状态码
        // is_success() 返回 true 表示状态码在 200-299 范围内
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(Box::new(SandboxError::RequestFailed(error_text)));
        }

        // 解析 JSON 响应
        let response_data: Value = response.json().await?;

        // 检查 JSON-RPC 错误字段
        // JSON-RPC 规范：错误响应包含 error 对象，成功响应包含 result 对象
        if let Some(error) = response_data.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            return Err(Box::new(SandboxError::ServerError(error_msg)));
        }

        // 提取并反序列化 result 字段到目标类型 T
        // get("result").cloned() 获取结果的副本，unwrap_or(Value::Null) 处理缺失情况
        let result =
            serde_json::from_value(response_data.get("result").cloned().unwrap_or(Value::Null))?;

        Ok(result)
    }

    /// # 启动沙箱容器
    ///
    /// 向 Microsandbox 服务器发送请求，创建并启动一个新的 Docker 容器。
    ///
    /// ## 参数
    ///
    /// * `image` - 可选的 Docker 镜像名称
    ///   - `Some("python:3.9")` - 使用指定的镜像
    ///   - `None` - 使用沙箱类型的默认镜像
    ///
    /// * `opts` - [`StartOptions`] 启动配置选项，包括：
    ///   - `memory` - 内存限制（MB）
    ///   - `cpus` - CPU 核心数
    ///   - `volumes` - 挂载的卷
    ///   - `ports` - 暴露的端口
    ///   - `envs` - 环境变量
    ///   - `timeout` - 启动超时时间（秒）
    ///   - 等等...
    ///
    /// ## 启动流程
    ///
    /// ```text
    /// 1. 检查沙箱是否已启动（避免重复启动）
    ///
    /// 2. 构建容器配置 JSON 对象
    ///    {
    ///      "image": "python:3.9",
    ///      "memory": 512,
    ///      "cpus": 1,
    ///      "volumes": [...],  // 可选
    ///      "ports": [...],    // 可选
    ///      ...
    ///    }
    ///
    /// 3. 构建 JSON-RPC 请求
    ///    {
    ///      "sandbox": "sandbox-name",
    ///      "config": { ... }
    ///    }
    ///
    /// 4. 创建带有自定义超时的 HTTP 客户端
    ///    - 客户端超时 = opts.timeout + 30 秒
    ///    - 额外 30 秒用于网络延迟和服务器处理
    ///
    /// 5. 发送请求并等待响应
    ///
    /// 6. 处理响应
    ///    - 检查错误
    ///    - 记录警告（如果有）
    ///
    /// 7. 更新内部状态为"已启动"
    /// ```
    ///
    /// ## 超时处理
    ///
    /// 启动容器可能需要较长时间（拉取镜像、初始化环境等）。
    /// 默认超时时间为 180 秒（3 分钟），可以通过 `StartOptions` 调整。
    ///
    /// ## 错误处理
    ///
    /// 可能遇到的错误：
    /// - [`SandboxError::Timeout`] - 启动超时
    /// - [`SandboxError::HttpError`] - HTTP 客户端错误
    /// - [`SandboxError::RequestFailed`] - 请求失败
    /// - [`SandboxError::ServerError`] - 服务器返回错误
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 启动成功
    /// * `Err(...)` - 启动失败
    ///
    /// ## 注意
    ///
    /// 此方法会阻塞直到容器完全启动或超时。对于长时间运行的操作，
    /// 建议在异步上下文中执行。
    pub async fn start_sandbox(
        &mut self,
        image: Option<String>,
        opts: &crate::StartOptions,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // 如果沙箱已经启动，直接返回成功（幂等性）
        if self.is_started {
            return Ok(());
        }

        // 构建容器配置 JSON 对象
        // 使用 json! 宏创建基础的配置结构
        let mut config = json!({
            "image": image,
            "memory": opts.memory,
            "cpus": opts.cpus.round() as i32,
        });

        // 动态添加可选配置项
        // as_object_mut() 返回可变引用，允许我们修改 JSON 对象
        if let Some(obj) = config.as_object_mut() {
            // 只添加非空的配置项，避免发送不必要的字段
            if !opts.volumes.is_empty() {
                obj.insert("volumes".to_string(), json!(opts.volumes));
            }
            if !opts.ports.is_empty() {
                obj.insert("ports".to_string(), json!(opts.ports));
            }
            if !opts.envs.is_empty() {
                obj.insert("envs".to_string(), json!(opts.envs));
            }
            if !opts.depends_on.is_empty() {
                obj.insert("depends_on".to_string(), json!(opts.depends_on));
            }
            if let Some(ref workdir) = opts.workdir {
                obj.insert("workdir".to_string(), json!(workdir));
            }
            if let Some(ref shell) = opts.shell {
                obj.insert("shell".to_string(), json!(shell));
            }
            if !opts.scripts.is_empty() {
                obj.insert("scripts".to_string(), json!(opts.scripts));
            }
            if let Some(ref exec) = opts.exec {
                obj.insert("exec".to_string(), json!(exec));
            }
        }

        // 构建完整的请求参数
        let params = json!({
            "sandbox": self.name,
            "config": config,
        });

        // 设置客户端超时时间
        // 客户端超时应该略长于服务器端超时，以便区分是服务器超时还是网络超时
        // 额外 30 秒的缓冲时间用于处理网络延迟
        let client_timeout = Duration::from_secs_f32(opts.timeout + 30.0);
        let client = reqwest::Client::builder().timeout(client_timeout).build()?;

        // 构建 JSON-RPC 请求数据
        let request_data = json!({
            "jsonrpc": "2.0",
            "method": "sandbox.start",
            "params": params,
            "id": Uuid::new_v4().to_string(),
        });

        // 创建 HTTP 请求头
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // 添加认证头（如果有 API 密钥）
        if let Some(api_key) = &self.api_key {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", api_key))?,
            );
        }

        // 发送启动请求并处理响应
        // 使用 match 表达式进行详细的错误匹配
        let response = match client
            .post(&format!("{}/api/v1/rpc", self.server_url))
            .headers(headers)
            .json(&request_data)
            .send()
            .await
        {
            // 请求成功发送
            Ok(resp) => resp,
            // 请求失败，需要区分超时和其他错误
            Err(e) => {
                if e.is_timeout() {
                    // HTTP 客户端超时（比 opts.timeout 多 30 秒）
                    return Err(Box::new(SandboxError::Timeout(format!(
                        "Timed out waiting for sandbox to start after {} seconds",
                        opts.timeout
                    ))));
                }
                // 其他 HTTP 错误（网络问题、DNS 解析失败等）
                return Err(Box::new(SandboxError::HttpError(e.to_string())));
            }
        };

        // 检查 HTTP 响应状态码
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(Box::new(SandboxError::RequestFailed(error_text)));
        }

        // 解析 JSON 响应
        let response_data: Value = response.json().await?;

        // 检查 JSON-RPC 错误
        if let Some(error) = response_data.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            return Err(Box::new(SandboxError::ServerError(error_msg)));
        }

        // 检查结果中的警告信息
        // 某些情况下服务器可能返回警告而不是错误（例如启动较慢）
        if let Some(result) = response_data.get("result") {
            if let Some(result_str) = result.as_str() {
                if result_str.contains("timed out waiting") {
                    eprintln!("Sandbox start warning: {}", result_str);
                }
            }
        }

        // 更新状态为已启动
        self.is_started = true;
        Ok(())
    }

    /// # 停止沙箱容器
    ///
    /// 向服务器发送请求，停止并清理沙箱容器。
    ///
    /// ## 幂等性
    ///
    /// 这个方法具有幂等性：
    /// - 如果沙箱未启动，直接返回 `Ok(())`
    /// - 如果沙箱已启动，发送停止请求
    ///
    /// 这意味着你可以安全地多次调用此方法，而不会出错。
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 停止成功（或沙箱本来就是停止状态）
    /// * `Err(...)` - 停止失败
    ///
    /// ## 注意
    ///
    /// 停止沙箱会：
    /// - 停止运行中的进程
    /// - 释放分配的资源
    /// - 清理临时文件
    /// - 删除容器（取决于服务器配置）
    pub async fn stop_sandbox(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // 如果沙箱未启动，直接返回成功（幂等性）
        if !self.is_started {
            return Ok(());
        }

        // 构建停止请求的参数
        let params = json!({
            "sandbox": self.name,
        });

        // 发送停止请求
        // 使用 make_request 方法，我们只关心是否成功，不关心返回结果
        let _result: Value = self.make_request("sandbox.stop", params).await?;

        // 更新状态为未启动
        self.is_started = false;

        Ok(())
    }

    /// # 在沙箱中执行代码
    ///
    /// 向服务器发送代码执行请求，并在沙箱中运行指定语言的代码。
    ///
    /// ## 参数
    ///
    /// * `language` - 编程语言标识符
    ///   - `"python"` - Python 代码
    ///   - `"javascript"` - JavaScript/Node.js 代码
    ///   - 其他支持的语言...
    ///
    /// * `code` - 要执行的源代码字符串
    ///
    /// ## 执行流程
    ///
    /// ```text
    /// 1. 检查沙箱是否已启动
    ///    └── 未启动 → 返回 NotStarted 错误
    ///
    /// 2. 构建执行参数
    ///    {
    ///      "sandbox": "sandbox-name",
    ///      "language": "python",
    ///      "code": "print('Hello')"
    ///    }
    ///
    /// 3. 发送 JSON-RPC 请求
    ///    方法：sandbox.repl.run
    ///
    /// 4. 解析响应并创建 Execution 对象
    /// ```
    ///
    /// ## 返回值
    ///
    /// * `Ok(Execution)` - 执行成功，返回执行结果
    ///   - 可以通过 `output()` 获取标准输出
    ///   - 可以通过 `error()` 获取错误输出
    ///   - 可以通过 `has_error()` 检查是否有错误
    ///
    /// * `Err(...)` - 执行失败
    ///   - [`SandboxError::NotStarted`] - 沙箱未启动
    ///   - 其他错误 - 网络或服务器错误
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::SandboxBase;
    /// # async fn example(base: &SandboxBase) -> Result<(), Box<dyn std::error::Error>> {
    /// let result = base.run_code("python", "print('Hello, World!')").await?;
    /// println!("输出：{}", result.output().await?);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn run_code(
        &self,
        language: &str,
        code: &str,
    ) -> Result<Execution, Box<dyn Error + Send + Sync>> {
        // 检查沙箱是否已启动
        if !self.is_started {
            return Err(Box::new(SandboxError::NotStarted));
        }

        // 构建代码执行参数
        let params = json!({
            "sandbox": self.name,
            "language": language,
            "code": code,
        });

        // 发送执行请求
        // sandbox.repl.run 是服务器上执行代码的方法
        // REPL = Read-Eval-Print Loop（读取 - 求值 - 输出循环）
        let result: HashMap<String, Value> = self.make_request("sandbox.repl.run", params).await?;

        // 创建并返回 Execution 对象
        Ok(Execution::new(result))
    }
}
