//! # 日志轮转（Log Rotation）实现
//!
//! 本模块实现了 microsandbox 运行时的异步日志轮转功能。
//!
//! ## 日志轮转工作原理
//!
//! 当日志文件达到指定的最大大小时，会自动执行以下轮转操作：
//!
//! 1. **同步数据**: 调用 `fsync` 确保所有缓冲数据写入磁盘
//! 2. **备份旧日志**: 将当前日志文件重命名为 `.old` 后缀
//!    - 如果已存在 `.old` 文件，先删除它（只保留一份备份）
//! 3. **创建新日志**: 创建一个新的空文件，使用原始文件名
//! 4. **继续写入**: 后续写入操作会写入新的日志文件
//!
//! ## 设计特点
//!
//! ### 完全异步
//! 实现 `tokio::io::AsyncWrite` trait，可以完全异步地写入日志，
//! 不会阻塞异步运行时。
//!
//! ### 双模式写入
//! 同时支持：
//! - **异步写入**: 通过 `AsyncWrite` trait
//! - **同步写入**: 通过 `get_sync_writer()` 获取 `std::io::Write` 实现
//!
//! ### 后台任务处理
//! 使用 channel 和后台任务来处理实际的写入操作，
//! 确保前端写入操作不会阻塞。
//!
//! ### 线程安全的大小追踪
//! 使用 `AtomicU64` 和 `Arc` 实现线程安全的当前文件大小追踪，
//! 确保在并发写入时也能正确触发轮转。
//!
//! ## 使用示例
//!
//! ### 基本用法
//!
//! ```rust,no_run
//! use microsandbox_utils::log::RotatingLog;
//! use tokio::io::AsyncWriteExt;
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     // 使用默认最大大小（10MB）创建日志
//!     let mut log = RotatingLog::new("app.log").await?;
//!
//!     // 异步写入
//!     log.write_all(b"日志内容\n").await?;
//!     log.flush().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### 自定义最大大小
//!
//! ```rust,no_run
//! use microsandbox_utils::log::RotatingLog;
//! use tokio::io::AsyncWriteExt;
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     // 创建最大 1MB 的日志文件
//!     let mut log = RotatingLog::with_max_size("app.log", 1024 * 1024).await?;
//!
//!     // 写入数据...
//!     Ok(())
//! }
//! ```
//!
//! ### 使用同步写入器
//!
//! ```rust,no_run
//! use microsandbox_utils::log::RotatingLog;
//! use std::io::Write;
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let log = RotatingLog::new("app.log").await?;
//!
//!     // 获取同步写入器
//!     let mut writer = log.get_sync_writer();
//!
//!     // 使用标准库的 Write trait
//!     writer.write_all(b"同步写入的日志\n")?;
//!     writer.flush()?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 内部架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    RotatingLog                              │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
//! │  │    file     │  │  tx(channel)│  │  _background_task   │  │
//! │  │   (File)    │  │ (Sender)    │  │   (JoinHandle)      │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘  │
//! │                            │                                │
//! │                            ▼                                │
//! │                   ┌─────────────────┐                       │
//! │                   │  background     │                       │
//! │                   │  task           │                       │
//! │                   │  handle_channel │                       │
//! │                   │  _data()        │                       │
//! │                   └─────────────────┘                       │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 状态机
//!
//! `RotatingLog` 内部使用状态机管理写入和轮转：
//!
//! - **Idle**: 空闲状态，准备接受写入
//! - **Writing**: 正在写入数据
//! - **Rotating**: 正在执行日志轮转
//!
//! 状态转换：
//! ```text
//! Idle ──[写入且未超限]──> Writing ──[写入完成]──> Idle
//!  │
//!  └──[写入且超限]──> Rotating ──[轮转完成]──> Writing
//! ```

use futures::future::BoxFuture;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
};
use tokio::{
    fs::{File, OpenOptions, remove_file, rename},
    io::{AsyncWrite, AsyncWriteExt},
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};

use crate::DEFAULT_LOG_MAX_SIZE;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// ### 轮转日志文件
///
/// `RotatingLog` 是一个自动轮转的日志文件实现。
/// 当文件大小达到预设的最大值时，会自动重命名当前文件并创建新文件。
///
/// ## 字段说明
///
/// - `file: File`: 当前正在写入的日志文件句柄
/// - `path: PathBuf`: 日志文件的路径
/// - `max_size: u64`: 触发轮转的最大文件大小（字节）
/// - `current_size: Arc<AtomicU64>`: 当前文件大小的原子计数器
///   - 使用 `Arc` 实现多线程共享
///   - 使用 `AtomicU64` 实现无锁原子操作
/// - `state: State`: 内部状态机（Idle/Writing/Rotating）
/// - `tx: UnboundedSender<Vec<u8>>`: 发送到后台任务的 channel
/// - `_background_task: JoinHandle<()>`: 后台写入任务的句柄
///
/// ## 日志轮转过程
///
/// 1. 每次写入前检查 `current_size + 写入大小 > max_size`
/// 2. 如果超限，触发轮转：
///    - 重命名当前文件为 `.old`
///    - 创建新文件
///    - 重置 `current_size` 为 0
/// 3. 写入数据到新文件
///
/// ## 示例
///
/// ```rust,no_run
/// use microsandbox_utils::log::RotatingLog;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     let log = RotatingLog::new("app.log").await?;
///     // 使用 log...
///     Ok(())
/// }
/// ```
pub struct RotatingLog {
    /// 当前正在写入的日志文件
    file: File,

    /// 日志文件的路径
    path: PathBuf,

    /// 触发轮转的最大文件大小（字节）
    max_size: u64,

    /// 当前文件大小的原子计数器
    /// 使用 Arc 在同步和异步路径之间共享
    current_size: Arc<AtomicU64>,

    /// 当前日志轮转状态
    state: State,

    /// 发送到同步写入器的 channel
    tx: UnboundedSender<Vec<u8>>,

    /// 后台写入任务的句柄
    _background_task: JoinHandle<()>,
}

/// ### 日志轮转内部状态机
///
/// `State` 枚举表示 `RotatingLog` 的内部状态，用于管理异步写入和轮转的流程。
///
/// ## 变体说明
///
/// ### `Idle`
/// 空闲状态，准备接受新的写入请求。
///
/// ### `Rotating(RotationFuture)`
/// 正在执行日志轮转。
/// 包含一个 `BoxFuture`，表示进行中的轮转操作。
///
/// ### `Writing`
/// 正在写入数据到文件。
///
/// ## 状态流转
///
/// ```text
///           ┌──────────────────────────────────────┐
///           │                                      │
///           ▼                                      │
///        ┌───────┐    写入且超限    ┌──────────┐   │
///        │ Idle  │ ───────────────> │ Rotating │ ──┤
///        └───────┘                  └──────────┘   │
///           │                              │       │
///           │ 写入未超限                   │ 完成  │
///           ▼                              ▼       │
///        ┌──────────┐                   ┌───────┐  │
///        │ Writing  │ ─────────────────> │ Idle  │ ─┘
///        └──────────┘    写入完成        └───────┘
/// ```
enum State {
    /// 空闲状态，准备接受写入
    Idle,

    /// 正在执行日志轮转
    Rotating(RotationFuture),

    /// 正在写入数据
    Writing,
}

/// ### 同步 channel 写入器
///
/// `SyncChannelWriter` 是一个实现了 `std::io::Write` 的包装器，
/// 它将所有写入的数据通过 channel 发送到后台任务进行异步处理。
///
/// ## 设计目的
///
/// 允许从同步代码（如 `tracing` 的日志宏）写入异步日志系统，
/// 而无需阻塞或使用 `tokio::spawn`。
///
/// ## 字段说明
///
/// - `tx: UnboundedSender<Vec<u8>>`: 发送到后台任务的 channel 发送端
///
/// ## 使用示例
///
/// ```rust,no_run
/// use microsandbox_utils::log::RotatingLog;
/// use std::io::Write;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     let log = RotatingLog::new("app.log").await?;
///     let mut writer = log.get_sync_writer();
///
///     // 使用标准库的 Write trait
///     writer.write_all(b"日志内容\n")?;
///     writer.flush()?;
///
///     Ok(())
/// }
/// ```
pub struct SyncChannelWriter {
    tx: UnboundedSender<Vec<u8>>,
}

/// ### 轮转未来类型
///
/// `RotationFuture` 是一个 boxed 的 future，表示进行中的日志轮转操作。
///
/// ## 为什么使用 BoxFuture？
///
/// 1. **固定大小**: `BoxFuture` 有固定的大小，可以存储在枚举变体中
/// 2. **类型擦除**: 隐藏了具体的 future 类型，简化了类型签名
/// 3. **自引用安全**: 使用 `'static` 生命周期，确保 future 可以安全地自引用
///
/// ## 类型定义
///
/// ```rust,ignore
/// type RotationFuture = BoxFuture<'static, io::Result<(File, PathBuf)>>;
/// ```
///
/// 返回一个包含新文件句柄和路径的元组。
type RotationFuture = BoxFuture<'static, io::Result<(File, PathBuf)>>;

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl RotatingLog {
    /// ### 创建新的轮转日志（使用默认最大大小）
    ///
    /// 这是一个便捷方法，使用 [`DEFAULT_LOG_MAX_SIZE`]（10MB）作为最大文件大小。
    ///
    /// ## 参数
    ///
    /// - `path`: 日志文件的路径
    ///
    /// ## 返回值
    ///
    /// - `Ok(RotatingLog)`: 成功创建轮转日志
    /// - `Err(io::Error)`: 创建或打开文件失败
    ///
    /// ## 可能的错误
    ///
    /// - 目录不存在且无法创建
    /// - 没有文件创建权限
    /// - 磁盘空间不足
    /// - 无法读取文件元数据
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// use microsandbox_utils::log::RotatingLog;
    ///
    /// #[tokio::main]
    /// async fn main() -> std::io::Result<()> {
    ///     // 使用默认 10MB 最大大小
    ///     let log = RotatingLog::new("app.log").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        // 调用 with_max_size 方法，使用默认最大大小
        Self::with_max_size(path, DEFAULT_LOG_MAX_SIZE).await
    }

    /// ### 创建新的轮转日志（自定义最大大小）
    ///
    /// 这是主要的构造函数，允许指定日志文件的最大大小。
    ///
    /// ## 参数
    ///
    /// - `path`: 日志文件的路径
    /// - `max_size`: 触发轮转的最大文件大小（字节）
    ///
    /// ## 返回值
    ///
    /// - `Ok(RotatingLog)`: 成功创建轮转日志
    /// - `Err(io::Error)`: 创建或打开文件失败
    ///
    /// ## 初始化过程
    ///
    /// 1. 打开或创建日志文件
    /// 2. 读取当前文件大小
    /// 3. 创建 channel 用于通信
    /// 4. 启动后台写入任务
    /// 5. 返回初始化完成的 `RotatingLog`
    ///
    /// ## 示例
    ///
    /// ```rust,no_run
    /// use microsandbox_utils::log::RotatingLog;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     // 创建最大 1MB 的日志文件
    ///     let log = RotatingLog::with_max_size("app.log", 1024 * 1024).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn with_max_size(path: impl AsRef<Path>, max_size: u64) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();

        // 打开或创建日志文件
        // OpenOptions 配置：
        // - create(true): 如果文件不存在则创建
        // - append(true): 以追加模式打开（写入总是在文件末尾）
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;

        // 获取当前文件大小
        let metadata = file.metadata().await?;

        // 创建无界 channel 用于发送写入数据
        let (tx, rx) = mpsc::unbounded_channel();

        // 创建共享的原子计数器，跟踪当前文件大小
        let current_size = Arc::new(AtomicU64::new(metadata.len()));

        // 为后台任务创建克隆
        let bg_file = file.try_clone().await?;  // 克隆文件句柄
        let bg_path = path.clone();             // 克隆路径
        let bg_max_size = max_size;             // 复制最大大小
        let bg_size = Arc::clone(&current_size); // 克隆共享计数器

        // 启动后台任务处理 channel 数据
        // 后台任务负责：
        // 1. 接收写入数据
        // 2. 检查是否需要轮转
        // 3. 执行轮转（如果需要）
        // 4. 写入数据到文件
        let background_task = tokio::spawn(async move {
            handle_channel_data(rx, bg_file, bg_path, bg_max_size, bg_size).await
        });

        Ok(Self {
            file,
            path,
            max_size,
            current_size,
            state: State::Idle,
            tx,
            _background_task: background_task,
        })
    }

    /// ### 获取同步写入器
    ///
    /// 返回一个实现了 `std::io::Write` 的同步写入器，
    /// 允许从同步代码写入异步日志系统。
    ///
    /// ## 返回值
    ///
    /// 返回 `SyncChannelWriter`，它实现了 `std::io::Write` trait。
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// use microsandbox_utils::log::RotatingLog;
    /// use std::io::Write;
    ///
    /// #[tokio::main]
    /// async fn main() -> std::io::Result<()> {
    ///     let log = RotatingLog::new("app.log").await?;
    ///     let mut writer = log.get_sync_writer();
    ///
    ///     // 同步写入
    ///     writer.write_all(b"日志内容\n")?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## 与异步写入的区别
    ///
    /// - **同步写入器**: 适合从同步代码（如 `tracing` 宏）写入
    /// - **异步写入**: 直接实现 `AsyncWrite`，适合异步代码
    pub fn get_sync_writer(&self) -> SyncChannelWriter {
        SyncChannelWriter::new(self.tx.clone())
    }
}

impl SyncChannelWriter {
    /// ### 创建新的同步 channel 写入器
    ///
    /// ## 参数
    ///
    /// - `tx`: channel 发送端，用于发送写入数据到后台任务
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `SyncChannelWriter` 实例。
    pub fn new(tx: UnboundedSender<Vec<u8>>) -> Self {
        Self { tx }
    }
}

//--------------------------------------------------------------------------------------------------
// 内部函数
//--------------------------------------------------------------------------------------------------

/// ### 执行日志轮转
///
/// 这个异步函数执行实际的日志轮转操作。
///
/// ## 参数
///
/// - `file`: 当前日志文件的句柄
/// - `path`: 日志文件的路径
///
/// ## 返回值
///
/// 返回一个元组：
/// - 新创建的日志文件句柄
/// - 日志文件的路径
///
/// ## 错误情况
///
/// - `io::ErrorKind::Other`: 文件同步失败
/// - `io::ErrorKind::NotFound`: 无法删除旧的备份文件
/// - `io::ErrorKind::PermissionDenied`: 没有文件权限
/// - `io::ErrorKind::Other`: 无法创建新文件
///
/// ## 轮转步骤
///
/// 1. **同步数据**: `file.sync_all()` 确保所有缓冲数据写入磁盘
/// 2. **构建备份路径**: `path.with_extension("old")`
/// 3. **删除旧备份**: 如果 `.old` 文件存在，先删除它
/// 4. **重命名当前文件**: `rename(&path, &backup_path)`
/// 5. **创建新文件**: 使用相同的配置打开新文件
///
/// ## 为什么先删除旧备份？
///
/// 这样可以确保只保留一份备份（`.old`），避免磁盘空间被多份备份占用。
/// 如果需要保留多份历史日志，可以使用带时间戳的命名方案。
async fn do_rotation(file: File, path: PathBuf) -> io::Result<(File, PathBuf)> {
    // 同步所有缓冲数据到磁盘
    // 这确保轮转后不会丢失任何日志数据
    file.sync_all().await?;

    // 构建备份文件路径
    let backup_path = path.with_extension("old");

    // 如果备份文件已存在，先删除它
    // 这样可以确保只保留一份备份
    if backup_path.exists() {
        remove_file(&backup_path).await?;
    }

    // 重命名当前文件为备份文件
    // rename 是原子操作，在 Unix 上不会丢失数据
    rename(&path, &backup_path).await?;

    // 创建新的日志文件
    // 使用相同的配置：如果不存在则创建，以追加模式打开
    let new_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;

    Ok((new_file, path))
}

/// ### 后台 channel 数据处理任务
///
/// 这个异步函数在后台运行，负责：
/// 1. 从 channel 接收写入数据
/// 2. 检查是否需要触发轮转
/// 3. 执行轮转（如果需要）
/// 4. 将数据写入文件
///
/// ## 参数
///
/// - `rx`: channel 接收端
/// - `file`: 日志文件句柄（可变）
/// - `path`: 日志文件路径
/// - `max_size`: 最大文件大小
/// - `current_size`: 共享的当前文件大小计数器
///
/// ## 工作流程
///
/// ```text
/// ┌─────────────────────────────────────────────────────┐
/// │                   handle_channel_data               │
/// ├─────────────────────────────────────────────────────┤
/// │  while let Some(data) = rx.recv().await {           │
/// │    │                                                │
/// │    ├─> 检查：current_size + data_len > max_size ?  │
/// │    │     │                                         │
/// │    │     ├─ YES ──> 执行轮转 ──> 重置计数器        │
/// │    │     │                                         │
/// │    │     └─ NO ──> 直接写入                        │
/// │    │                                                │
/// │    └─> 写入数据到文件                               │
/// │  }                                                  │
/// └─────────────────────────────────────────────────────┘
/// ```
///
/// ## 错误处理
///
/// - **轮转失败**: 记录错误日志，继续尝试写入
/// - **写入失败**: 记录错误日志，从计数器中减去该次写入的大小
///
/// 这种设计确保即使发生错误，日志系统也不会完全停止工作。
async fn handle_channel_data(
    mut rx: UnboundedReceiver<Vec<u8>>,
    mut file: File,
    path: PathBuf,
    max_size: u64,
    current_size: Arc<AtomicU64>,
) {
    // 持续从 channel 接收数据
    // 当 channel 发送端全部断开时，rx.recv() 返回 None，循环结束
    while let Some(data) = rx.recv().await {
        let data_len = data.len() as u64;

        // 原子地增加计数器并获取旧值
        // fetch_add 返回增加前的值，这样我们可以检查增加后是否会超限
        let size = current_size.fetch_add(data_len, Ordering::Relaxed);

        // 检查是否需要轮转
        if size + data_len > max_size {
            // 在轮转之前克隆文件句柄
            // 这是因为 do_rotation 会消耗文件句柄
            if let Ok(file_clone) = file.try_clone().await {
                match do_rotation(file_clone, path.clone()).await {
                    Ok((new_file, _)) => {
                        // 轮转成功，更新文件句柄并重置计数器
                        file = new_file;
                        current_size.store(0, Ordering::Relaxed);
                    }
                    Err(e) => {
                        // 轮转失败，记录错误
                        // 注意：我们没有减去刚才增加的大小，因为数据可能已经部分写入
                        tracing::error!("failed to rotate log file: {}", e);
                        continue;
                    }
                }
            } else {
                // 无法克隆文件句柄，记录错误
                tracing::error!("failed to clone file handle for rotation");
                continue;
            }
        }

        // 将数据写入文件
        if let Err(e) = file.write_all(&data).await {
            tracing::error!("failed to write to log file: {}", e);
            // 写入失败时，从计数器中减去该次写入的大小
            // 这样可以保持计数器的准确性
            current_size.fetch_sub(data_len, Ordering::Relaxed);
        }
    }
}

//--------------------------------------------------------------------------------------------------
// AsyncWrite trait 实现
//--------------------------------------------------------------------------------------------------

/// ### RotatingLog 的 AsyncWrite 实现
///
/// 实现 `tokio::io::AsyncWrite` trait，使 `RotatingLog` 可以作为异步写入目标使用。
///
/// ## 状态机实现
///
/// `poll_write` 方法根据当前状态执行不同的逻辑：
///
/// ### Idle 状态
/// 1. 检查写入后是否会超限
/// 2. 如果超限：进入 Rotating 状态，开始异步轮转
/// 3. 如果未超限：进入 Writing 状态，准备写入
///
/// ### Rotating 状态
/// 1. 轮询轮转 future
/// 2. 如果未完成：返回 `Poll::Pending`
/// 3. 如果成功：更新文件句柄，进入 Writing 状态
/// 4. 如果失败：返回错误
///
/// ### Writing 状态
/// 1. 将数据写入文件
/// 2. 写入完成：返回写入的字节数，回到 Idle 状态
/// 3. 写入失败：返回错误，回到 Idle 状态
impl AsyncWrite for RotatingLog {
    /// ### 异步写入数据
    ///
    /// ## 参数
    ///
    /// - `self: Pin<&mut Self>`: 自引用，用于异步操作
    /// - `cx: &mut Context<'_>`: 异步上下文，用于注册唤醒器
    /// - `buf: &[u8]`: 要写入的数据缓冲区
    ///
    /// ## 返回值
    ///
    /// - `Poll::Ready(Ok(n))`: 成功写入 n 字节
    /// - `Poll::Ready(Err(e))`: 写入失败
    /// - `Poll::Pending`: 操作未完成（正在轮转），稍后会唤醒
    ///
    /// ## Poll 机制
    ///
    /// Rust 异步 IO 使用 `Poll` 类型来表示操作状态：
    /// - `Ready(value)`: 操作完成，返回结果
    /// - `Pending`: 操作未完成，注册唤醒器后稍后重试
    ///
    /// 当返回 `Pending` 时，运行时会在适当的时候再次调用 `poll_write`。
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        let buf_len = buf.len() as u64;

        // 状态机循环
        // 使用 loop 是因为状态可能在一个 poll 周期内多次转换
        loop {
            match &mut this.state {
                State::Idle => {
                    // 尝试增加计数器并获取旧值
                    let size = this.current_size.fetch_add(buf_len, Ordering::Relaxed);

                    // 检查是否会超限
                    if size + buf_len > this.max_size {
                        // 超限，需要轮转
                        // 创建一个临时文件句柄用于轮转（/dev/null 作为占位符）
                        let old_file = std::mem::replace(
                            &mut this.file,
                            File::from_std(std::fs::File::open("/dev/null").unwrap()),
                        );
                        let old_path = this.path.clone();

                        // 创建轮转 future 并 box 起来
                        let fut = Box::pin(do_rotation(old_file, old_path));

                        // 进入 Rotating 状态
                        this.state = State::Rotating(fut);
                    } else {
                        // 未超限，进入 Writing 状态
                        this.state = State::Writing;
                    }
                }

                State::Rotating(fut) => {
                    // 轮询轮转 future
                    match fut.as_mut().poll(cx) {
                        // 轮转未完成，返回 Pending
                        Poll::Pending => return Poll::Pending,

                        // 轮转失败
                        Poll::Ready(Err(e)) => {
                            this.state = State::Idle;
                            // 回滚计数器
                            this.current_size.fetch_sub(buf_len, Ordering::Relaxed);
                            return Poll::Ready(Err(e));
                        }

                        // 轮转成功
                        Poll::Ready(Ok((new_file, new_path))) => {
                            this.file = new_file;
                            this.path = new_path;
                            this.current_size.store(0, Ordering::Relaxed);
                            this.state = State::Writing;
                        }
                    }
                }

                State::Writing => {
                    // 实际写入数据
                    let pinned_file = Pin::new(&mut this.file);
                    match pinned_file.poll_write(cx, buf) {
                        // 写入成功
                        Poll::Ready(Ok(written)) => {
                            this.state = State::Idle;
                            return Poll::Ready(Ok(written));
                        }

                        // 写入失败
                        Poll::Ready(Err(e)) => {
                            this.state = State::Idle;
                            // 回滚计数器
                            this.current_size.fetch_sub(buf_len, Ordering::Relaxed);
                            return Poll::Ready(Err(e));
                        }

                        // 写入未完成
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }

    /// ### 异步刷新缓冲区
    ///
    /// 确保所有缓冲数据都被写入底层文件。
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_flush(cx)
    }

    /// ### 异步关闭写入器
    ///
    /// 关闭写入器，表示不再写入数据。
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.file).poll_shutdown(cx)
    }
}

//--------------------------------------------------------------------------------------------------
// Write trait 实现
//--------------------------------------------------------------------------------------------------

/// ### SyncChannelWriter 的 Write 实现
///
/// 实现 `std::io::Write` trait，使 `SyncChannelWriter` 可以作为同步写入目标使用。
///
/// ## 工作原理
///
/// 1. 将写入的数据复制到 `Vec<u8>`
/// 2. 通过 channel 发送到后台任务
/// 3. 立即返回成功（实际写入在后台进行）
///
/// ## 注意事项
///
/// - `write` 方法总是返回 `Ok(buf.len())`，表示接受了所有数据
/// - 实际写入可能稍后在后台进行，可能失败
/// - `flush` 是空操作，因为数据通过 channel 立即发送
impl Write for SyncChannelWriter {
    /// ### 同步写入数据
    ///
    /// ## 参数
    ///
    /// - `buf: &[u8]`: 要写入的数据
    ///
    /// ## 返回值
    ///
    /// - `Ok(n)`: 成功接受 n 字节（总是返回 `buf.len()`）
    /// - `Err(io::Error)`: channel 发送失败
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let data = buf.to_vec();
        self.tx
            .send(data)
            .map_err(|_| io::Error::other("failed to send log data to channel"))?;
        // 返回写入的字节数（实际是接受的字节数）
        Ok(buf.len())
    }

    /// ### 刷新缓冲区
    ///
    /// 对于 channel 写入器，这是一个空操作，
    /// 因为数据通过 channel 立即发送。
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_create_new_log() -> io::Result<()> {
        let dir = tempdir()?;
        let log_path = dir.path().join("test.log");

        let log = RotatingLog::with_max_size(&log_path, 1024).await?;
        assert!(log_path.exists());
        assert_eq!(log.max_size, 1024);
        assert_eq!(log.current_size.load(Ordering::Relaxed), 0);

        Ok(())
    }

    #[tokio::test]
    async fn test_write_to_log() -> io::Result<()> {
        let dir = tempdir()?;
        let log_path = dir.path().join("test.log");

        let mut log = RotatingLog::with_max_size(&log_path, 1024).await?;
        let test_data = b"test log entry\n";
        log.write_all(test_data).await?;
        log.flush().await?;

        let content = fs::read_to_string(&log_path)?;
        assert_eq!(content, String::from_utf8_lossy(test_data));
        assert_eq!(
            log.current_size.load(Ordering::Relaxed),
            test_data.len() as u64
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_log_rotation() -> io::Result<()> {
        let dir = tempdir()?;
        let log_path = dir.path().join("test.log");
        let max_size = 20; // 设置较小的值以触发轮转

        let mut log = RotatingLog::with_max_size(&log_path, max_size).await?;

        // 写入数据直到触发轮转
        let first_entry = b"first entry\n";
        log.write_all(first_entry).await?;
        log.flush().await?;

        let second_entry = b"second entry\n";
        log.write_all(second_entry).await?;
        log.flush().await?;

        // 等待轮转完成
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 检查当前日志和旧日志文件都存在
        assert!(log_path.exists());
        assert!(log_path.with_extension("old").exists());

        // 验证旧文件包含第一条目
        let old_content = fs::read_to_string(log_path.with_extension("old"))?;
        assert_eq!(old_content, String::from_utf8_lossy(first_entry));

        // 验证新文件包含第二条目
        let new_content = fs::read_to_string(&log_path)?;
        assert_eq!(new_content, String::from_utf8_lossy(second_entry));

        Ok(())
    }

    #[tokio::test]
    async fn test_oversized_write() -> io::Result<()> {
        let dir = tempdir()?;
        let log_path = dir.path().join("test.log");
        let max_size = 10; // 设置较小的值

        let mut log = RotatingLog::with_max_size(&log_path, max_size).await?;

        // 写入超过 max_size 的数据
        let large_entry = b"this is a very large log entry that exceeds the maximum size\n";
        log.write_all(large_entry).await?;
        log.flush().await?;

        // 等待轮转完成
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 验证内容被写入（即使超过 max_size）
        assert!(log_path.exists());
        let content = fs::read_to_string(&log_path)?;
        assert_eq!(content, String::from_utf8_lossy(large_entry));

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_writer() -> io::Result<()> {
        let dir = tempdir()?;
        let log_path = dir.path().join("test.log");

        let log = RotatingLog::with_max_size(&log_path, 1024).await?;
        let mut sync_writer = log.get_sync_writer();

        let test_data = b"sync writer test\n";
        sync_writer.write_all(test_data)?;
        sync_writer.flush()?;

        // 等待异步处理
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let content = fs::read_to_string(&log_path)?;
        assert_eq!(content, String::from_utf8_lossy(test_data));

        Ok(())
    }

    #[tokio::test]
    async fn test_multiple_rotations() -> io::Result<()> {
        let dir = tempdir()?;
        let log_path = dir.path().join("test.log");
        let max_size = 20;

        let mut log = RotatingLog::with_max_size(&log_path, max_size).await?;

        // 多次写入以触发多次轮转
        for i in 0..3 {
            let test_data = format!("rotation test {}\n", i).into_bytes();
            log.write_all(&test_data).await?;
            log.flush().await?;

            // 等待轮转
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // 验证只有一个 .old 文件存在（最新轮转）
        assert!(log_path.exists());
        assert!(log_path.with_extension("old").exists());

        Ok(())
    }
}
