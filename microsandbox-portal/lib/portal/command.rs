//! # 命令执行模块 (Command Execution)
//!
//! 本模块提供在沙箱环境中执行系统命令的功能。
//!
//! ## 主要功能
//!
//! 本模块处理：
//! - 使用 `tokio::process::Command` 派生和管理命令进程
//! - 实时流式传输 stdout 和 stderr 输出
//! - 管理命令生命周期和终止
//! - 为系统命令提供安全的执行环境
//!
//! ## 架构设计
//!
//! 架构遵循与代码评估系统类似的模式：
//!
//! ```text
//! ┌─────────────────┐
//! │  CommandHandle  │  ← 客户端接口
//! └─────────────────┘
//!         │
//!         │ CommandRequest
//!         ▼
//! ┌─────────────────┐
//! │  后台处理任务   │
//! └─────────────────┘
//!         │
//!         │ 派生命令
//!         ▼
//! ┌─────────────────┐
//! │ tokio::process  │  ← 实际进程执行
//! └─────────────────┘
//! ```
//!
//! ### 执行流程
//!
//! 1. **命令处理接收请求**: 通过 channel 接收执行请求
//! 2. **在受控环境中执行**: 使用 tokio::process 派生子进程
//! 3. **流式返回输出**: 通过 channel 将输出发送回调用者
//!
//! ## 安全考虑
//!
//! 所有命令都使用仔细控制的权限和环境变量执行，以维护系统安全。
//! 命令执行是隔离的，以防止对宿主系统造成损害。
//!
//! ## 与 REPL 引擎的区别
//!
//! | 特性 | REPL 引擎 | 命令执行 |
//! |------|-----------|----------|
//! | 用途 | 交互式代码执行 | 系统命令执行 |
//! | 进程模型 | 长生命周期 | 短生命周期 |
//! | 状态保持 | 支持 | 不支持 |
//! | 语言 | Python/Node.js | 任意系统命令 |

use std::{
    fmt,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{
        mpsc::{self, Sender},
        oneshot,
    },
    time::{Duration, sleep},
};
use uuid::Uuid;

use crate::portal::repl::types::Stream;

//--------------------------------------------------------------------------------------------------
// 类型 (Types)
//--------------------------------------------------------------------------------------------------

/// # 命令操作错误类型
///
/// 此枚举封装了在命令操作期间可能发生的各种错误。
///
/// ## 变体说明
///
/// ### SpawnError(String)
/// 派生命令时发生的错误：
/// - 命令不存在（不在 PATH 中）
/// - 权限不足
/// - 资源不足（如文件描述符耗尽）
///
/// ### ExecutionError(String)
/// 命令执行期间的错误：
/// - 无法捕获 stdout/stderr
/// - 进程意外终止
/// - 通信错误
///
/// ### Timeout(u64)
/// 执行超时：
/// - 包含超时的秒数
/// - 当命令执行时间超过指定的 timeout 时返回
///
/// ### Unavailable(String)
/// 命令环境不可用：
/// - 命令执行器未初始化
/// - channel 已关闭
///
/// ## thiserror 使用说明
///
/// `#[derive(Error)]` 自动实现 `std::error::Error` trait。
/// `#[error("...")]` 属性定义 `Display` 实现，用于格式化错误消息。
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// 派生错误
    ///
    /// 包含命令派生失败的详细原因
    #[error("Failed to spawn command: {0}")]
    SpawnError(String),

    /// 执行错误
    ///
    /// 包含执行失败的详细原因
    #[error("Command execution error: {0}")]
    ExecutionError(String),

    /// 超时错误
    ///
    /// 包含超时的秒数
    #[error("Command timeout after {0} seconds")]
    Timeout(u64),

    /// 不可用错误
    ///
    /// 包含环境不可用的原因
    #[error("Command environment unavailable: {0}")]
    Unavailable(String),
}

/// # 命令行输出行
///
/// 表示命令执行的单行输出。
///
/// ## 字段说明
///
/// * `stream` - 输出来源（stdout 或 stderr）
/// * `text` - 行的实际内容
///
/// ## 与 Line 的关系
///
/// `CommandLine` 与 REPL 模块中的 `Line` 结构类似，
/// 但专用于命令执行模块，以保持模块独立性。
#[derive(Debug, Clone)]
pub struct CommandLine {
    /// 流类型（标准输出或标准错误）
    pub stream: Stream,

    /// 行的文本内容
    pub text: String,
}

/// # 命令执行响应
///
/// 表示命令执行过程中的各种响应类型。
///
/// ## 变体说明
///
/// ### Line
/// 一行输出：
/// - `id`: 执行标识符
/// - `stream`: 流类型
/// - `text`: 行内容
///
/// ### Done
/// 执行完成：
/// - `id`: 执行标识符
/// - `exit_code`: 进程退出码（0 表示成功）
///
/// ### Error
/// 执行错误：
/// - `id`: 执行标识符
/// - `message`: 错误描述
///
/// ## 响应流程
///
/// 典型的命令执行响应序列：
///
/// ```text
/// 命令执行开始
///     │
///     ▼
/// CommandResp::Line { text: "output 1" }
/// CommandResp::Line { text: "output 2" }
/// ...
///     │
///     ▼
/// CommandResp::Done { exit_code: 0 }  ← 成功
/// 或
/// CommandResp::Error { message: "..." }  ← 失败
/// ```
#[derive(Debug)]
pub enum CommandResp {
    /// 输出一行
    Line {
        /// 执行的唯一标识符
        id: String,

        /// 流类型（标准输出或标准错误）
        stream: Stream,

        /// 行的文本内容
        text: String,
    },

    /// 执行完成
    Done {
        /// 执行的唯一标识符
        id: String,

        /// 进程的退出码
        /// - 0: 通常表示成功
        /// - 非 0: 表示错误，具体值由命令定义
        exit_code: i32,
    },

    /// 执行错误
    Error {
        /// 执行的唯一标识符
        id: String,

        /// 错误消息
        message: String,
    },
}

/// # 命令执行器句柄
///
/// 这是客户端在受控环境中执行系统命令的主要接口。
///
/// ## 设计说明
///
/// `CommandHandle` 是一个轻量级的句柄，包含一个命令发送器。
/// 它通过与后台处理任务的 channel 进行通信。
///
/// ## 字段说明
///
/// * `cmd_sender` - 发送到处理任务的命令通道发送端
///   - 私有的，外部代码不能直接发送命令
///   - 必须通过 `CommandHandle::execute()` 方法
///
/// ## Clone 行为
///
/// 派生的 `Clone` 实现：
/// - 克隆 `Sender` 会增加 channel 的引用计数
/// - 多个句柄可以发送到同一个处理任务
/// - 允许在多个任务间共享命令执行器
///
/// ## 使用示例
///
/// ```rust,no_run
/// let handle = CommandHandle::new();
///
/// // 执行命令
/// let (exit_code, output) = handle
///     .execute("ls", vec!["-la".to_string()], Some(30))
///     .await?;
/// ```
#[derive(Clone)]
pub struct CommandHandle {
    /// 命令通道发送端 - 私有
    cmd_sender: Sender<CommandRequest>,
}

// CommandHandle 的 Debug 实现
// 由于 cmd_sender 是 tokio channel，不能直接打印，所以显示占位符
impl fmt::Debug for CommandHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandHandle")
            .field("cmd_sender", &"<SENDER>")  // 占位符
            .finish()
    }
}

/// # 命令执行请求
///
/// 内部结构，表示对命令执行的请求。
///
/// ## 字段说明
///
/// * `id` - 执行的唯一标识符
/// * `command` - 要执行的命令
/// * `args` - 命令参数列表
/// * `resp_tx` - 发送响应的通道
/// * `done_tx` - 发送最终结果的 oneshot 通道
/// * `timeout` - 可选的超时时间（秒）
///
/// ## oneshot 通道说明
///
/// `oneshot::Sender` 是只能发送一次的通道：
/// - 用于发送最终结果（退出码或错误）
/// - 与 `mpsc` 通道不同，`oneshot` 保证只发送一次
/// - 适合用于返回单一结果的场景
struct CommandRequest {
    id: String,
    command: String,
    args: Vec<String>,
    resp_tx: Sender<CommandResp>,
    done_tx: oneshot::Sender<Result<i32, CommandError>>,
    timeout: Option<u64>,
}

//--------------------------------------------------------------------------------------------------
// 方法 (Methods)
//--------------------------------------------------------------------------------------------------

/// Default trait 实现
///
/// 允许使用 `CommandHandle::default()` 创建新实例。
/// 实际上调用 `CommandHandle::new()`。
impl Default for CommandHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandHandle {
    /// # 创建新的命令执行器句柄
    ///
    /// 此构造函数创建一个新的命令执行器，并在后台启动处理任务。
    ///
    /// ## 工作原理
    ///
    /// ```text
    /// CommandHandle::new()
    ///     │
    ///     ▼
    /// 1. 创建命令通道 (cmd_sender, cmd_receiver)
    ///     │
    ///     ▼
    /// 2. 生成后台处理任务 (tokio::spawn)
    ///     │
    ///     ├─→ 监听命令请求
    ///     │
    ///     └─→ 为每个命令生成单独的执行任务
    ///     │
    ///     ▼
    /// 3. 返回 CommandHandle { cmd_sender }
    /// ```
    ///
    /// ## 后台处理任务
    ///
    /// 后台任务持续监听命令请求：
    /// 1. 从通道接收 `CommandRequest`
    /// 2. 为每个请求生成新的任务执行命令
    /// 3. 允许多个命令并发执行
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `CommandHandle` 实例，可用于执行命令。
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// let handle = CommandHandle::new();
    ///
    /// // 执行命令
    /// let (exit_code, output) = handle
    ///     .execute("echo", vec!["Hello".to_string()], None)
    ///     .await?;
    /// ```
    pub fn new() -> Self {
        // ================================================================================
        // 步骤 1: 创建命令通道
        // ================================================================================
        // cmd_sender: 用于发送命令请求
        // cmd_receiver: 后台任务用于接收命令请求
        // 通道容量为 100，提供背压防止请求过多
        let (cmd_sender, mut cmd_receiver) = mpsc::channel::<CommandRequest>(100);

        // ================================================================================
        // 步骤 2: 启动后台命令执行任务
        // ================================================================================
        // tokio::spawn 创建异步任务在后台运行
        tokio::spawn(async move {
            // 持续监听命令请求，直到通道关闭
            while let Some(req) = cmd_receiver.recv().await {
                // 解构请求以获取各个字段
                let CommandRequest {
                    id,
                    command,
                    args,
                    resp_tx,
                    done_tx,
                    timeout,
                } = req;

                // ========================================================================
                // 步骤 2.1: 为每个命令生成单独的执行任务
                // ========================================================================
                // 在单独任务中执行命令，允许并发执行多个命令
                // move 关键字将变量所有权转移到闭包中
                tokio::spawn(async move {
                    // 执行实际命令
                    let result = execute_command(id, command, args, resp_tx.clone(), timeout).await;
                    // 通过 oneshot 通道发送最终结果
                    let _ = done_tx.send(result);
                });
            }
            // 当 cmd_receiver 关闭时（所有发送端都丢弃），循环结束
        });

        // ================================================================================
        // 步骤 3: 返回句柄
        // ================================================================================
        // 只返回发送端，接收端由后台任务持有
        Self { cmd_sender }
    }

    /// # 执行命令并流式传输输出
    ///
    /// 此方法执行指定的系统命令，并通过通道流式传输输出。
    ///
    /// ## 参数说明
    ///
    /// * `command` - 要执行的命令（任何可转换为 String 的类型）
    ///   - 如 `"ls"`, `"echo"`, `"python"` 等
    /// * `args` - 命令参数列表
    ///   - 每个参数是独立的字符串
    ///   - 命令本身不包含在 args 中
    /// * `timeout` - 可选的超时时间（秒）
    ///   - `Some(n)`: n 秒后终止命令
    ///   - `None`: 不设置超时
    ///
    /// ## 泛型说明
    ///
    /// ```rust
    /// S: Into<String>
    /// ```
    /// 允许 `command` 是 `&str` 或 `String`。
    ///
    /// ## 返回值
    ///
    /// 返回元组 `(exit_code, Vec<CommandLine>)`：
    /// - `exit_code`: 进程退出码（0 表示成功）
    /// - `Vec<CommandLine>`: 所有输出行的向量
    ///
    /// ## 错误情况
    ///
    /// - `CommandError::SpawnError`: 命令不存在或无法派生
    /// - `CommandError::ExecutionError`: 执行期间错误
    /// - `CommandError::Timeout`: 执行超时
    /// - `CommandError::Unavailable`: 命令执行器不可用
    ///
    /// ## 执行流程
    ///
    /// 1. 生成唯一的执行 ID
    /// 2. 创建通信通道
    /// 3. 发送命令请求到后台任务
    /// 4. 在单独任务中处理响应
    /// 5. 收集所有输出行
    /// 6. 等待执行完成并返回结果
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// let handle = CommandHandle::new();
    ///
    /// // 执行 ls -la
    /// let (exit_code, output) = handle
    ///     .execute("ls", vec!["-la".to_string()], Some(30))
    ///     .await?;
    ///
    /// println!("退出码：{}", exit_code);
    /// for line in output {
    ///     println!("[{:?}] {}", line.stream, line.text);
    /// }
    /// ```
    pub async fn execute<S: Into<String>>(
        &self,
        command: S,
        args: Vec<String>,
        timeout: Option<u64>,
    ) -> Result<(i32, Vec<CommandLine>), CommandError> {
        // 将 command 转换为 String
        let command = command.into();

        // ================================================================================
        // 步骤 1: 生成唯一的执行 ID
        // ================================================================================
        // 使用 UUID v4 生成唯一标识符
        // UUID 用于调试和日志记录，帮助追踪特定执行
        let execution_id = Uuid::new_v4().to_string();

        // ================================================================================
        // 步骤 2: 创建通信通道
        // ================================================================================
        // resp_tx/resp_rx: 用于接收原始 CommandResp 消息
        // line_tx/line_rx: 用于收集最终的 CommandLine 对象
        // done_tx/done_rx: oneshot 通道，用于接收最终退出码
        let (resp_tx, mut resp_rx) = mpsc::channel::<CommandResp>(100);
        let (line_tx, mut line_rx) = mpsc::channel::<CommandLine>(100);
        let (done_tx, done_rx) = oneshot::channel::<Result<i32, CommandError>>();

        // ================================================================================
        // 步骤 3: 发送命令执行请求
        // ================================================================================
        // 构造 CommandRequest 并发送到后台任务
        self.cmd_sender
            .send(CommandRequest {
                id: execution_id,
                command,
                args,
                resp_tx,
                done_tx,
                timeout,
            })
            .await
            // 如果发送失败，说明后台任务已终止
            .map_err(|_| CommandError::Unavailable("Command executor not available".to_string()))?;

        // ================================================================================
        // 步骤 4: 在单独任务中处理响应
        // ================================================================================
        // 此任务负责将 CommandResp 转换为 CommandLine 并收集
        let process_handle = tokio::spawn(async move {
            let mut exit_code = 0;  // 默认退出码

            while let Some(resp) = resp_rx.recv().await {
                match resp {
                    // 输出行 - 转发到 line_tx
                    CommandResp::Line {
                        id: _,
                        stream,
                        text,
                    } => {
                        let _ = line_tx.send(CommandLine { stream, text }).await;
                    }
                    // 完成 - 保存退出码并退出循环
                    CommandResp::Done {
                        id: _,
                        exit_code: code,
                    } => {
                        exit_code = code;
                        break;
                    }
                    // 错误 - 将错误消息作为 stderr 行发送
                    CommandResp::Error { id: _, message } => {
                        let _ = line_tx
                            .send(CommandLine {
                                stream: Stream::Stderr,
                                text: format!("Error: {}", message),
                            })
                            .await;
                        break;
                    }
                }
            }

            exit_code
        });

        // ================================================================================
        // 步骤 5: 收集所有输出行
        // ================================================================================
        // 当 line_rx 关闭或所有消息都被接收时，循环结束
        let mut lines = Vec::new();
        while let Some(line) = line_rx.recv().await {
            lines.push(line);
        }

        // ================================================================================
        // 步骤 6: 等待处理任务完成
        // ================================================================================
        // process_handle.await 等待 spawn 的任务完成
        // unwrap_or(1) 在任务 panic 时返回错误退出码
        let _exit_code = process_handle.await.unwrap_or(1);

        // ================================================================================
        // 步骤 7: 等待执行完成并返回结果
        // ================================================================================
        // done_rx.await 等待最终结果
        // ?? 展开两层 Result：
        // - 第一层：oneshot::RecvError（通道错误）
        // - 第二层：CommandError（命令执行错误）
        let result = done_rx
            .await
            .map_err(|_| CommandError::ExecutionError("Command execution failed".to_string()))??;

        Ok((result, lines))
    }
}

//--------------------------------------------------------------------------------------------------
// 函数 (Functions)
//--------------------------------------------------------------------------------------------------

/// # 创建新的命令执行器句柄
///
/// 这是一个便捷函数，等同于 `CommandHandle::new()`。
///
/// ## 返回值
///
/// 返回一个新的 `CommandHandle` 实例。
///
/// ## 使用示例
///
/// ```rust,no_run
/// use microsandbox_portal::command::create_command_executor;
///
/// let handle = create_command_executor();
/// let (exit_code, output) = handle.execute("ls", vec![], None).await?;
/// ```
pub fn create_command_executor() -> CommandHandle {
    CommandHandle::new()
}

/// # 执行系统命令并流式传输输出
///
/// 这是实际执行命令的内部函数。
///
/// ## 参数说明
///
/// * `id` - 执行的唯一标识符
/// * `command` - 要执行的命令
/// * `args` - 命令参数列表
/// * `resp_tx` - 发送响应的通道
/// * `timeout` - 可选的超时时间（秒）
///
/// ## 返回值
///
/// * `Ok(i32)` - 命令的退出码
/// * `Err(CommandError)` - 执行失败时的错误
///
/// ## 执行流程
///
/// 1. 派生进程
/// 2. 设置 stdout/stderr 管道
/// 3. 启动输出处理任务
/// 4. 等待进程完成（带可选超时）
/// 5. 清理资源
///
/// ## tokio::select! 说明
///
/// `tokio::select!` 宏允许等待多个异步操作中任何一个完成：
/// - 这里用于实现超时功能
/// - 哪个分支先完成就执行哪个
/// - 未完成的分支被取消
async fn execute_command(
    id: String,
    command: String,
    args: Vec<String>,
    resp_tx: Sender<CommandResp>,
    timeout: Option<u64>,
) -> Result<i32, CommandError> {
    // ================================================================================
    // 步骤 1: 派生命令进程
    // ================================================================================
    // Command::new() 创建一个新的命令构建器
    // .args() 设置命令行参数
    // .stdin/stdout/stderr() 设置标准输入/输出/错误的处理方式
    // .spawn() 派生新进程
    //
    // 标准输入设置为 null，因为命令不需要交互式输入
    // 标准输出和错误被管道化，以便捕获输出
    let mut process = Command::new(&command)
        .args(&args)
        .stdin(std::process::Stdio::null())    // 关闭标准输入
        .stdout(std::process::Stdio::piped())  // 捕获标准输出
        .stderr(std::process::Stdio::piped())  // 捕获标准错误
        .spawn()
        .map_err(|e| CommandError::SpawnError(format!("Failed to spawn command: {}", e)))?;

    // ================================================================================
    // 步骤 2: 获取 stdout 和 stderr 句柄
    // ================================================================================
    // .take() 获取 Option 内部的值，设置为 None
    // 这样做了之后，process.stdout 和 process.stderr 就不能再访问了
    let stdout = process
        .stdout
        .take()
        .ok_or_else(|| CommandError::ExecutionError("Failed to capture stdout".to_string()))?;

    let stderr = process
        .stderr
        .take()
        .ok_or_else(|| CommandError::ExecutionError("Failed to capture stderr".to_string()))?;

    // ================================================================================
    // 步骤 3: 创建处理状态跟踪器
    // ================================================================================
    // Arc<Mutex<bool>> 用于在多个任务间共享状态
    // true 表示正在处理，false 表示应该停止
    //
    // Arc: 允许多个任务共享同一个 Mutex
    // Mutex: 保护内部的布尔值
    let processing = Arc::new(Mutex::new(true));

    // ================================================================================
    // 步骤 4: 启动 stdout 处理任务
    // ================================================================================
    // BufReader 提供缓冲的异步读取
    // .lines() 创建一个按行读取的迭代器
    let stdout_reader = BufReader::new(stdout);
    let stdout_resp_tx = resp_tx.clone();  // 克隆响应通道发送端
    let stdout_id = id.clone();            // 克隆执行 ID
    let stdout_processing = Arc::clone(&processing);  // 克隆处理状态

    // 生成任务处理 stdout 输出
    let stdout_handle = tokio::spawn(async move {
        let mut lines = stdout_reader.lines();

        // 逐行读取输出
        while let Ok(Some(line)) = lines.next_line().await {
            // 检查是否应该继续处理
            if *stdout_processing.lock().unwrap() {
                // 发送输出行
                let _ = stdout_resp_tx
                    .send(CommandResp::Line {
                        id: stdout_id.clone(),
                        stream: Stream::Stdout,
                        text: line,
                    })
                    .await;
            } else {
                // 收到停止信号，退出循环
                break;
            }
        }
    });

    // ================================================================================
    // 步骤 5: 启动 stderr 处理任务
    // ================================================================================
    // 与 stdout 处理类似，但使用 stderr 通道
    let stderr_reader = BufReader::new(stderr);
    let stderr_resp_tx = resp_tx.clone();
    let stderr_id = id.clone();
    let stderr_processing = Arc::clone(&processing);

    let stderr_handle = tokio::spawn(async move {
        let mut lines = stderr_reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if *stderr_processing.lock().unwrap() {
                let _ = stderr_resp_tx
                    .send(CommandResp::Line {
                        id: stderr_id.clone(),
                        stream: Stream::Stderr,
                        text: line,
                    })
                    .await;
            } else {
                break;
            }
        }
    });

    // ================================================================================
    // 步骤 6: 创建等待进程完成的 Future
    // ================================================================================
    // 这个异步块等待进程完成并发送响应
    let process_wait = async {
        match process.wait().await {
            // 进程正常退出
            Ok(status) => {
                let exit_code = status.code().unwrap_or(1);
                // 发送完成响应
                let _ = resp_tx
                    .send(CommandResp::Done {
                        id: id.clone(),
                        exit_code,
                    })
                    .await;
                Ok(exit_code)
            }
            // 等待进程时出错
            Err(e) => {
                // 发送错误响应
                let _ = resp_tx
                    .send(CommandResp::Error {
                        id: id.clone(),
                        message: format!("Command execution failed: {}", e),
                    })
                    .await;
                Err(CommandError::ExecutionError(format!(
                    "Failed to wait for command: {}",
                    e
                )))
            }
        }
    };

    // ================================================================================
    // 步骤 7: 执行并处理超时
    // ================================================================================
    let result = match timeout {
        // --- 有超时设置 ---
        Some(timeout_secs) => {
            let timeout_duration = Duration::from_secs(timeout_secs);
            // tokio::select! 等待多个操作中第一个完成的
            tokio::select! {
                // 分支 1: 进程完成
                result = process_wait => result,
                // 分支 2: 超时
                _ = sleep(timeout_duration) => {
                    // 超时发生，杀死进程
                    let _ = process.kill().await;
                    // 发送超时错误响应
                    let _ = resp_tx
                        .send(CommandResp::Error {
                            id: id.clone(),
                            message: format!("Command timed out after {} seconds", timeout_secs),
                        })
                        .await;
                    // 返回超时错误
                    Err(CommandError::Timeout(timeout_secs))
                }
            }
        }
        // --- 无超时设置 ---
        None => {
            // 直接等待进程完成
            process_wait.await
        }
    };

    // ================================================================================
    // 步骤 8: 通知输出处理任务停止
    // ================================================================================
    // 获取锁并设置为 false，通知输出处理任务退出
    {
        let mut guard = processing.lock().unwrap();
        *guard = false;
    }

    // ================================================================================
    // 步骤 9: 等待输出处理任务完成
    // ================================================================================
    // 确保所有输出都被处理完毕
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    result
}
