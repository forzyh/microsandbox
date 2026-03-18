//! # 命令执行模块
//!
//! 这个模块提供了在沙箱中执行 shell 命令的功能。通过 `Command` 结构体，
//! 你可以在隔离的沙箱环境中运行各种系统命令，比如：
//!
//! - 文件操作：`ls`、`cp`、`mv`、`rm`
//! - 文本处理：`cat`、`grep`、`sed`、`awk`
//! - 网络工具：`curl`、`wget`、`ping`
//! - 包管理：`pip`、`npm`、`apt`
//! - 以及其他任何沙箱镜像中可用的命令
//!
//! ## 安全考虑
//!
//! 在沙箱中执行命令是相对安全的，因为：
//! 1. 命令在隔离的 Docker 容器中运行
//! 2. 容器无法访问宿主机的文件系统
//! 3. 容器的网络访问可以被限制
//! 4. 资源使用（CPU、内存）受到限制
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_sdk::{PythonSandbox, BaseSandbox};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let sandbox = PythonSandbox::create("test").await?;
//!     sandbox.start(None).await?;
//!
//!     // 获取命令接口
//!     let command = sandbox.command().await?;
//!
//!     // 执行简单命令
//!     let result = command.run("ls", Some(vec!["-la"]), None).await?;
//!     println!("目录内容:\n{}", result.output().await?);
//!
//!     // 执行带超时的命令
//!     let result = command.run("sleep", Some(vec!["5"]), Some(2)).await;
//!     // 这会超时，因为 sleep 5 需要 5 秒，但超时设置为 2 秒
//!
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```

use serde_json::Value;
use std::{collections::HashMap, error::Error, sync::Arc};

use tokio::sync::Mutex;

use crate::{SandboxBase, SandboxError};

/// # 命令执行结果
///
/// `CommandExecution` 结构体封装了一次命令执行的所有信息，包括：
/// - 执行的命令和参数
/// - 退出码
/// - 标准输出和标准错误
/// - 是否成功
///
/// ## 设计说明
///
/// 这个结构体是不可变的（所有字段都是私有的，没有 setter 方法），
/// 这是为了保证数据的完整性。一旦创建，执行结果就不会改变。
///
/// ## 字段说明
#[derive(Debug, Clone)]
pub struct CommandExecution {
    /// 执行的命令名称
    ///
    /// 例如：`"ls"`、`"python"`、`"curl"`
    command: String,

    /// 传递给命令的参数列表
    ///
    /// 例如，对于 `ls -la /home`：
    /// ```rust,ignore
    /// args = vec!["-la", "/home"]
    /// ```
    args: Vec<String>,

    /// 命令的退出码
    ///
    /// 退出码的含义：
    /// - `0` - 成功执行
    /// - `1-255` - 各种错误（具体含义取决于命令）
    /// - `-1` - 未知错误或未正常退出
    exit_code: i32,

    /// 命令是否成功执行
    ///
    /// 这个布尔值根据退出码判断：
    /// - `true` - 退出码为 0
    /// - `false` - 退出码非 0
    success: bool,

    /// 输出行列表
    ///
    /// 包含命令执行过程中产生的所有输出，包括 stdout 和 stderr
    output_lines: Vec<OutputLine>,
}

/// # 单行输出
///
/// `OutputLine` 表示命令输出的一行，包含流类型和文本内容。
///
/// ## 流类型
///
/// - `"stdout"` - 标准输出，通常是命令的正常输出
/// - `"stderr"` - 标准错误，通常是错误信息和警告
///
/// ## 设计说明
///
/// 这个结构体是私有的（没有 `pub` 修饰符），只在 crate 内部使用。
/// 外部用户通过 `CommandExecution` 提供的方法访问输出内容。
#[derive(Debug, Clone)]
struct OutputLine {
    /// 流类型标识符
    ///
    /// 可能的值：
    /// - `"stdout"` - 标准输出
    /// - `"stderr"` - 标准错误
    stream: String,
    /// 输出的文本内容（不包含换行符）
    text: String,
}

impl CommandExecution {
    /// # 创建命令执行结果
    ///
    /// 这是一个内部方法，从服务器返回的 JSON 数据构建 `CommandExecution` 实例。
    ///
    /// ## 参数
    ///
    /// * `output_data` - 服务器返回的 HashMap，包含以下字段：
    ///   - `command`: 命令名称
    ///   - `args`: 参数数组
    ///   - `exit_code`: 退出码
    ///   - `success`: 是否成功
    ///   - `output`: 输出数组，每个元素包含 `stream` 和 `text`
    ///
    /// ## 数据处理流程
    ///
    /// ```text
    /// 1. 提取 command 字段
    /// 2. 提取 args 数组（如果存在）
    /// 3. 提取 exit_code（如果存在，否则默认为 -1）
    /// 4. 提取 success 标志（如果存在，否则默认为 false）
    /// 5. 解析 output 数组，构建 OutputLine 列表
    /// 6. 返回完整的 CommandExecution 实例
    /// ```
    ///
    /// ## 错误处理
    ///
    /// 这个方法对缺失字段很宽容：
    /// - 如果某个字段不存在，使用合理的默认值
    /// - 如果类型不匹配，忽略该字段
    /// - 这确保了即使服务器返回部分数据，也不会崩溃
    fn new(output_data: HashMap<String, Value>) -> Self {
        // 提取命令名称
        // and_then(|v| v.as_str()) 用于将 JSON 值转换为字符串
        let command = output_data
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // 提取参数数组
        // 这是一个嵌套的 Option 处理，确保类型正确
        let args = if let Some(args_val) = output_data.get("args") {
            if let Some(args_arr) = args_val.as_array() {
                // 将 JSON 数组转换为 Rust Vec<String>
                args_arr
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                // 如果不是数组，返回空向量
                Vec::new()
            }
        } else {
            // 如果不存在，返回空向量
            Vec::new()
        };

        // 提取退出码
        // 注意：JSON 中的数字是 i64，需要转换为 i32
        let exit_code = output_data
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1) as i32;

        // 提取成功标志
        let success = output_data
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // 解析输出行
        let mut output_lines = Vec::new();
        if let Some(output) = output_data.get("output") {
            if let Some(lines) = output.as_array() {
                // 遍历每一行输出
                for line in lines {
                    if let Some(line_obj) = line.as_object() {
                        // 提取 stream 和 text 字段
                        let stream = line_obj
                            .get("stream")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let text = line_obj
                            .get("text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        output_lines.push(OutputLine { stream, text });
                    }
                }
            }
        }

        Self {
            command,
            args,
            exit_code,
            success,
            output_lines,
        }
    }

    /// # 获取执行的命令
    ///
    /// 返回执行的命令名称。
    ///
    /// ## 返回值
    ///
    /// 返回命令名称的字符串切片，例如 `"ls"` 或 `"python"`。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// # use microsandbox_sdk::CommandExecution;
    /// # fn example(exec: &CommandExecution) {
    /// println!("执行的命令：{}", exec.command());
    /// # }
    /// ```
    pub fn command(&self) -> &str {
        &self.command
    }

    /// # 获取命令参数
    ///
    /// 返回传递给命令的参数列表。
    ///
    /// ## 返回值
    ///
    /// 返回参数切片的引用，例如 `&["-la", "/home"]`。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// # use microsandbox_sdk::CommandExecution;
    /// # fn example(exec: &CommandExecution) {
    /// for arg in exec.args() {
    ///     println!("参数：{}", arg);
    /// }
    /// # }
    /// ```
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// # 获取退出码
    ///
    /// 返回命令的退出码。
    ///
    /// ## 返回值
    ///
    /// * `0` - 成功执行
    /// * `1-255` - 错误码（具体含义取决于命令）
    /// * `-1` - 未知错误
    ///
    /// ## 常见退出码
    ///
    /// | 退出码 | 含义 |
    /// |--------|------|
    /// | 0 | 成功 |
    /// | 1 | 一般错误 |
    /// | 2 | 用法错误（命令行参数问题） |
    /// | 126 | 命令不可执行 |
    /// | 127 | 命令未找到 |
    /// | 130 | 被 Ctrl+C 终止 |
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    /// # 获取标准输出
    ///
    /// 收集并返回命令的标准输出（stdout）。
    ///
    /// ## 返回值
    ///
    /// * `Ok(String)` - 所有 stdout 输出连接成的字符串
    /// * `Err(...)` - 处理输出时出错
    ///
    /// ## 处理逻辑
    ///
    /// 1. 遍历所有输出行
    /// 2. 筛选出 `stream == "stdout"` 的行
    /// 3. 将这些行连接起来，每行后添加换行符
    /// 4. 移除末尾的换行符（如果存在）
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let command = sandbox.command().await?;
    /// let result = command.run("echo", Some(vec!["Hello"]), None).await?;
    ///
    /// println!("输出：{}", result.output().await?);
    /// // 输出：Hello
    /// # Ok(())
    /// # }
    /// ```
    pub async fn output(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut output_text = String::new();

        // 收集所有 stdout 行
        for line in &self.output_lines {
            if line.stream == "stdout" {
                output_text.push_str(&line.text);
                output_text.push('\n');
            }
        }

        // 移除末尾的换行符（如果存在）
        // pop() 移除并返回最后一个字符
        if output_text.ends_with('\n') {
            output_text.pop();
        }

        Ok(output_text)
    }

    /// # 获取标准错误
    ///
    /// 收集并返回命令的标准错误（stderr）。
    ///
    /// ## 返回值
    ///
    /// * `Ok(String)` - 所有 stderr 输出连接成的字符串
    /// * `Err(...)` - 处理输出时出错
    ///
    /// ## 处理逻辑
    ///
    /// 与 `output()` 方法类似，但筛选 `stream == "stderr"` 的行。
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let command = sandbox.command().await?;
    /// let result = command.run("ls", Some(vec!["/nonexistent"]), None).await?;
    ///
    /// if !result.is_success() {
    ///     eprintln!("错误：{}", result.error().await?);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn error(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut error_text = String::new();

        // 收集所有 stderr 行
        for line in &self.output_lines {
            if line.stream == "stderr" {
                error_text.push_str(&line.text);
                error_text.push('\n');
            }
        }

        // 移除末尾的换行符（如果存在）
        if error_text.ends_with('\n') {
            error_text.pop();
        }

        Ok(error_text)
    }

    /// # 检查命令是否成功
    ///
    /// 返回命令是否成功执行（退出码是否为 0）。
    ///
    /// ## 返回值
    ///
    /// * `true` - 退出码为 0，命令成功
    /// * `false` - 退出码非 0，命令失败
    ///
    /// ## 与 `exit_code()` 的区别
    ///
    /// - `is_success()` 返回简单的布尔值，适合条件判断
    /// - `exit_code()` 返回具体的退出码，适合详细错误处理
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let command = sandbox.command().await?;
    /// let result = command.run("test", Some(vec!["-f", "/some/file"]), None).await?;
    ///
    /// if result.is_success() {
    ///     println!("文件存在");
    /// } else {
    ///     println!("文件不存在");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn is_success(&self) -> bool {
        self.success
    }
}

/// # 命令接口
///
/// `Command` 结构体提供了在沙箱中执行 shell 命令的能力。
/// 它是 `SandboxBase` 的封装，专门处理命令执行相关的功能。
///
/// ## 设计说明
///
/// `Command` 使用 `Arc<Mutex<SandboxBase>>` 来共享对底层沙箱的访问：
///
/// - `Arc` (Atomic Reference Counting) 允许多个所有者共享所有权
/// - `Mutex` 确保同一时间只有一个线程可以访问沙箱
/// - 这种组合在异步 Rust 中很常见
///
/// ## 为什么需要 Arc<Mutex<...>>？
///
/// 因为：
/// 1. 多个 `Command` 实例可能同时存在
/// 2. 每个实例都需要同步访问同一个沙箱
/// 3. 沙箱操作是异步的，需要 `.await`
///
/// ```text
/// ┌─────────────────┐
/// │  PythonSandbox  │
/// └────────┬────────┘
///          │
///          │ Arc<Mutex<...>>
///          │
///          ▼
/// ┌─────────────────┐
/// │  SandboxBase    │
/// └─────────────────┘
///          ▲
///          │
///          │ Arc<Mutex<...>>
///          │
/// ┌────────┴────────┐
/// │    Command      │
/// └─────────────────┘
/// ```
pub struct Command {
    /// 对底层沙箱的共享引用
    sandbox: Arc<Mutex<SandboxBase>>,
}

impl Command {
    /// # 创建新的 Command 实例
    ///
    /// 这是一个内部方法，通常通过沙箱的 `command()` 方法获取 `Command` 实例。
    ///
    /// ## 参数
    ///
    /// * `sandbox` - 沙箱的共享引用
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `Command` 实例。
    pub(crate) fn new(sandbox: Arc<Mutex<SandboxBase>>) -> Self {
        Self { sandbox }
    }

    /// # 在沙箱中执行 shell 命令
    ///
    /// 这是 `Command` 的核心方法，用于在沙箱中运行命令。
    ///
    /// ## 参数
    ///
    /// * `command` - 要执行的命令名称
    ///   - 例如：`"ls"`、`"python"`、`"curl"`
    ///
    /// * `args` - 命令的参数列表（可选）
    ///   - `Some(vec!["-la", "/home"])` - 带参数
    ///   - `None` - 无参数
    ///   - `Some(vec![])` - 空参数列表
    ///
    /// * `timeout` - 执行超时时间（秒，可选）
    ///   - `Some(30)` - 30 秒超时
    ///   - `None` - 使用服务器默认超时
    ///
    /// ## 返回值
    ///
    /// * `Ok(CommandExecution)` - 命令执行完成，返回结果
    /// * `Err(...)` - 执行失败，可能的错误：
    ///   - [`SandboxError::NotStarted`] - 沙箱未启动
    ///   - 网络错误 - 无法连接到服务器
    ///   - 服务器错误 - 命令执行失败
    ///
    /// ## 执行流程
    ///
    /// ```text
    /// 1. 检查沙箱是否已启动
    ///    └── 未启动 → 返回 NotStarted 错误
    ///
    /// 2. 准备参数
    ///    - 将 &str 参数转换为 String
    ///
    /// 3. 构建 JSON-RPC 请求
    ///    {
    ///      "sandbox": "sandbox-name",
    ///      "command": "ls",
    ///      "args": ["-la"],
    ///      "timeout": 30  // 可选
    ///    }
    ///
    /// 4. 发送请求到服务器
    ///    方法：sandbox.command.run
    ///
    /// 5. 解析响应并创建 CommandExecution
    /// ```
    ///
    /// ## 使用示例
    ///
    /// ### 执行简单命令
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let command = sandbox.command().await?;
    /// let result = command.run("pwd", None, None).await?;
    /// println!("当前目录：{}", result.output().await?);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ### 执行带参数的命令
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// # let command = sandbox.command().await?;
    /// let result = command.run(
    ///     "find",
    ///     Some(vec![".", "-name", "*.rs"]),
    ///     None
    /// ).await?;
    /// println!("Rust 文件:\n{}", result.output().await?);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ### 执行带超时的命令
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// # let command = sandbox.command().await?;
    /// // 设置 5 秒超时
    /// let result = command.run("sleep", Some(vec!["10"]), Some(5)).await;
    /// // 这应该会超时...
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 安全提示
    ///
    /// 虽然沙箱提供了隔离，但仍需注意：
    /// - 不要执行来自不可信来源的命令
    /// - 注意命令注入攻击（例如，将用户输入直接拼接到命令中）
    /// - 使用参数数组而不是字符串拼接
    pub async fn run(
        &self,
        command: &str,
        args: Option<Vec<&str>>,
        timeout: Option<i32>,
    ) -> Result<CommandExecution, Box<dyn Error + Send + Sync>> {
        // 检查沙箱是否已启动
        // 使用块作用域来获取锁，锁会在块结束时自动释放
        let is_started = {
            let base = self.sandbox.lock().await;
            base.is_started
        };

        // 如果沙箱未启动，返回错误
        if !is_started {
            return Err(Box::new(SandboxError::NotStarted));
        }

        // 将参数从 &str 转换为 String
        // unwrap_or_default() 在 args 为 None 时返回空向量
        let args_vec = args
            .unwrap_or_default()
            .iter()
            .map(|&s| s.to_string())
            .collect::<Vec<_>>();

        // 获取沙箱名称
        let name = {
            let base = self.sandbox.lock().await;
            base.name.clone()
        };

        // 构建 JSON-RPC 请求参数
        let mut params = serde_json::json!({
            "sandbox": name,
            "command": command,
            "args": args_vec,
        });

        // 如果指定了超时，添加到参数中
        if let Some(t) = timeout {
            params["timeout"] = serde_json::json!(t);
        }

        // 执行命令
        // 使用 make_request 发送 JSON-RPC 请求
        let base = self.sandbox.lock().await;
        let result: HashMap<String, Value> =
            base.make_request("sandbox.command.run", params).await?;

        // 创建并返回 CommandExecution 实例
        Ok(CommandExecution::new(result))
    }
}
