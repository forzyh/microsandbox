//! # 代码执行结果模块
//!
//! 这个模块定义了 `Execution` 结构体，用于表示和处理在沙箱中执行代码的结果。
//! 当你在沙箱中运行 Python、JavaScript 或其他语言的代码时，执行结果会被
//! 封装在 `Execution` 对象中返回。
//!
//! ## Execution 包含的信息
//!
//! - **输出行** - 代码执行过程中产生的所有输出（stdout 和 stderr）
//! - **状态** - 执行状态（如 "success"、"error"、"exception"）
//! - **语言** - 执行使用的编程语言
//! - **错误标志** - 是否遇到错误
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
//!     // 执行成功的代码
//!     let result = sandbox.run("print('Hello, World!')").await?;
//!     println!("输出：{}", result.output().await?);
//!     println!("状态：{}", result.status());
//!     println!("语言：{}", result.language());
//!
//!     // 执行有错误的代码
//!     let result = sandbox.run("print(undefined_variable)").await?;
//!     if result.has_error() {
//!         eprintln!("执行出错：{}", result.error().await?);
//!     }
//!
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```

use serde_json::Value;
use std::collections::HashMap;
use std::error::Error;

/// # 代码执行结果
///
/// `Execution` 结构体封装了在沙箱中执行代码的完整结果。
/// 它提供了访问输出、检查错误状态、获取执行信息等方法。
///
/// ## 数据结构
///
/// ```text
/// Execution
/// ├── output_lines: Vec<OutputLine>  // 所有输出行
/// │   ├── OutputLine { stream: "stdout", text: "Hello" }
/// │   └── OutputLine { stream: "stderr", text: "Error: ..." }
/// ├── status: String                  // 执行状态
/// ├── language: String                // 编程语言
/// └── has_error: bool                 // 是否有错误
/// ```
///
/// ## 错误检测
///
/// `Execution` 会在以下情况标记为有错误（`has_error = true`）：
///
/// 1. **状态指示错误** - `status` 为 "error" 或 "exception"
/// 2. **stderr 有输出** - 任何写入标准错误的非空内容
///
/// 注意：有些程序可能会向 stderr 写入警告信息但仍成功执行，
/// 所以 `has_error()` 返回 `true` 不一定意味着执行完全失败。
///
/// ## 字段说明
#[derive(Debug, Clone)]
pub struct Execution {
    /// 输出行列表
    ///
    /// 包含代码执行过程中产生的所有输出行，包括：
    /// - `stdout` - 标准输出（正常的 print/println 输出）
    /// - `stderr` - 标准错误（错误信息、警告、异常堆栈）
    output_lines: Vec<OutputLine>,

    /// 执行状态
    ///
    /// 可能的值：
    /// - `"success"` - 执行成功完成
    /// - `"error"` - 执行遇到错误
    /// - `"exception"` - 抛出未捕获的异常
    /// - `"timeout"` - 执行超时
    /// - `"unknown"` - 未知状态
    status: String,

    /// 执行使用的编程语言
    ///
    /// 可能的值：
    /// - `"python"` - Python 代码
    /// - `"javascript"` - JavaScript/Node.js 代码
    /// - 其他支持的语言
    language: String,

    /// 是否遇到错误
    ///
    /// 这个标志在以下情况被设置为 `true`：
    /// 1. `status` 为 "error" 或 "exception"
    /// 2. 有任何非空的 stderr 输出
    has_error: bool,
}

/// # 单行输出
///
/// `OutputLine` 表示执行输出的一行，包含流类型和文本内容。
///
/// ## 流类型说明
///
/// ### stdout（标准输出）
///
/// - 正常的程序输出
/// - `print()`、`println!()` 等的输出
/// - 命令的正常结果
///
/// ### stderr（标准错误）
///
/// - 错误消息
/// - 异常堆栈跟踪
/// - 警告信息
/// - 诊断输出
///
/// ## 设计说明
///
/// 这个结构体是私有的（没有 `pub` 修饰符），只在 crate 内部使用。
/// 外部用户通过 `Execution` 提供的方法访问输出内容。
#[derive(Debug, Clone)]
struct OutputLine {
    /// 流类型标识符
    ///
    /// 可能的值：
    /// - `"stdout"` - 标准输出
    /// - `"stderr"` - 标准错误
    stream: String,

    /// 输出的文本内容
    ///
    /// 注意：不包含换行符，换行符在解析时被移除
    text: String,
}

impl Execution {
    /// # 创建 Execution 实例
    ///
    /// 这是一个内部方法，从服务器返回的 JSON 数据构建 `Execution` 实例。
    ///
    /// ## 参数
    ///
    /// * `output_data` - 服务器返回的 HashMap，包含以下字段：
    ///   - `status`: 执行状态字符串
    ///   - `language`: 编程语言标识符
    ///   - `output`: 输出数组，每个元素包含 `stream` 和 `text`
    ///
    /// ## 错误检测逻辑
    ///
    /// ```text
    /// 1. 检查 status 字段
    ///    ├── "error" → has_error = true
    ///    ├── "exception" → has_error = true
    ///    └── 其他 → 继续检查
    ///
    /// 2. 遍历输出行
    ///    └── 如果 stream == "stderr" 且 text 非空
    ///        → has_error = true
    /// ```
    ///
    /// ## 容错处理
    ///
    /// 这个方法对缺失字段很宽容：
    /// - 如果 `status` 不存在，默认为 "unknown"
    /// - 如果 `language` 不存在，默认为 "unknown"
    /// - 如果 `output` 不存在或为空，`output_lines` 为空向量
    pub(crate) fn new(output_data: HashMap<String, Value>) -> Self {
        let mut output_lines = Vec::new();
        let mut has_error = false;

        // 提取执行状态
        // and_then(|v| v.as_str()) 用于安全地将 JSON 值转换为字符串
        let status = output_data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 提取编程语言
        let language = output_data
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 检查状态是否指示错误
        // "error" 和 "exception" 都表示执行失败
        if status == "error" || status == "exception" {
            has_error = true;
        }

        // 解析输出行
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

                        // 检查是否有 stderr 输出（表示可能的错误）
                        // 注意：有些程序会向 stderr 写入警告，不一定是致命错误
                        if stream == "stderr" && !text.is_empty() {
                            has_error = true;
                        }

                        output_lines.push(OutputLine { stream, text });
                    }
                }
            }
        }

        Self {
            output_lines,
            status,
            language,
            has_error,
        }
    }

    /// # 获取标准输出
    ///
    /// 收集并返回代码执行的标准输出（stdout）。
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
    /// let mut sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let result = sandbox.run("print('Line 1')\nprint('Line 2')").await?;
    /// let output = result.output().await?;
    ///
    /// assert_eq!(output, "Line 1\nLine 2");
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

    /// # 获取错误输出
    ///
    /// 收集并返回代码执行的错误输出（stderr）。
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
    /// ## 使用场景
    ///
    /// - 调试执行失败的原因
    /// - 获取异常堆栈跟踪
    /// - 查看编译器/解释器错误消息
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let result = sandbox.run("raise ValueError('Something went wrong')").await?;
    ///
    /// if result.has_error() {
    ///     eprintln!("执行出错：\n{}", result.error().await?);
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

    /// # 检查执行是否包含错误
    ///
    /// 返回执行过程中是否遇到了错误。
    ///
    /// ## 返回值
    ///
    /// * `true` - 执行遇到错误（状态为 error/exception 或有 stderr 输出）
    /// * `false` - 执行成功，没有错误
    ///
    /// ## 判断依据
    ///
    /// 这个方法在以下情况返回 `true`：
    /// 1. `status` 字段为 "error" 或 "exception"
    /// 2. 有任何非空的 stderr 输出
    ///
    /// ## 注意事项
    ///
    /// - `has_error() == true` 不一定意味着程序完全失败
    /// - 有些程序会向 stderr 写入警告信息但仍成功完成
    /// - 对于严格的错误检查，建议同时检查 `status()`
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let result = sandbox.run("print('Hello')").await?;
    ///
    /// if result.has_error() {
    ///     eprintln!("执行失败：{}", result.error().await?);
    /// } else {
    ///     println!("执行成功：{}", result.output().await?);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// # 获取执行状态
    ///
    /// 返回代码执行的状态。
    ///
    /// ## 返回值
    ///
    /// 返回状态字符串的切片引用，可能的值：
    /// - `"success"` - 执行成功完成
    /// - `"error"` - 执行遇到错误
    /// - `"exception"` - 抛出未捕获的异常
    /// - `"timeout"` - 执行超时
    /// - `"unknown"` - 未知状态
    ///
    /// ## 与 `has_error()` 的区别
    ///
    /// - `status()` 返回具体的状态字符串，适合详细判断
    /// - `has_error()` 返回简单的布尔值，适合快速检查
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut sandbox = PythonSandbox::create("test").await?;
    /// # sandbox.start(None).await?;
    /// let result = sandbox.run("print('Hello')").await?;
    ///
    /// match result.status() {
    ///     "success" => println!("执行成功"),
    ///     "error" => eprintln!("执行错误：{}", result.error().await?),
    ///     "exception" => eprintln!("异常：{}", result.error().await?),
    ///     "timeout" => eprintln!("执行超时"),
    ///     _ => eprintln!("未知状态"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn status(&self) -> &str {
        &self.status
    }

    /// # 获取编程语言
    ///
    /// 返回执行所使用的编程语言标识符。
    ///
    /// ## 返回值
    ///
    /// 返回语言字符串的切片引用，例如：
    /// - `"python"` - Python
    /// - `"javascript"` - JavaScript/Node.js
    /// - `"unknown"` - 未知语言
    ///
    /// ## 使用场景
    ///
    /// - 日志记录
    /// - 调试多语言执行环境
    /// - 验证执行结果
    pub fn language(&self) -> &str {
        &self.language
    }
}
