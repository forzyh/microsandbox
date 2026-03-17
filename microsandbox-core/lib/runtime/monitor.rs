//! MicroVM 进程监控器
//!
//! 本模块提供了用于监控 MicroVM 进程运行状态的核心功能。
//! MicroVmMonitor 负责监督沙箱进程的生命周期，包括启动、停止和日志管理。
//!
//! ## 主要功能
//!
//! - **进程监控** - 跟踪 MicroVM 进程的运行状态
//! - **数据库同步** - 在数据库中更新沙箱指标和元数据
//! - **日志管理** - 使用轮转日志记录进程输出
//! - **终端处理** - 支持 TTY 模式和管道 I/O 两种模式
//! - **信号处理** - 优雅地停止沙箱进程

use std::{
    io::{Read, Write},
    os::fd::BorrowedFd,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use microsandbox_utils::{
    ChildIo, LOG_SUFFIX, MicrosandboxUtilsError, MicrosandboxUtilsResult, ProcessMonitor,
    RotatingLog,
};
use sqlx::{Pool, Sqlite};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{MicrosandboxResult, management::db, vm::Rootfs};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// 沙箱运行时的状态常量
pub const SANDBOX_STATUS_RUNNING: &str = "RUNNING";

/// 沙箱停止时的状态常量
pub const SANDBOX_STATUS_STOPPED: &str = "STOPPED";

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// MicroVM 进程监控器
///
/// MicroVmMonitor 负责监督 MicroVM 沙箱进程的完整生命周期。
/// 它追踪进程状态、管理日志输出，并在数据库中记录沙箱指标。
///
/// ## 字段说明
/// * `sandbox_db` - 用于追踪沙箱指标和元数据的 SQLite 数据库连接池
/// * `sandbox_name` - 沙箱名称
/// * `config_file` - 沙箱配置文件路径
/// * `config_last_modified` - 配置文件最后修改时间
/// * `supervisor_pid` - Supervisor 进程的 PID
/// * `log_path` - MicroVM 日志文件路径
/// * `log_dir` - 日志目录
/// * `rootfs` - 根文件系统配置
/// * `original_term` - 原始终端设置（TTY 模式下保存）
/// * `forward_output` - 是否将输出转发到 stdout/stderr
pub struct MicroVmMonitor {
    /// 用于追踪沙箱指标和元数据的数据库
    sandbox_db: Pool<Sqlite>,

    /// 沙箱名称
    sandbox_name: String,

    /// 沙箱配置文件路径
    config_file: String,

    /// 配置文件的最后修改时间
    config_last_modified: DateTime<Utc>,

    /// Supervisor 进程 ID
    supervisor_pid: u32,

    /// MicroVM 日志文件路径
    log_path: Option<PathBuf>,

    /// 日志目录路径
    log_dir: PathBuf,

    /// 根文件系统配置
    rootfs: Rootfs,

    /// 原始终端设置（TTY 模式下使用）
    original_term: Option<nix::sys::termios::Termios>,

    /// 是否将输出转发到父进程的 stdout/stderr
    forward_output: bool,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl MicroVmMonitor {
    /// 创建新的 MicroVM 监控器
    ///
    /// ## 参数
    /// * `supervisor_pid` - Supervisor 进程 ID
    /// * `sandbox_db_path` - 沙箱数据库路径
    /// * `sandbox_name` - 沙箱名称
    /// * `config_file` - 配置文件路径
    /// * `config_last_modified` - 配置文件最后修改时间
    /// * `log_dir` - 日志目录
    /// * `rootfs` - 根文件系统配置
    /// * `forward_output` - 是否转发输出
    ///
    /// ## 返回值
    /// 返回新创建的 MicroVmMonitor 实例
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        supervisor_pid: u32,
        sandbox_db_path: impl AsRef<Path>,
        sandbox_name: String,
        config_file: String,
        config_last_modified: DateTime<Utc>,
        log_dir: impl Into<PathBuf>,
        rootfs: Rootfs,
        forward_output: bool,
    ) -> MicrosandboxResult<Self> {
        Ok(Self {
            supervisor_pid,
            sandbox_db: db::get_pool(sandbox_db_path.as_ref()).await?,
            sandbox_name,
            config_file,
            config_last_modified,
            log_path: None,
            log_dir: log_dir.into(),
            rootfs,
            original_term: None,
            forward_output,
        })
    }

    /// 恢复终端设置
    ///
    /// 如果在 TTY 模式下修改了终端设置，此方法会恢复到原始设置。
    /// 这确保了在监控器停止后，用户的终端能恢复正常行为。
    fn restore_terminal_settings(&mut self) {
        if let Some(original_term) = self.original_term.take()
            && let Err(e) = nix::sys::termios::tcsetattr(
                unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) },
                nix::sys::termios::SetArg::TCSANOW,
                &original_term,
            )
        {
            tracing::warn!(error = %e, "failed to restore terminal settings in restore_terminal_settings");
        }
    }

    /// 生成层级日志文件路径
    ///
    /// 生成格式为 `<log_dir>/<config_file>/<sandbox_name>.<LOG_SUFFIX>` 的路径。
    /// 这种目录结构通过配置文件和沙箱名称对日志进行命名空间隔离。
    ///
    /// ## 返回值
    /// 返回完整的日志文件路径
    fn generate_log_path(&self) -> PathBuf {
        // 为配置文件创建目录
        let config_dir = self.log_dir.join(&self.config_file);
        // 在目录内放置以沙箱名称命名的日志文件
        config_dir.join(format!("{}.{}", self.sandbox_name, LOG_SUFFIX))
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

#[async_trait]
impl ProcessMonitor for MicroVmMonitor {
    /// 启动进程监控
    ///
    /// 此方法初始化日志记录，设置数据库追踪，并启动 I/O 处理任务。
    /// 支持两种 I/O 模式：Piped（管道）和 TTY（终端）。
    ///
    /// ## 参数
    /// * `pid` - 要监控的进程 ID
    /// * `child_io` - 子进程的 I/O 配置（管道或 TTY）
    ///
    /// ## 返回值
    /// 成功返回 Ok(())，失败返回错误
    ///
    /// ## 处理流程
    /// 1. 生成日志路径并确保父目录存在
    /// 2. 创建轮转日志处理器
    /// 3. 在数据库中保存/更新沙箱信息
    /// 4. 根据 I/O 模式启动相应的处理任务：
    ///    - Piped 模式：分别处理 stdin/stdout/stderr
    ///    - TTY 模式：处理主从终端之间的双向通信
    async fn start(&mut self, pid: u32, child_io: ChildIo) -> MicrosandboxUtilsResult<()> {
        // 生成带有目录层级隔离的日志路径
        let log_path = self.generate_log_path();

        // 确保父目录存在
        if let Some(parent) = log_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let microvm_log =
            std::sync::Arc::new(tokio::sync::Mutex::new(RotatingLog::new(&log_path).await?));
        let microvm_pid = pid;

        self.log_path = Some(log_path);

        // 获取根文件系统路径
        let rootfs_paths = match &self.rootfs {
            Rootfs::Native(path) => format!("native:{}", path.to_string_lossy().into_owned()),
            Rootfs::Overlayfs(paths) => format!(
                "overlayfs:{}",
                paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<String>>()
                    .join(":")
            ),
        };

        // 将沙箱条目插入数据库
        db::save_or_update_sandbox(
            &self.sandbox_db,
            &self.sandbox_name,
            &self.config_file,
            &self.config_last_modified,
            SANDBOX_STATUS_RUNNING,
            self.supervisor_pid,
            microvm_pid,
            &rootfs_paths,
        )
        .await
        .map_err(MicrosandboxUtilsError::custom)?;

        match child_io {
            // 管道 I/O 模式：分别处理 stdin、stdout、stderr
            ChildIo::Piped {
                stdin,
                stdout,
                stderr,
            } => {
                // 处理 stdout 日志记录
                if let Some(mut stdout) = stdout {
                    let log = microvm_log.clone();
                    let forward_output = self.forward_output;
                    tokio::spawn(async move {
                        let mut buf = [0u8; 8192]; // NOTE(appcypher): Using 8192 as buffer size because ChatGPT recommended it lol
                        while let Ok(n) = stdout.read(&mut buf).await {
                            if n == 0 {
                                break;
                            }
                            // 写入日志文件
                            let mut log_guard = log.lock().await;
                            if let Err(e) = log_guard.write_all(&buf[..n]).await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to write to microvm stdout log");
                            }
                            if let Err(e) = log_guard.flush().await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to flush microvm stdout log");
                            }

                            // 如果启用，同时转发到父进程的 stdout
                            if forward_output {
                                print!("{}", String::from_utf8_lossy(&buf[..n]));
                                // 刷新 stdout 以防数据缓冲
                                if let Err(e) = std::io::stdout().flush() {
                                    tracing::warn!(error = %e, "failed to flush parent stdout");
                                }
                            }
                        }
                    });
                }

                // 处理 stderr 日志记录
                if let Some(mut stderr) = stderr {
                    let log = microvm_log.clone();
                    let forward_output = self.forward_output;
                    tokio::spawn(async move {
                        let mut buf = [0u8; 8192]; // NOTE(appcypher): Using 8192 as buffer size because ChatGPT recommended it lol
                        while let Ok(n) = stderr.read(&mut buf).await {
                            if n == 0 {
                                break;
                            }
                            // 写入日志文件
                            let mut log_guard = log.lock().await;
                            if let Err(e) = log_guard.write_all(&buf[..n]).await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to write to microvm stderr log");
                            }
                            if let Err(e) = log_guard.flush().await {
                                tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to flush microvm stderr log");
                            }

                            // 如果启用，同时转发到父进程的 stderr
                            if forward_output {
                                eprint!("{}", String::from_utf8_lossy(&buf[..n]));
                                // 刷新 stderr 以防数据缓冲
                                if let Err(e) = std::io::stderr().flush() {
                                    tracing::warn!(error = %e, "failed to flush parent stderr");
                                }
                            }
                        }
                    });
                }

                // 处理从父进程到子进程的 stdin 流式传输
                if let Some(mut child_stdin) = stdin {
                    tokio::spawn(async move {
                        let mut parent_stdin = tokio::io::stdin();
                        if let Err(e) = tokio::io::copy(&mut parent_stdin, &mut child_stdin).await {
                            tracing::warn!(error = %e, "failed to copy parent stdin to child stdin");
                        }
                    });
                }
            }
            // TTY 模式：处理伪终端的双向通信
            ChildIo::TTY {
                master_read,
                mut master_write,
            } => {
                // 处理 TTY I/O
                // 将终端设置为原始模式（raw mode）
                let term = nix::sys::termios::tcgetattr(unsafe {
                    BorrowedFd::borrow_raw(libc::STDIN_FILENO)
                })?;
                self.original_term = Some(term.clone());
                let mut raw_term = term.clone();
                nix::sys::termios::cfmakeraw(&mut raw_term);
                nix::sys::termios::tcsetattr(
                    unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) },
                    nix::sys::termios::SetArg::TCSANOW,
                    &raw_term,
                )?;

                // 生成异步任务从主设备读取输出
                let log = microvm_log.clone();
                let forward_output = self.forward_output;
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    loop {
                        let mut read_guard = match master_read.readable().await {
                            Ok(guard) => guard,
                            Err(e) => {
                                tracing::warn!(error = %e, "error waiting for master fd to become readable");
                                break;
                            }
                        };

                        match read_guard.try_io(|inner| inner.get_ref().read(&mut buf)) {
                            Ok(Ok(0)) => break, // 已到达 EOF
                            Ok(Ok(n)) => {
                                // 写入日志文件
                                let mut log_guard = log.lock().await;
                                if let Err(e) = log_guard.write_all(&buf[..n]).await {
                                    tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to write to microvm tty log");
                                }
                                if let Err(e) = log_guard.flush().await {
                                    tracing::error!(microvm_pid = microvm_pid, error = %e, "failed to flush microvm tty log");
                                }

                                // 如果启用，打印子进程的 output
                                if forward_output {
                                    print!("{}", String::from_utf8_lossy(&buf[..n]));
                                    // 刷新 stdout 以防数据缓冲
                                    std::io::stdout().flush().ok();
                                }
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(error = %e, "error reading from master fd");
                                break;
                            }
                            Err(_) => continue,
                        }
                    }
                });

                // 生成异步任务将父进程的 stdin 复制到主设备
                tokio::spawn(async move {
                    let mut stdin = tokio::io::stdin();
                    if let Err(e) = tokio::io::copy(&mut stdin, &mut master_write).await {
                        tracing::warn!(error = %e, "error copying stdin to master fd");
                    }
                });
            }
        }

        Ok(())
    }

    /// 停止进程监控
    ///
    /// 此方法执行清理操作，包括：
    /// 1. 恢复终端设置（如果之前被修改）
    /// 2. 更新数据库中的沙箱状态为 STOPPED
    /// 3. 重置日志路径
    ///
    /// ## 返回值
    /// 成功返回 Ok(())，失败返回错误
    async fn stop(&mut self) -> MicrosandboxUtilsResult<()> {
        // 恢复终端设置（如果被修改过）
        self.restore_terminal_settings();

        // 更新沙箱状态为已停止
        db::update_sandbox_status(
            &self.sandbox_db,
            &self.sandbox_name,
            &self.config_file,
            SANDBOX_STATUS_STOPPED,
        )
        .await
        .map_err(MicrosandboxUtilsError::custom)?;

        // 重置日志路径
        self.log_path = None;

        Ok(())
    }
}

/// Drop 实现确保终端设置被恢复
///
/// 当监控器被释放时（无论正常还是异常退出），
/// 都会尝试恢复原始终端设置，确保用户终端不被破坏。
impl Drop for MicroVmMonitor {
    fn drop(&mut self) {
        self.restore_terminal_settings();
    }
}
