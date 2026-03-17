//! # REPL 类型和接口定义 (Types and Interfaces)
//!
//! 本模块定义了代码评估系统使用的基础类型、特质和错误定义。
//! 每个语言实现必须符合这些接口。
//!
//! ## 架构概览
//!
//! 本模块围绕几个关键抽象构建：
//!
//! ### 1. Language 枚举
//! 表示支持的编程语言，用于在运行时选择正确的引擎。
//!
//! ### 2. EngineHandle 结构
//! 客户端与引擎交互的主要接口，通过命令通道发送请求。
//!
//! ### 3. Engine 特质
//! 每个语言引擎必须实现的接口，定义初始化、评估和关闭操作。
//!
//! ### 4. Resp 和 Line 类型
//! 表示评估输出的数据结构。
//!
//! ## 数据流
//!
//! ```text
//! 客户端
//!   │
//!   │ EngineHandle::eval()
//!   ▼
//! ┌─────────────────┐
//! │  EngineHandle   │
//! │  (cmd_sender)   │
//! └─────────────────┘
//!   │
//!   │ Cmd::Eval (通过 channel)
//!   ▼
//! ┌─────────────────┐
//! │  Reactor 线程   │
//! └─────────────────┘
//!   │
//!   │ 分发到相应引擎
//!   ▼
//! ┌─────────────────┐
//! │ Language Engine │
//! │ (Python/Node)   │
//! └─────────────────┘
//!   │
//!   │ Resp::Line / Resp::Done / Resp::Error
//!   ▼
//! ┌─────────────────┐
//! │  响应通道       │
//! └─────────────────┘
//!   │
//!   ▼
//! 客户端接收 Vec<Line>
//! ```
//!
//! ## 错误处理
//!
//! `EngineError` 类型封装了引擎操作期间可能发生的各种错误：
//! - 初始化失败
//! - 评估错误
//! - 超时
//! - 引擎不可用

use thiserror::Error;
use tokio::sync::mpsc::Sender;

//--------------------------------------------------------------------------------------------------
// 类型 (Types)
//--------------------------------------------------------------------------------------------------

/// # 支持的编程语言枚举
///
/// 此枚举表示代码评估系统支持的所有编程语言。
///
/// ## 变体说明
///
/// ### Python (需要 `python` 特性)
/// 表示 Python 语言支持。当启用 `python` 特性时，
/// Python 引擎通过 subprocess 运行 `python3 -i` 交互式解释器。
///
/// ### Node (需要 `nodejs` 特性)
/// 表示 Node.js/JavaScript 支持。当启用 `nodejs` 特性时，
/// Node 引擎通过 subprocess 运行配置了自定义 REPL 的 node 进程。
///
/// ## 派生特质
///
/// - `Debug`: 调试格式化
/// - `Clone`, `Copy`: 值类型，可以廉价复制
/// - `PartialEq`, `Eq`: 支持相等性比较
///
/// ## 使用示例
///
/// ```rust
/// #[cfg(feature = "python")]
/// let lang = Language::Python;
///
/// #[cfg(feature = "nodejs")]
/// let lang = Language::Node;
///
/// // 在 match 中使用
/// match language {
///     #[cfg(feature = "python")]
///     Language::Python => { /* 处理 Python */ }
///     #[cfg(feature = "nodejs")]
///     Language::Node => { /* 处理 Node.js */ }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Python 语言支持
    ///
    /// 仅在编译时启用 `python` 特性时可用
    #[cfg(feature = "python")]
    Python,

    /// Node.js/JavaScript 支持
    ///
    /// 仅在编译时启用 `nodejs` 特性时可用
    #[cfg(feature = "nodejs")]
    Node,
}

/// # 输出流类型枚举
///
/// 标识输出行的来源流。
///
/// ## 变体说明
///
/// ### Stdout
/// 标准输出流。包含程序的正常输出。
///
/// ### Stderr
/// 标准错误流。包含错误消息、警告等。
///
/// ## 使用场景
///
/// 在解析引擎输出时，区分 stdout 和 stderr 很重要：
/// - stdout: 正常输出，如 print() 的结果
/// - stderr: 错误输出，如异常堆栈跟踪
///
/// ## 派生特质
///
/// - `Debug`: 调试格式化
/// - `Clone`, `Copy`: 值类型
/// - `PartialEq`, `Eq`: 支持比较
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// 标准输出流
    Stdout,

    /// 标准错误流
    Stderr,
}

/// # 输出行结构
///
/// 表示代码评估产生的单行输出。
///
/// ## 字段说明
///
/// * `stream` - 输出来源（stdout 或 stderr）
/// * `text` - 行的实际内容
///
/// ## 使用示例
///
/// ```rust
/// let line = Line {
///     stream: Stream::Stdout,
///     text: "Hello, World!".to_string(),
/// };
///
/// println!("[{:?}] {}", line.stream, line.text);
/// ```
///
/// ## 派生特质
///
/// - `Debug`: 调试格式化
/// - `Clone`: 支持克隆（因为包含 String）
#[derive(Debug, Clone)]
pub struct Line {
    /// 流类型（标准输出或标准错误）
    pub stream: Stream,

    /// 行的文本内容
    pub text: String,
}

/// # 引擎句柄结构
///
/// 这是客户端与 REPL 引擎交互的主要接口。
///
/// ## 设计说明
///
/// `EngineHandle` 是一个轻量级的句柄，包含一个命令发送器。
/// 它通过与反应器线程的 channel 进行通信：
///
/// ```text
/// EngineHandle
///     │
///     │ cmd_sender (channel 发送端)
///     ▼
/// ┌─────────────────┐
/// │  Reactor 线程   │ ← 监听命令
/// │  (后台运行)     │
/// └─────────────────┘
///     │
///     │ 分发给具体引擎
///     ▼
/// Python/Node Engine
/// ```
///
/// ## 字段说明
///
/// * `cmd_sender` - 发送到反应器线程的命令通道发送端
///   - `pub(crate)`: 仅在当前 crate 内可见
///   - 这样外部代码不能直接发送命令，必须通过 EngineHandle 的方法
///
/// ## Clone 行为
///
/// 派生的 `Clone` 实现复制句柄：
/// - 克隆 `Sender` 会增加 channel 的引用计数
/// - 多个句柄可以发送到同一个反应器
/// - 这是期望的行为，允许共享引擎访问
///
/// ## 使用示例
///
/// ```rust
/// let handle = start_engines().await?;
///
/// // 可以在多个任务间共享
/// let handle2 = handle.clone();
///
/// // 两个句柄都可以使用
/// handle.eval(...).await?;
/// handle2.eval(...).await?;
/// ```
#[derive(Clone)]
pub struct EngineHandle {
    /// 命令通道发送端 - 包级私有
    pub(crate) cmd_sender: Sender<Cmd>,
}

/// # 引擎错误类型枚举
///
/// 封装引擎操作期间可能发生的各种错误情况。
///
/// ## 变体说明
///
/// ### Initialization(String)
/// 引擎初始化期间发生错误：
/// - 进程启动失败（如 python3 不在 PATH 中）
/// - 管道设置失败
/// - 初始配置失败
///
/// ### Evaluation(String)
/// 代码评估期间发生错误：
/// - 语法错误
/// - 运行时错误
/// - 发送代码到引擎失败
///
/// ### Timeout(u64)
/// 评估超时：
/// - 包含超时的秒数
/// - 当代码执行时间超过指定的 timeout 时返回
///
/// ### Unavailable(String)
/// 引擎不可用：
/// - 反应器线程已关闭
/// - channel 已断开
/// - 引擎崩溃
///
/// ## thiserror 使用说明
///
/// `#[derive(Error)]` 自动实现 `std::error::Error` trait。
/// `#[error("...")]` 属性定义 `Display` 实现：
///
/// ```rust
/// let err = EngineError::Timeout(30);
/// println!("{}", err);  // 输出：Evaluation timeout after 30 seconds
/// ```
#[derive(Debug, Error)]
pub enum EngineError {
    /// 初始化错误
    ///
    /// 包含错误的详细描述
    #[error("Failed to initialize engine: {0}")]
    Initialization(String),

    /// 评估错误
    ///
    /// 包含错误的详细描述
    #[error("Evaluation error: {0}")]
    Evaluation(String),

    /// 超时错误
    ///
    /// 包含超时的秒数
    #[error("Evaluation timeout after {0} seconds")]
    Timeout(u64),

    /// 不可用错误
    ///
    /// 包含引擎不可用的原因
    #[error("Engine unavailable: {0}")]
    Unavailable(String),
}

/// # 命令枚举（发送到反应器线程）
///
/// 这些命令从 `EngineHandle` 发送到反应器线程，
/// 以执行代码评估和引擎关闭等操作。
///
/// ## 变体说明
///
/// ### Eval
/// 评估指定的代码：
/// - `_id`: 评估的唯一标识符
/// - `_code`: 要执行的源代码
/// - `_language`: 使用的编程语言
/// - `_resp_tx`: 接收响应的通道发送端
/// - `_timeout`: 可选的超时时间（秒）
///
/// ### Shutdown
/// 关闭反应器和所有引擎。
///
/// ## 命名约定
///
/// 字段前缀 `_` 表示这些参数在当前模式中未直接使用，
/// 但需要存在于结构中以供反应器使用。
///
/// ## 可见性
///
/// `pub(crate)` 表示此枚举仅在当前 crate 内可见。
/// 外部代码不能直接构造命令，必须通过 `EngineHandle` 的方法。
#[derive(Debug)]
pub(crate) enum Cmd {
    /// 评估代码命令
    ///
    /// 包含评估所需的所有信息
    Eval {
        /// 评估的唯一标识符
        _id: String,

        /// 要评估的源代码
        _code: String,

        /// 使用的编程语言
        _language: Language,

        /// 响应通道发送端
        _resp_tx: Sender<Resp>,

        /// 可选的超时时间（秒）
        _timeout: Option<u64>,
    },

    /// 关闭命令
    ///
    /// 通知反应器停止处理并关闭所有引擎
    Shutdown,
}

/// # 响应枚举（从引擎返回）
///
/// 这些响应从引擎发送回客户端，以提供
/// 评估结果、输出行或错误消息。
///
/// ## 变体说明
///
/// ### Line
/// 评估产生的一行输出：
/// - `id`: 评估的标识符（用于匹配响应和请求）
/// - `stream`: 输出流类型（stdout/stderr）
/// - `text`: 行的内容
///
/// ### Done
/// 评估成功完成：
/// - `id`: 评估的标识符
///
/// ### Error
/// 评估期间发生错误：
/// - `id`: 评估的标识符
/// - `message`: 错误描述
///
/// ## 响应流程
///
/// 典型的评估响应序列：
///
/// ```text
/// Cmd::Eval 发送
///     │
///     ▼
/// Resp::Line { text: "output line 1" }
/// Resp::Line { text: "output line 2" }
/// ...
/// Resp::Line { text: "output line n" }
///     │
///     ▼
/// Resp::Done  ← 评估完成
/// ```
///
/// 或者在错误情况下：
///
/// ```text
/// Cmd::Eval 发送
///     │
///     ▼
/// Resp::Error { message: "SyntaxError: ..." }  ← 评估失败
/// ```
#[derive(Debug)]
pub enum Resp {
    /// 评估输出行
    Line {
        /// 评估的唯一标识符
        id: String,

        /// 流类型（标准输出或标准错误）
        stream: Stream,

        /// 行的文本内容
        text: String,
    },

    /// 评估完成
    Done {
        /// 评估的唯一标识符
        id: String,
    },

    /// 评估错误
    Error {
        /// 评估的唯一标识符
        id: String,

        /// 错误消息
        message: String,
    },
}

//--------------------------------------------------------------------------------------------------
// 特质 (Traits)
//--------------------------------------------------------------------------------------------------

/// # 引擎特质 - 定义通用引擎操作
///
/// 此特质必须由每个语言特定的引擎实现。
/// 它定义了所有引擎必须支持的核心操作。
///
/// ## 特质约束
///
/// ```rust
/// #[async_trait::async_trait]
/// pub trait Engine: Send + 'static
/// ```
///
/// ### Send
/// 表示此 trait 的对象可以在线程间安全发送。
/// 这是必需的，因为引擎在单独的线程中运行。
///
/// ### 'static
/// 生命周期'static 表示实现不包含任何非静态生命周期的引用。
/// 这是必需的，因为引擎会长期存在。
///
/// ### async_trait
/// `#[async_trait]` 宏允许在 trait 中定义 async 方法。
/// Rust 原生不支持 trait 中的 async 方法，此宏通过 Box  Future 实现。
///
/// ## 必须实现的方法
///
/// | 方法 | 说明 |
/// |------|------|
/// | `initialize()` | 初始化引擎，设置进程和管道 |
/// | `eval()` | 评估代码并发送响应 |
/// | `shutdown()` | 关闭引擎，清理资源 |
///
/// ## 实现示例
///
/// ```rust,no_run
/// #[async_trait]
/// impl Engine for PythonEngine {
///     async fn initialize(&mut self) -> Result<(), EngineError> {
///         // 启动 Python 进程...
///     }
///
///     async fn eval(&mut self, ...) -> Result<(), EngineError> {
///         // 发送代码到 Python 进程...
///     }
///
///     async fn shutdown(&mut self) {
///         // 终止 Python 进程...
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Engine: Send + 'static {
    /// # 初始化引擎
    ///
    /// 此方法在引擎首次创建时调用，用于设置
    /// 必要的资源、启动评估上下文等。
    ///
    /// ## 实现细节
    ///
    /// 对于 Python 和 Node.js 引擎，此方法：
    /// 1. 启动子进程（python3 或 node）
    /// 2. 设置 stdin/stdout/stderr 管道
    /// 3. 启动输出处理线程
    /// 4. 等待引擎准备就绪
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 初始化成功
    /// * `Err(EngineError::Initialization)` - 初始化失败
    async fn initialize(&mut self) -> Result<(), EngineError>;

    /// # 评估代码并通过通道发送响应
    ///
    /// 此方法评估给定的代码并通过提供的通道发送
    /// 输出和状态消息。
    ///
    /// ## 参数说明
    ///
    /// * `id` - 此评估的唯一标识符
    ///   - 用于将响应与请求匹配
    ///   - 在并发评估时特别重要
    ///
    /// * `code` - 要评估的源代码
    ///   - 可以是单行或多行代码
    ///   - 语言必须与引擎匹配
    ///
    /// * `sender` - 用于发送评估响应的通道
    ///   - 响应类型：`Resp::Line`, `Resp::Done`, `Resp::Error`
    ///   - 通道允许流式输出
    ///
    /// * `timeout` - 可选的超时时间（秒）
    ///   - `Some(n)`: n 秒后终止执行
    ///   - `None`: 不设置超时
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 评估完成（结果通过通道发送）
    /// * `Err(EngineError)` - 评估失败
    ///
    /// ## 实现细节
    ///
    /// 典型实现流程：
    /// 1. 将代码写入引擎的 stdin
    /// 2. 添加结束标记（EOE marker）
    /// 3. 等待结束标记出现在输出中
    /// 4. 通过通道发送所有输出行
    /// 5. 发送 `Resp::Done` 或 `Resp::Error`
    async fn eval(
        &mut self,
        id: String,
        code: String,
        sender: &Sender<Resp>,
        timeout: Option<u64>,
    ) -> Result<(), EngineError>;

    /// # 关闭引擎
    ///
    /// 此方法在引擎关闭时调用，用于清理
    /// 资源、终止进程等。
    ///
    /// ## 实现细节
    ///
    /// 对于 Python 和 Node.js 引擎，此方法：
    /// 1. 发送关闭命令到控制线程
    /// 2. 等待处理中的请求完成
    /// 3. 终止子进程
    /// 4. 清理管道和句柄
    ///
    /// ## 注意
    ///
    /// 此方法是 async 但返回 `()`，因为：
    /// - 关闭操作可能涉及异步操作
    /// - 通常不需要报告关闭错误
    /// - 即使关闭失败，程序也会继续退出
    async fn shutdown(&mut self);
}

// -------------------------------------------------------------------------------------------------
// Trait 实现 (Trait Implementations)
// -------------------------------------------------------------------------------------------------

// EngineHandle 的 Debug 实现
// 由于 cmd_sender 是 tokio channel，不能直接打印，所以显示占位符
impl std::fmt::Debug for EngineHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineHandle")
            .field("cmd_sender", &"<channel>")  // 占位符，表示 channel 类型
            .finish()
    }
}
