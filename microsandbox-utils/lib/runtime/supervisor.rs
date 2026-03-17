//! # 进程监督者（Supervisor）实现
//!
//! 本模块实现了进程监督者，用于管理子进程的生命周期和日志。
//!
//! ## 什么是监督者模式？
//!
//! 监督者（Supervisor）是一种容错设计模式，起源于 Erlang/OTP：
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Supervisor                           │
//! │                                                         │
//! │  ┌─────────────────────────────────────────────────┐   │
//! │  │              监控逻辑                            │   │
//! │  │  • 启动子进程                                    │   │
//! │  │  • 监控进程状态                                  │   │
//! │  │  • 处理信号（SIGTERM, SIGINT）                   │   │
//! │  │  • 管理日志                                      │   │
//! │  └─────────────────────────────────────────────────┘   │
//! │                         │                               │
//! │                         ▼                               │
//! │  ┌─────────────────────────────────────────────────┐   │
//! │  │              Child Process                       │   │
//! │  │              (e.g., msbrun)                      │   │
//! │  └─────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 监督者的职责
//!
//! 1. **启动子进程**: 使用正确的参数和环境变量启动子进程
//! 2. **IO 管理**: 根据运行环境选择 TTY 或管道模式
//! 3. **日志管理**: 使用轮转日志记录子进程输出
//! 4. **信号处理**: 捕获 SIGTERM/SIGINT 并优雅地关闭子进程
//! 5. **状态监控**: 通过 `ProcessMonitor` 监控子进程状态
//!
//! ## 启动流程
//!
//! ```text
//! start()
//!   │
//!   ├─> 创建日志目录
//!   │
//!   ├─> 初始化轮转日志
//!   │
//!   ├─> 检测终端类型
//!   │     │
//!   │     ├─ 交互式终端 ──> 创建 PTY ──> TTY 模式
//!   │     │
//!   │     └─ 非交互式 ──> 创建管道 ──> Piped 模式
//!   │
//!   ├─> 启动子进程
//!   │
//!   ├─> 启动 ProcessMonitor
//!   │
//!   └─> 等待事件（tokio::select!）
//!         │
//!         ├─ 子进程退出 ──> 记录状态，清理资源
//!         │
//!         ├─ 收到 SIGTERM ──> 发送 SIGTERM 给子进程
//!         │
//!         └─ 收到 SIGINT ──> 发送 SIGTERM 给子进程
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_utils::runtime::{Supervisor, ProcessMonitor};
//! use microsandbox_utils::MicosandboxUtilsResult;
//!
//! // 假设有一个实现了 ProcessMonitor 的类型
//! struct MyMonitor;
//!
//! #[tokio::main]
//! async fn main() -> MicrosandboxUtilsResult<()> {
//!     // 创建监督者
//!     let mut supervisor = Supervisor::new(
//!         "/path/to/msbrun",                // 子进程可执行文件
//!         vec!["run", "--config", "app"],   // 参数
//!         vec![("RUST_LOG", "info")],       // 环境变量
//!         "/var/log/microsandbox",          // 日志目录
//!         MyMonitor,                        // 进程监控器
//!     );
//!
//!     // 启动监督者（会阻塞直到子进程退出）
//!     supervisor.start().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 信号处理
//!
//! 监督者捕获以下 Unix 信号：
//!
//! | 信号 | 触发条件 | 行为 |
//! |------|---------|------|
//! | SIGTERM | `kill <pid>` | 优雅关闭子进程 |
//! | SIGINT | Ctrl+C | 优雅关闭子进程 |
//!
//! "优雅关闭"意味着：
//! 1. 发送 SIGTERM 给子进程
//! 2. 等待子进程退出
//! 3. 记录退出状态
//! 4. 清理资源
//!
//! ## TTY 与管道模式的选择
//!
//! 监督者自动检测运行环境并选择合适的模式：
//!
//! ### TTY 模式（交互式）
//! - 检测条件：`term::is_interactive_terminal()` 返回 `true`
//! - 创建伪终端（PTY）
//! - 适用于：用户直接运行的 CLI
//!
//! ### 管道模式（非交互式）
//! - 检测条件：`term::is_interactive_terminal()` 返回 `false`
//! - 创建标准管道
//! - 适用于：systemd 服务、CI/CD、后台运行
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                       Supervisor<M>                         │
//! │  泛型参数 M: ProcessMonitor                                 │
//! ├─────────────────────────────────────────────────────────────┤
//! │  字段：                                                     │
//! │  • child_exe: PathBuf          - 子进程可执行文件路径       │
//! │  • child_args: Vec<String>     - 子进程参数                 │
//! │  • child_pid: Option<u32>      - 子进程 PID                 │
//! │  • child_envs: Vec<(String, String)> - 环境变量             │
//! │  • log_dir: PathBuf            - 日志目录路径               │
//! │  • process_monitor: M          - 进程监控器                 │
//! ├─────────────────────────────────────────────────────────────┤
//! │  方法：                                                     │
//! │  • new() -> Self               - 创建新实例                 │
//! │  • start() -> Result<()>       - 启动监督者                 │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    pty::openpty,
    unistd::Pid,
};
use std::{
    os::unix::io::{FromRawFd, IntoRawFd},
    path::PathBuf,
    process::Stdio,
};
use tokio::{
    fs::{File, create_dir_all},
    io::unix::AsyncFd,
    process::Command,
    signal::unix::{SignalKind, signal},
};

use crate::{
    ChildIo, MicrosandboxUtilsResult, ProcessMonitor, RotatingLog, path::SUPERVISOR_LOG_FILENAME,
    term,
};

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// ### 进程监督者
///
/// `Supervisor` 负责管理子进程的生命周期，包括启动、IO 管理、信号处理和日志记录。
///
/// ## 泛型参数
///
/// - `M: ProcessMonitor + Send`: 进程监控器类型
///   - 必须实现 `ProcessMonitor` trait
///   - 必须实现 `Send` 以便在线程间传递
///
/// ## 字段说明
///
/// - `child_exe`: 子进程可执行文件的路径
/// - `child_args`: 传递给子进程的命令行参数
/// - `child_pid`: 子进程的 PID（启动后设置）
/// - `child_envs`: 子进程的环境变量列表
/// - `log_dir`: 日志目录路径
/// - `process_monitor`: 进程监控器实例
///
/// ## 设计特点
///
/// 1. **泛型设计**: 使用泛型 `M` 允许灵活的监控策略
/// 2. ** builder 模式**: 通过 `new()` 方法设置所有配置
/// 3. **状态追踪**: 使用 `Option<u32>` 追踪子进程是否已启动
///
/// ## 示例
///
/// ```rust,ignore
/// let supervisor = Supervisor::new(
///     "/path/to/exe",
///     vec!["arg1", "arg2"],
///     vec![("KEY", "value")],
///     "/path/to/logs",
///     my_monitor,
/// );
/// ```
pub struct Supervisor<M>
where
    M: ProcessMonitor + Send,
{
    /// 子进程可执行文件的路径
    child_exe: PathBuf,

    /// 传递给子进程的命令行参数
    child_args: Vec<String>,

    /// 子进程的进程 ID（启动后设置）
    child_pid: Option<u32>,

    /// 子进程的环境变量列表
    child_envs: Vec<(String, String)>,

    /// 日志目录路径
    log_dir: PathBuf,

    /// 进程监控器实例
    process_monitor: M,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl<M> Supervisor<M>
where
    M: ProcessMonitor + Send,
{
    /// ### 创建新的监督者实例
    ///
    /// 这是 `Supervisor` 的主要构造函数，使用 builder 模式设置所有配置。
    ///
    /// ## 参数
    ///
    /// - `child_exe`: 子进程可执行文件的路径（任何可转换为 `PathBuf` 的类型）
    /// - `child_args`: 命令行参数迭代器（任何可转换为 `String` 的类型）
    /// - `child_envs`: 环境变量迭代器，每个元素是 `(key, value)` 对
    /// - `log_dir`: 日志目录路径
    /// - `process_monitor`: 进程监控器实例
    ///
    /// ## 泛型约束说明
    ///
    /// ```rust,ignore
    /// child_args: impl IntoIterator<Item = impl Into<String>>
    /// ```
    /// 这种设计允许传入各种类型的集合：
    /// - `Vec<String>`
    /// - `Vec<&str>`
    /// - `&[String]`
    /// - `&[&str]`
    ///
    /// 环境变量同理：
    /// ```rust,ignore
    /// child_envs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>
    /// ```
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `Supervisor` 实例，`child_pid` 初始为 `None`。
    ///
    /// ## 示例
    ///
    /// ```rust,ignore
    /// // 使用 Vec<String>
    /// let supervisor = Supervisor::new(
    ///     "/path/to/msbrun",
    ///     vec!["run".to_string(), "--config".to_string()],
    ///     vec![("RUST_LOG".to_string(), "info".to_string())],
    ///     "/var/log/msb",
    ///     monitor,
    /// );
    ///
    /// // 使用 &str（更简洁）
    /// let supervisor = Supervisor::new(
    ///     "/path/to/msbrun",
    ///     vec!["run", "--config"],
    ///     vec![("RUST_LOG", "info")],
    ///     "/var/log/msb",
    ///     monitor,
    /// );
    /// ```
    pub fn new(
        child_exe: impl Into<PathBuf>,
        child_args: impl IntoIterator<Item = impl Into<String>>,
        child_envs: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
        log_dir: impl Into<PathBuf>,
        process_monitor: M,
    ) -> Self {
        Self {
            child_exe: child_exe.into(),
            child_args: child_args.into_iter().map(Into::into).collect(),
            child_envs: child_envs
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
            child_pid: None,  // 初始为 None，启动后设置
            log_dir: log_dir.into(),
            process_monitor,
        }
    }

    /// ### 启动监督者和子进程
    ///
    /// 这是监督者的主要运行方法，会：
    /// 1. 创建日志目录
    /// 2. 初始化轮转日志
    /// 3. 检测终端类型并选择 IO 模式
    /// 4. 启动子进程
    /// 5. 启动进程监控器
    /// 6. 等待子进程退出或收到终止信号
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 子进程正常退出
    /// - `Err(MicrosandboxUtilsError)`: 启动或运行过程中发生错误
    ///
    /// ## 可能的错误
    ///
    /// - 日志目录创建失败
    /// - PTY 创建失败（nix 错误）
    /// - 子进程启动失败
    /// - 信号处理设置失败
    ///
    /// ## 阻塞行为
    ///
    /// 此方法是**异步阻塞**的：
    /// - 使用 `tokio::select!` 等待多个事件
    /// - 不会阻塞 tokio 运行时（其他任务可以继续运行）
    /// - 直到子进程退出或收到信号才返回
    ///
    /// ## 详细流程
    ///
    /// ```text
    /// start()
    ///   │
    ///   ├─> 1. 创建日志目录 (create_dir_all)
    ///   │
    ///   ├─> 2. 初始化轮转日志 (RotatingLog::new)
    ///   │
    ///   ├─> 3. 检测终端类型
    ///   │     │
    ///   │     ├─ 交互式 ──> 4a. 创建 PTY (openpty)
    ///   │     │             ├─ 设置非阻塞模式 (fcntl O_NONBLOCK)
    ///   │     │             ├─ 克隆 slave 文件描述符
    ///   │     │             ├─ 配置 Command 使用 slave 作为 stdio
    ///   │     │             ├─ 设置 session 和 controlling terminal
    ///   │     │             └─ 创建 TTY ChildIo
    ///   │     │
    ///   │     └─ 非交互式 ──> 4b. 创建管道
    ///   │                   ├─ Command 使用 piped stdio
    ///   │                   └─ 创建 Piped ChildIo
    ///   │
    ///   ├─> 5. 启动子进程 (command.spawn())
    ///   │
    ///   ├─> 6. 保存 PID
    ///   │
    ///   ├─> 7. 启动 ProcessMonitor
    ///   │
    ///   ├─> 8. 设置信号处理器 (SIGTERM, SIGINT)
    ///   │
    ///   └─> 9. tokio::select! 等待事件
    ///         │
    ///         ├─ child.wait() ──> 子进程退出
    ///         │
    ///         ├─ sigterm.recv() ──> 收到 SIGTERM
    ///         │
    ///         └─ sigint.recv() ──> 收到 SIGINT
    /// ```
    pub async fn start(&mut self) -> MicrosandboxUtilsResult<()> {
        // === 步骤 1: 创建日志目录 ===
        // 如果目录不存在则创建
        // 如果已存在，create_dir_all 不会报错（幂等操作）
        create_dir_all(&self.log_dir).await?;

        // === 步骤 2: 初始化轮转日志 ===
        // 使用监督者日志文件名创建轮转日志
        // 注意：这里使用 _ 前缀表示有意忽略未使用的变量
        // 日志文件在作用域结束时自动关闭
        let _supervisor_log = RotatingLog::new(self.log_dir.join(SUPERVISOR_LOG_FILENAME)).await?;

        // === 步骤 3-4: 检测终端类型并启动子进程 ===
        // 根据终端类型选择不同的 IO 模式
        let (mut child, child_io) = if term::is_interactive_terminal() {
            // ========== TTY 模式（交互式）==========
            tracing::info!("running in an interactive terminal");

            // --- 创建伪终端（PTY）---
            // openpty() 创建一对连接的 PTY 设备：
            // - master: 由监督者控制
            // - slave: 被子进程用作终端
            let pty = openpty(None, None)?;

            // --- 设置 master 为非阻塞模式 ---
            // 这是异步 IO 的必要条件
            // F_GETFL: 获取文件状态标志
            // F_SETFL: 设置文件状态标志
            // O_NONBLOCK: 非阻塞标志
            {
                let flags = OFlag::from_bits_truncate(fcntl(&pty.master, FcntlArg::F_GETFL)?);
                let new_flags = flags | OFlag::O_NONBLOCK;
                fcntl(&pty.master, FcntlArg::F_SETFL(new_flags))?;
            }

            // --- 克隆 slave 文件描述符 ---
            // 需要三个独立的文件描述符用于 stdin/stdout/stderr
            let slave_in = pty.slave.try_clone()?;
            let slave_out = pty.slave.try_clone()?;
            let slave_err = pty.slave;

            // --- 配置子进程命令 ---
            let mut command = Command::new(&self.child_exe);
            command
                .args(&self.child_args)
                .envs(self.child_envs.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::from(slave_in))
                .stdout(Stdio::from(slave_out))
                .stderr(Stdio::from(slave_err));

            // --- 设置子进程的 session 和 controlling terminal ---
            // 这些配置必须在子进程 fork 之后、exec 之前执行
            // 使用 pre_exec 闭包实现
            //
            // setsid(): 创建新的会话，使子进程成为会话 leader
            // TIOCSCTTY: ioctl 命令，设置 controlling terminal
            //
            // unsafe 说明：
            // pre_exec 在子进程中执行，此时只有一个线程
            // 必须确保不调用任何非 async-signal-safe 的函数
            unsafe {
                command.pre_exec(|| {
                    // 创建新会话
                    libc::setsid();
                    // 设置 controlling terminal
                    // STDIN_FILENO: 标准输入的文件描述符 (0)
                    // TIOCSCTTY: ioctl 命令，设置 controlling terminal
                    // 1: 强制设置，即使已经有 controlling terminal
                    if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 1 as libc::c_long) < 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }

            // --- 启动子进程 ---
            let child = command.spawn()?;

            // --- 设置 master 文件句柄 ---
            // 需要为读写创建独立的文件句柄
            let master_fd_owned = pty.master;
            // dup() 创建文件描述符的副本
            let master_write_fd = nix::unistd::dup(&master_fd_owned)?;

            // 从原始文件描述符创建 File
            // from_raw_fd: 获取所有权（原文件描述符不再有效）
            // into_raw_fd: 放弃所有权（返回原始文件描述符）
            let master_read_file =
                unsafe { std::fs::File::from_raw_fd(master_fd_owned.into_raw_fd()) };
            let master_write_file =
                unsafe { std::fs::File::from_raw_fd(master_write_fd.into_raw_fd()) };

            // 创建异步读取文件描述符
            let master_read = AsyncFd::new(master_read_file)?;
            // 同步写入文件句柄（tokio::fs::File）
            let master_write = File::from_std(master_write_file);

            // 创建 TTY ChildIo
            let child_io = ChildIo::TTY {
                master_read,
                master_write,
            };

            (child, child_io)
        } else {
            // ========== 管道模式（非交互式）==========
            tracing::info!("running in a non-interactive terminal");

            // --- 启动子进程，使用管道 ---
            let mut child = Command::new(&self.child_exe)
                .args(&self.child_args)
                .envs(self.child_envs.iter().map(|(k, v)| (k, v)))
                .stdin(Stdio::piped())    // 创建 stdin 管道
                .stdout(Stdio::piped())   // 创建 stdout 管道
                .stderr(Stdio::piped())   // 创建 stderr 管道
                .spawn()?;

            // --- 获取管道所有权 ---
            // take() 方法从 Option 中取出值，原位置设为 None
            let stdin = child.stdin.take();
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            // 创建 Piped ChildIo
            let child_io = ChildIo::Piped {
                stdin,
                stdout,
                stderr,
            };

            (child, child_io)
        };

        // === 步骤 5: 保存子进程 PID ===
        // id() 方法返回子进程的 PID
        // expect() 在获取失败时 panic（不应该发生）
        let child_pid = child.id().expect("failed to get child process id");
        self.child_pid = Some(child_pid);

        // === 步骤 6: 启动进程监控器 ===
        self.process_monitor.start(child_pid, child_io).await?;

        // === 步骤 7: 设置信号处理器 ===
        // signal() 创建 Unix 信号的异步流
        let mut sigterm = signal(SignalKind::terminate())?;  // SIGTERM
        let mut sigint = signal(SignalKind::interrupt())?;   // SIGINT

        // === 步骤 8: 等待事件 ===
        // tokio::select! 并发等待多个异步事件，哪个先完成就执行哪个分支
        tokio::select! {
            // --- 分支 1: 子进程退出 ---
            status = child.wait() => {
                // 停止进程监控
                self.process_monitor.stop().await?;

                tracing::info!("child process {} exited", child_pid);

                // 检查退出状态
                if status.is_ok() {
                    if let Ok(status) = status {
                        if status.success() {
                            tracing::info!(
                                "child process {} exited successfully",
                                child_pid
                            );
                        } else {
                            tracing::error!(
                                "child process {} exited with status: {:?}",
                                child_pid,
                                status
                            );
                        }
                    }
                } else {
                    tracing::error!(
                        "failed to wait for child process {}: {:?}",
                        child_pid,
                        status
                    );
                }
            }

            // --- 分支 2: 收到 SIGTERM ---
            _ = sigterm.recv() => {
                // 停止进程监控
                self.process_monitor.stop().await?;

                tracing::info!("received SIGTERM signal");

                // 向子进程发送 SIGTERM
                if let Some(pid) = self.child_pid.take()
                    && let Err(e) = nix::sys::signal::kill(Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGTERM) {
                    tracing::error!("failed to send SIGTERM to process {pid}: {e}");
                }

                // 等待子进程退出
                if let Err(e) = child.wait().await {
                    tracing::error!("error waiting for child after SIGTERM: {e}");
                }
            }

            // --- 分支 3: 收到 SIGINT ---
            _ = sigint.recv() => {
                // 停止进程监控
                self.process_monitor.stop().await?;

                tracing::info!("received SIGINT signal");

                // 向子进程发送 SIGTERM（注意：不是 SIGINT）
                if let Some(pid) = self.child_pid.take()
                    && let Err(e) = nix::sys::signal::kill(Pid::from_raw(pid as i32), nix::sys::signal::Signal::SIGTERM) {
                    tracing::error!("failed to send SIGTERM to process {pid}: {e}");
                }

                // 等待子进程退出
                if let Err(e) = child.wait().await {
                    tracing::error!("error waiting for child after SIGINT: {e}");
                }
            }
        }

        // === 步骤 9: 清理状态 ===
        // 清除子进程 PID（表示进程已退出）
        self.child_pid = None;

        Ok(())
    }
}
