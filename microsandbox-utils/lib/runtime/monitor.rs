//! # 进程监控（Process Monitor）模块
//!
//! 本模块定义了进程监控的 trait 和相关类型，用于抽象化进程监控的实现。
//!
//! ## 核心类型
//!
//! ### [`ChildIo`]
//! 枚举类型，表示子进程的 IO 连接方式：
//! - `TTY`: 伪终端模式，用于交互式会话
//! - `Piped`: 管道模式，用于非交互式后台运行
//!
//! ### [`ProcessMonitor`]
//! Trait，定义了进程监控的接口：
//! - `start()`: 开始监控进程
//! - `stop()`: 停止监控
//!
//! ## 为什么需要抽象监控接口？
//!
//! 1. **解耦**: 监督者（Supervisor）不需要关心具体的监控实现
//! 2. **可测试性**: 可以使用 mock 实现进行单元测试
//! 3. **可扩展性**: 可以添加不同的监控策略（如资源监控、健康检查等）
//!
//! ## ChildIo 的两种模式
//!
//! ### TTY 模式（伪终端）
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │                  TTY                    │
//! │  ┌─────────────┐     ┌─────────────┐    │
//! │  │ master_read │     │master_write │    │
//! │  │  (AsyncFd)  │     │   (File)    │    │
//! │  └─────────────┘     └─────────────┘    │
//! │         │                   │           │
//! │         └────────┬──────────┘           │
//! │                  ▼                      │
//! │         ┌─────────────┐                 │
//! │         │  PTY Master │                 │
//! │         └─────────────┘                 │
//! │                  │                      │
//! │         ┌─────────────┐                 │
//! │         │  PTY Slave  │                 │
//! │         └─────────────┘                 │
//! │         │   │   │                       │
//! │         ▼   ▼   ▼                       │
//! │      stdin stdout stderr                │
//! │         (子进程)                         │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ### 管道模式（Piped）
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │                Piped                    │
//! │  ┌──────────┐  ┌──────────┐  ┌────────┐ │
//! │  │  stdin   │  │  stdout  │  │ stderr │ │
//! │  │(ChildStd)│  │(ChildStd)│  │(ChildStd)│
//! │  └──────────┘  └──────────┘  └────────┘ │
//! │      │            │            │        │
//! │      ▼            ▼            ▼        │
//! │   stdin        stdout       stderr      │
//! │      │            │            │        │
//! │      ▼            ▼            ▼        │
//! │         (子进程)                        │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## 使用示例
//!
//! ### 实现 ProcessMonitor trait
//!
//! ```rust,ignore
//! use microsandbox_utils::runtime::{ProcessMonitor, ChildIo};
//! use microsandbox_utils::MicosandboxUtilsResult;
//! use async_trait::async_trait;
//!
//! struct MyMonitor {
//!     // 监控相关的字段
//! }
//!
//! #[async_trait]
//! impl ProcessMonitor for MyMonitor {
//!     async fn start(&mut self, pid: u32, child_io: ChildIo) -> MicrosandboxUtilsResult<()> {
//!         // 开始监控进程
//!         // pid: 进程 ID
//!         // child_io: 子进程的 IO 连接
//!         Ok(())
//!     }
//!
//!     async fn stop(&mut self) -> MicrosandboxUtilsResult<()> {
//!         // 停止监控，清理资源
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ## AsyncFd 说明
//!
//! `AsyncFd` 是 tokio 提供的类型，用于将同步的文件描述符
//! 转换为异步 IO 对象。它使用 epoll/kqueue 实现异步就绪通知。
//!
//! 在 TTY 模式下，`master_read` 使用 `AsyncFd<std::fs::File>` 包装，
//! 使得可以异步地读取伪终端主端的数据。

use async_trait::async_trait;
use tokio::{
    fs::File,
    io::unix::AsyncFd,
    process::{ChildStderr, ChildStdin, ChildStdout},
};

use crate::MicrosandboxUtilsResult;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// ### 子进程 IO 类型枚举
///
/// `ChildIo` 表示子进程的 IO 连接方式，有两种模式：
///
/// ## 变体说明
///
/// ### `TTY` - 伪终端模式
///
/// 用于交互式终端会话，提供完整的终端功能支持。
///
/// #### 字段
///
/// - `master_read: AsyncFd<std::fs::File>`
///   - 伪终端主端的读取侧
///   - 使用 `AsyncFd` 包装，支持异步读取
///   - 从子进程的 stdout/stderr 读取输出
///
/// - `master_write: File`
///   - 伪终端主端的写入侧
///   - 用于向子进程的 stdin 写入数据
///
/// #### PTY 工作原理
///
/// 伪终端（PTY, Pseudo-Terminal）由一对设备文件组成：
/// - **主端（Master）**: 由监督者控制
/// - **从端（Slave）**: 被子进程用作终端
///
/// 数据流向：
/// ```text
/// 监督者写入 ──> master_write ──> PTY ──> slave ──> 子进程 stdin
/// 子进程 stdout ──> slave ──> PTY ──> master_read ──> 监督者读取
/// ```
///
/// ### `Piped` - 管道模式
///
/// 用于非交互式后台运行，使用标准管道连接。
///
/// #### 字段
///
/// - `stdin: Option<ChildStdin>`: 子进程的标准输入管道
/// - `stdout: Option<ChildStdout>`: 子进程的标准输出管道
/// - `stderr: Option<ChildStderr>`: 子进程的标准错误管道
///
/// #### 为什么使用 Option？
///
/// 管道可能被消费（take）或关闭，使用 `Option` 表示可能的空状态。
///
/// ## 使用场景对比
///
/// | 特性 | TTY 模式 | 管道模式 |
/// |------|---------|---------|
/// | 交互性 | 支持 | 不支持 |
/// | 终端特性 | 完整支持 | 不支持 |
/// | 输出捕获 | 混合输出 | 分离的 stdout/stderr |
/// | 适用场景 | shell 会话 | 后台服务 |
///
/// ## 示例
///
/// ```rust,ignore
/// // TTY 模式
/// let child_io = ChildIo::TTY {
///     master_read: async_fd,
///     master_write: file,
/// };
///
/// // 管道模式
/// let child_io = ChildIo::Piped {
///     stdin: child_stdin,
///     stdout: child_stdout,
///     stderr: child_stderr,
/// };
/// ```
pub enum ChildIo {
    /// ### 伪终端（TTY）模式
    ///
    /// 用于交互式终端会话。
    ///
    /// ## 字段
    ///
    /// - `master_read`: 伪终端主端的异步读取文件描述符
    /// - `master_write`: 伪终端主端的写入文件句柄
    TTY {
        /// 伪终端主端的读取侧
        /// 使用 AsyncFd 包装以支持异步 IO
        master_read: AsyncFd<std::fs::File>,

        /// 伪终端主端的写入侧
        /// 用于向子进程发送输入
        master_write: File,
    },

    /// ### 管道（Piped）模式
    ///
    /// 用于非交互式后台运行。
    ///
    /// ## 字段
    ///
    /// - `stdin`: 子进程的标准输入
    /// - `stdout`: 子进程的标准输出
    /// - `stderr`: 子进程的标准错误
    Piped {
        /// 子进程的标准输入管道
        stdin: Option<ChildStdin>,

        /// 子进程的标准输出管道
        stdout: Option<ChildStdout>,

        /// 子进程的标准错误管道
        stderr: Option<ChildStderr>,
    },
}

//--------------------------------------------------------------------------------------------------
// Trait 定义
//--------------------------------------------------------------------------------------------------

/// ### 进程监控 Trait
///
/// `ProcessMonitor` 定义了进程监控的异步接口。
///
/// 实现这个 trait 的类型可以：
/// 1. 开始监控一个进程（通过 PID 和 IO 连接）
/// 2. 停止监控并清理资源
///
/// ## 为什么使用 Trait？
///
/// - **解耦**: Supervisor 不依赖具体的监控实现
/// - **可替换**: 可以在运行时使用不同的监控策略
/// - **可测试**: 可以使用 mock 实现进行单元测试
///
/// ## async_trait 说明
///
/// `#[async_trait]` 是一个过程宏，用于在 trait 中定义异步方法。
///
/// Rust 标准库的 trait 方法不能直接返回 `impl Future`，
/// `async_trait` 宏通过返回 `Box<dyn Future>` 来解决这个问题。
///
/// ## 方法说明
///
/// ### `start()`
///
/// 开始监控进程。
///
/// #### 参数
///
/// - `pid: u32`: 子进程的进程 ID
/// - `child_io: ChildIo`: 子进程的 IO 连接
///
/// #### 返回值
///
/// - `Ok(())`: 成功开始监控
/// - `Err(MicrosandboxUtilsError)`: 启动监控失败
///
/// ### `stop()`
///
/// 停止监控并清理资源。
///
/// #### 返回值
///
/// - `Ok(())`: 成功停止监控
/// - `Err(MicrosandboxUtilsError)`: 停止监控失败
///
/// ## 实现示例
///
/// ```rust,ignore
/// use microsandbox_utils::runtime::{ProcessMonitor, ChildIo};
/// use microsandbox_utils::MicosandboxUtilsResult;
/// use async_trait::async_trait;
///
/// struct ResourceMonitor {
///     // 资源监控相关的字段
/// }
///
/// #[async_trait]
/// impl ProcessMonitor for ResourceMonitor {
///     async fn start(&mut self, pid: u32, child_io: ChildIo) -> MicrosandboxUtilsResult<()> {
///         // 初始化资源监控
///         // 可以启动后台任务定期收集 CPU/内存使用情况
///         Ok(())
///     }
///
///     async fn stop(&mut self) -> MicrosandboxUtilsResult<()> {
///         // 清理监控资源
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait ProcessMonitor {
    /// ### 开始监控进程
    ///
    /// 此异步方法在进程启动后被调用，用于初始化监控。
    ///
    /// ## 参数
    ///
    /// - `pid: u32`: 子进程的进程 ID
    ///   - 可以用于发送信号、查询状态等操作
    /// - `child_io: ChildIo`: 子进程的 IO 连接
    ///   - 可以是 TTY 模式或管道模式
    ///   - 用于读写子进程的输入输出
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 监控成功启动
    /// - `Err(MicrosandboxUtilsError)`: 启动监控失败
    ///
    /// ## 典型实现
    ///
    /// 一个典型的 `start` 实现可能：
    /// 1. 保存 pid 和 child_io 到结构体字段
    /// 2. 启动后台任务监控进程状态
    /// 3. 启动后台任务处理 IO（如日志记录）
    /// 4. 初始化资源监控（如需要）
    async fn start(&mut self, pid: u32, child_io: ChildIo) -> MicrosandboxUtilsResult<()>;

    /// ### 停止监控
    ///
    /// 此异步方法用于停止监控并清理资源。
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 监控成功停止
    /// - `Err(MicrosandboxUtilsError)`: 停止监控失败
    ///
    /// ## 典型实现
    ///
    /// 一个典型的 `stop` 实现可能：
    /// 1. 停止后台任务
    /// 2. 关闭 IO 连接
    /// 3. 释放监控资源
    /// 4. 记录最终状态
    async fn stop(&mut self) -> MicrosandboxUtilsResult<()>;
}
