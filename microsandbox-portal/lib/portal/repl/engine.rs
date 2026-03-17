//! # 核心引擎管理模块 (Core Engine Management)
//!
//! 本模块实现了 REPL 引擎的中央管理系统。
//! 它通过 `EngineHandle` 类型提供与语言特定引擎交互的统一接口，
//! 并管理每个引擎的生命周期。
//!
//! ## 架构设计
//!
//! 本架构遵循 Reactor（反应器）模式：
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │                        EngineHandle                            │
//! │  ┌──────────────────────────────────────────────────────────┐  │
//! │  │  eval() / shutdown() 方法                                 │  │
//! │  │       │                                                   │  │
//! │  │       ▼                                                   │  │
//! │  │  cmd_sender (mpsc channel 发送端)                         │  │
//! │  └──────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────┘
//!                              │
//!                              │ Cmd::Eval / Cmd::Shutdown
//!                              ▼
//! ┌────────────────────────────────────────────────────────────────┐
//! │                      Reactor 线程                              │
//! │  ┌──────────────────────────────────────────────────────────┐  │
//! │  │  while let Some(cmd) = cmd_rx.recv().await               │  │
//! │  │       │                                                   │  │
//! │  │       ▼                                                   │  │
//! │  │  match cmd {                                              │  │
//! │  │      Cmd::Eval → engines.<lang>.eval()                   │  │
//! │  │      Cmd::Shutdown → break                               │  │
//! │  │  }                                                        │  │
//! │  └──────────────────────────────────────────────────────────┘  │
//! └────────────────────────────────────────────────────────────────┘
//!                              │
//!              ┌───────────────┼───────────────┐
//!              ▼               ▼               ▼
//!     ┌─────────────┐ ┌─────────────┐
//!     │   Python    │ │   Node.js   │
//!     │   Engine    │ │   Engine    │
//!     │  (python.rs)│ │ (nodejs.rs) │
//!     └─────────────┘ └─────────────┘
//! ```
//!
//! ### Reactor 模式说明
//!
//! 1. **中央反应器线程**: 监听命令通道上的请求
//! 2. **命令分发**: 每个命令被分发到相应的语言引擎
//! 3. **结果返回**: 结果通过响应通道发送回调用者
//!
//! ### 设计优势
//!
//! - **解耦**: 客户端不直接与引擎交互，通过反应器间接通信
//! - **可扩展**: 可以轻松添加新的语言引擎
//! - **线程安全**: 所有通信通过消息传递，避免共享可变状态
//! - **背压**: 使用有界通道防止请求淹没系统
//!
//! ## 特性标志
//!
//! 本模块使用特性标志条件包含语言引擎：
//!
//! | 特性 | 启用 |
//! |------|------|
//! | `python` | Python 引擎 |
//! | `nodejs` | Node.js 引擎 |
//!
//! ## 线程安全
//!
//! 所有组件设计为线程安全的：
//! - 使用消息传递进行线程间通信
//! - 使用 Arc 等线程安全包装器保护共享状态
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_portal::repl::{start_engines, Language};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 启动引擎
//!     let handle = start_engines().await?;
//!
//!     // 评估 Python 代码
//!     #[cfg(feature = "python")]
//!     let result = handle.eval(
//!         "print('Hello, world!')",
//!         Language::Python,
//!         "exec-1",
//!         Some(30)
//!     ).await?;
//!
//!     // 关闭引擎
//!     handle.shutdown().await?;
//!     Ok(())
//! }
//! ```

use tokio::sync::mpsc;

#[cfg(feature = "nodejs")]
use super::nodejs;
#[cfg(feature = "python")]
use super::python;

use super::types::{Cmd, EngineError, EngineHandle, Language, Line, Resp, Stream};

#[cfg(any(feature = "python", feature = "nodejs"))]
use super::types::Engine;

//--------------------------------------------------------------------------------------------------
// 类型 (Types)
//--------------------------------------------------------------------------------------------------

/// # 所有可用 REPL 引擎的集合
///
/// 此结构持有通过特性标志启用的每个语言引擎的实例。
/// 每个引擎都实现了 `Engine` trait。
///
/// ## 字段说明
///
/// * `python` - Python 引擎实例（仅当启用 `python` 特性）
/// * `nodejs` - Node.js 引擎实例（仅当启用 `nodejs` 特性）
///
/// ## Box<dyn Engine> 说明
///
/// 使用 `Box<dyn Engine>` 的原因：
/// - `Box<T>`: 堆分配的智能指针
/// - `dyn Engine`: 动态分发，允许存储不同类型的引擎
/// - 这样可以在运行时选择调用哪个引擎的方法
///
/// ## 为什么使用条件字段
///
/// 通过 `#[cfg(feature = "...")]` 条件编译：
/// - 未启用的特性不会编译对应的引擎代码
/// - 减小最终二进制文件的大小
/// - 避免不必要的依赖
#[cfg(any(feature = "python", feature = "nodejs"))]
struct Engines {
    /// Python 引擎实例
    #[cfg(feature = "python")]
    python: Box<dyn Engine>,

    /// Node.js 引擎实例
    #[cfg(feature = "nodejs")]
    nodejs: Box<dyn Engine>,
}

//--------------------------------------------------------------------------------------------------
// 方法 (Methods)
//--------------------------------------------------------------------------------------------------

impl EngineHandle {
    /// # 评估指定语言的代码
    ///
    /// 此方法向反应器线程发送命令，以指定的语言
    /// 评估提供的代码，然后收集输出行。
    ///
    /// ## 参数说明
    ///
    /// * `code` - 要评估的代码（任何可转换为 String 的类型）
    /// * `language` - 用于评估的编程语言
    /// * `execution_id` - 此评估的唯一标识符
    ///   - 用于调试和日志记录
    ///   - 在并发评估时帮助匹配响应
    /// * `timeout` - 可选的超时时间（秒）
    ///   - `Some(n)`: n 秒后取消评估
    ///   - `None`: 不设置超时
    ///
    /// ## 泛型说明
    ///
    /// ```rust
    /// S: Into<String>
    /// ```
    /// 这意味着 `code` 和 `execution_id` 可以是：
    /// - `&str` - 字符串切片
    /// - `String` - 拥有的字符串
    /// - 任何实现 `Into<String>` 的类型
    ///
    /// ## 返回值
    ///
    /// * `Ok(Vec<Line>)` - 评估产生的输出行向量
    /// * `Err(EngineError)` - 评估失败时的错误
    ///
    /// ## 错误情况
    ///
    /// - `EngineError::Unavailable`: 反应器线程不可用
    /// - 其他引擎特定错误
    ///
    /// ## 实现细节
    ///
    /// 1. 创建响应通道接收结果
    /// 2. 发送评估命令到反应器
    /// 3. 在单独任务中处理响应
    /// 4. 收集所有输出行
    /// 5. 等待处理完成
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// let handle = start_engines().await?;
    ///
    /// // 使用 &str
    /// let result = handle.eval("print('hi')", Language::Python, "id1", Some(30)).await?;
    ///
    /// // 使用 String
    /// let code = String::from("print('hi')");
    /// let result = handle.eval(code, Language::Python, "id2", None).await?;
    /// ```
    pub async fn eval<S: Into<String>>(
        &self,
        code: S,
        language: Language,
        execution_id: S,
        timeout: Option<u64>,
    ) -> Result<Vec<Line>, EngineError> {
        // 将输入转换为 String
        // Into<String> trait 允许从多种字符串类型转换
        let code = code.into();
        let execution_id = execution_id.into();

        // ================================================================================
        // 步骤 1: 创建接收结果的通道
        // ================================================================================
        // resp_tx/resp_rx: 用于接收原始 Resp 消息（Line/Done/Error）
        // line_tx/line_rx: 用于收集最终的 Line 结果
        let (resp_tx, mut resp_rx) = mpsc::channel::<Resp>(100);
        let (line_tx, mut line_rx) = mpsc::channel::<Line>(100);

        // ================================================================================
        // 步骤 2: 发送评估命令到反应器
        // ================================================================================
        // Cmd::Eval 包含所有评估所需的信息
        // .send().await 异步发送命令
        // map_err 将发送错误转换为 EngineError::Unavailable
        self.cmd_sender
            .send(Cmd::Eval {
                _id: execution_id,
                _code: code,
                _language: language,
                _resp_tx: resp_tx,
                _timeout: timeout,
            })
            .await
            .map_err(|_| EngineError::Unavailable("Reactor thread not available".to_string()))?;

        // ================================================================================
        // 步骤 3: 在单独任务中处理响应
        // ================================================================================
        // tokio::spawn 创建一个新的异步任务
        // 此任务负责将 Resp 消息转换为 Line 对象
        let process_handle = tokio::spawn(async move {
            while let Some(resp) = resp_rx.recv().await {
                match resp {
                    // 输出行 - 转发到 line_tx
                    Resp::Line {
                        id: _,      // ID 在此不使用，忽略
                        stream,     // 流类型（stdout/stderr）
                        text,       // 行内容
                    } => {
                        let _ = line_tx.send(Line { stream, text }).await;
                    }
                    // 完成 - 停止处理
                    Resp::Done { id: _ } => {
                        break;
                    }
                    // 错误 - 将错误消息作为 stderr 行发送
                    Resp::Error { id: _, message } => {
                        let _ = line_tx
                            .send(Line {
                                stream: Stream::Stderr,  // 错误消息发送到 stderr
                                text: format!("Error: {}", message),
                            })
                            .await;
                        break;
                    }
                }
            }
        });

        // ================================================================================
        // 步骤 4: 收集所有输出行
        // ================================================================================
        // line_rx.recv().await 异步接收每一行
        // 当发送端关闭或所有消息都被接收时，循环结束
        let mut lines = Vec::new();
        while let Some(line) = line_rx.recv().await {
            lines.push(line);
        }

        // ================================================================================
        // 步骤 5: 等待处理任务完成
        // ================================================================================
        // process_handle.await 等待 spawn 的任务完成
        // 这确保所有资源都被正确清理
        let _ = process_handle.await;

        Ok(lines)
    }

    /// # 关闭所有引擎和反应器
    ///
    /// 此方法向反应器线程发送关闭命令，
    /// 反应器随后关闭所有语言引擎并终止。
    ///
    /// ## 关闭流程
    ///
    /// 1. 发送 `Cmd::Shutdown` 到反应器
    /// 2. 反应器收到后：
    ///    - 调用每个引擎的 `shutdown()` 方法
    ///    - 终止子进程
    ///    - 清理资源
    ///    - 退出反应器循环
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 关闭命令成功发送
    /// * `Err(EngineError::Unavailable)` - 反应器线程不可用
    ///
    /// ## 注意
    ///
    /// 此方法只保证关闭命令被发送。
    /// 实际的关闭过程在后台进行。
    /// 如果需要确保关闭完成，应在发送命令后等待足够时间。
    pub async fn shutdown(&self) -> Result<(), EngineError> {
        // 发送关闭命令到反应器
        self.cmd_sender
            .send(Cmd::Shutdown)
            .await
            // 如果发送失败，说明反应器线程已终止
            .map_err(|_| EngineError::Unavailable("Reactor thread not available".to_string()))?;
        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// 函数 (Functions)
//--------------------------------------------------------------------------------------------------

/// # 启动所有支持的 REPL 引擎并返回句柄
///
/// 此函数初始化所有通过特性标志启用的语言引擎，
/// 并启动管理它们的反应器线程。
/// 返回的句柄可用于与引擎交互。
///
/// ## 启动流程
///
/// ```text
/// 1. 创建命令通道 (cmd_tx, cmd_rx)
///         │
///         ▼
/// 2. 生成反应器任务 (tokio::spawn)
///         │
///         ▼
/// 3. 反应器初始化所有引擎
///         │
///         ▼
/// 4. 返回 EngineHandle(cmd_tx)
/// ```
///
/// ## 返回值
///
/// * `Ok(EngineHandle)` - 成功启动，返回可用于评估代码和关闭引擎的句柄
/// * `Err(EngineError)` - 引擎初始化失败
///
/// ## 反应器任务
///
/// 反应器任务在后台运行，处理：
/// - 接收评估命令
/// - 分发到相应的语言引擎
/// - 处理关闭命令
///
/// ## 使用示例
///
/// ```rust,no_run
/// #[tokio::main]
/// async fn main() {
///     // 启动所有启用的引擎
///     let handle = start_engines().await.unwrap();
///
///     // 使用引擎...
///
///     // 完成后关闭
///     handle.shutdown().await.unwrap();
/// }
/// ```
pub async fn start_engines() -> Result<EngineHandle, EngineError> {
    // ================================================================================
    // 步骤 1: 创建命令通道
    // ================================================================================
    // cmd_tx: 发送端，保存在 EngineHandle 中
    // _cmd_rx: 接收端，由反应器任务使用
    // 通道容量为 100，提供背压防止请求过多
    let (cmd_tx, mut _cmd_rx) = mpsc::channel::<Cmd>(100);

    // ================================================================================
    // 步骤 2: 生成反应器任务
    // ================================================================================
    // tokio::spawn 创建一个新的异步任务在后台运行
    // 此任务独立于调用者，持续处理命令直到关闭
    #[cfg(any(feature = "python", feature = "nodejs"))]
    tokio::spawn(async move {
        // ----------------------------------------------------------------------------
        // 步骤 2.1: 异步初始化引擎
        // ----------------------------------------------------------------------------
        // initialize_engines() 创建并初始化所有启用的引擎
        // expect() 在初始化失败时 panic（通常在开发阶段捕获问题）
        let mut engines = initialize_engines()
            .await
            .expect("Failed to initialize engines");

        // ----------------------------------------------------------------------------
        // 步骤 2.2: 命令处理循环
        // ----------------------------------------------------------------------------
        // while let 循环持续接收命令，直到通道关闭或遇到 Shutdown 命令
        while let Some(cmd) = _cmd_rx.recv().await {
            // 模式匹配命令类型
            match cmd {
                // ========================================================================
                // 命令：Eval - 评估代码
                // ========================================================================
                Cmd::Eval {
                    _id,        // 评估 ID（用于日志和调试）
                    _code,      // 要执行的代码
                    _language,  // 目标语言
                    _resp_tx,   // 响应通道
                    _timeout,   // 可选超时
                } => match _language {
                    // --- Python 评估 ---
                    #[cfg(feature = "python")]
                    Language::Python => {
                        // 调用 Python 引擎的 eval 方法
                        if let Err(e) = engines
                            .python
                            .eval(_id.clone(), _code, &_resp_tx, _timeout)
                            .await
                        {
                            // 评估失败，发送错误响应
                            let _ = _resp_tx
                                .send(Resp::Error {
                                    id: _id,
                                    message: e.to_string(),
                                })
                                .await;
                        }
                    }
                    // --- Node.js 评估 ---
                    #[cfg(feature = "nodejs")]
                    Language::Node => {
                        // 调用 Node.js 引擎的 eval 方法
                        if let Err(e) = engines
                            .nodejs
                            .eval(_id.clone(), _code, &_resp_tx, _timeout)
                            .await
                        {
                            // 评估失败，发送错误响应
                            let _ = _resp_tx
                                .send(Resp::Error {
                                    id: _id,
                                    message: e.to_string(),
                                })
                                .await;
                        }
                    }
                },
                // ========================================================================
                // 命令：Shutdown - 关闭所有引擎
                // ========================================================================
                Cmd::Shutdown => {
                    // 关闭所有引擎
                    #[cfg(feature = "python")]
                    engines.python.shutdown().await;
                    #[cfg(feature = "nodejs")]
                    engines.nodejs.shutdown().await;
                    // 退出命令处理循环
                    break;
                }
            }
        }
        // 循环结束，反应器任务终止
    });

    // ================================================================================
    // 步骤 3: 返回引擎句柄
    // ================================================================================
    // 反应器任务在后台运行
    // 调用者通过 cmd_tx 发送命令到反应器
    Ok(EngineHandle { cmd_sender: cmd_tx })
}

/// # 初始化所有引擎
///
/// 此函数创建并初始化每个通过特性标志启用的语言引擎实例。
///
/// ## 初始化流程
///
/// 1. 创建每个启用的引擎实例
/// 2. 异步调用每个引擎的 `initialize()` 方法
/// 3. 将所有引擎包装在 `Engines` 结构中返回
///
/// ## 返回值
///
/// * `Ok(Engines)` - 包含所有初始化引擎的结构
/// * `Err(EngineError::Initialization)` - 引擎初始化失败
///
/// ## 引擎初始化细节
///
/// ### Python 引擎
/// - 启动 `python3 -q -u -i` 进程
/// - 设置 stdin/stdout/stderr 管道
/// - 等待解释器就绪
///
/// ### Node.js 引擎
/// - 启动配置了自定义 REPL 的 node 进程
/// - 设置通信管道
/// - 等待 REPL 就绪
#[cfg(any(feature = "python", feature = "nodejs"))]
async fn initialize_engines() -> Result<Engines, EngineError> {
    // ================================================================================
    // 步骤 1: 创建引擎实例
    // ================================================================================
    // create_engine() 函数创建未初始化的引擎
    // 每个引擎在各自模块中定义（python.rs, nodejs.rs）
    #[cfg(feature = "python")]
    let mut python_engine = python::create_engine()?;

    #[cfg(feature = "nodejs")]
    let mut nodejs_engine = nodejs::create_engine()?;

    // ================================================================================
    // 步骤 2: 异步初始化每个引擎
    // ================================================================================
    // initialize() 启动子进程并设置通信通道
    // .await 等待初始化完成
    #[cfg(feature = "python")]
    python_engine.initialize().await?;

    #[cfg(feature = "nodejs")]
    nodejs_engine.initialize().await?;

    // ================================================================================
    // 步骤 3: 返回包含所有引擎的结构
    // ================================================================================
    Ok(Engines {
        #[cfg(feature = "python")]
        python: python_engine,
        #[cfg(feature = "nodejs")]
        nodejs: nodejs_engine,
    })
}
