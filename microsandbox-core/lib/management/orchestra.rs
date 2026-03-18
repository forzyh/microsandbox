//! Microsandbox 的编排（Orchestra）管理功能模块
//!
//! 本模块提供了协调管理多个沙箱集合的功能，
//! 类似于容器编排工具（如 Kubernetes、Docker Compose）管理多个容器的方式。
//! 它处理配置中定义的多个沙箱的完整生命周期，包括启动、关闭和应用配置变更。
//!
//! ## 核心功能
//!
//! 本模块提供的主要操作包括：
//!
//! - **`up`**: 启动配置文件中定义的所有（或指定的）沙箱
//!   - 支持分离模式（detached）和附加模式（attached）
//!   - 附加模式下提供带彩色前缀的多路复用输出
//!   - 自动跳过已经运行的沙箱
//!
//! - **`down`**: 优雅地关闭所有（或指定的）正在运行的沙箱
//!   - 通过发送 SIGTERM 信号实现优雅关闭
//!   - 只关闭配置文件中定义且正在运行的沙箱
//!
//! - **`apply`**: 使运行中的沙箱与配置保持一致（reconcile）
//!   - 启动配置中存在但未运行的沙箱
//!   - 关闭运行中但配置中不存在的沙箱
//!   - 用于配置变更后的自动同步
//!
//! ## 状态管理
//!
//! 本模块还沙箱状态查询功能：
//!
//! - **`status`**: 获取沙箱的详细状态信息（运行状态、资源使用等）
//! - **`show_status`**: 以表格形式展示沙箱状态（CLI 模式）
//! - **`show_status_projects`**: 展示多个项目的沙箱状态（服务器模式）
//!
//! ## 实现原理
//!
//! 本模块的工作流程如下：
//!
//! 1. **加载配置**: 从 `microsandbox.yaml` 等配置文件中读取沙箱定义
//! 2. **数据库查询**: 使用 SQLite 数据库查询当前运行的沙箱状态
//! 3. **差异比较**: 比较配置与运行状态，确定需要启动/停止的沙箱
//! 4. **执行操作**: 调用 `sandbox` 模块执行具体的启动/停止操作
//! 5. **状态反馈**: 通过 CLI 提供实时的状态反馈和进度显示
//!
//! ## 关键数据结构
//!
//! - `SandboxStatus`: 描述沙箱资源使用状态的结构体，包含 CPU、内存、磁盘使用量
//!
//! ## 相关模块
//!
//! - [`config`]: 配置文件加载和解析
//! - [`sandbox`]: 单个沙箱的启动/停止操作
//! - [`db`]: 沙箱状态数据库操作
//! - [`menv`]: 微沙箱环境目录管理

//! This module provides functionality for managing collections of sandboxes in a coordinated way,
//! similar to how container orchestration tools manage multiple containers. It handles the lifecycle
//! of multiple sandboxes defined in configuration, including starting them up, shutting them down,
//! and applying configuration changes.
//!
//! The main operations provided by this module are:
//! - `up`: Start up all sandboxes defined in configuration
//! - `down`: Gracefully shut down all running sandboxes
//! - `apply`: Reconcile running sandboxes with configuration

// ============================================================================
// 导入（Imports）
// ============================================================================

use crate::{
    config::{Microsandbox, START_SCRIPT_NAME},
    MicrosandboxError, MicrosandboxResult,
};

#[cfg(feature = "cli")]
use console::style;
#[cfg(feature = "cli")]
use microsandbox_utils::term;
use microsandbox_utils::{MICROSANDBOX_ENV_DIR, SANDBOX_DB_FILENAME};
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use once_cell::sync::Lazy;
#[cfg(feature = "cli")]
use std::io::{self, IsTerminal};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::RwLock,
    time::{Duration, Instant},
};

use super::{config, db, menv, sandbox};

//--------------------------------------------------------------------------------------------------
// 常量（Constants）
//--------------------------------------------------------------------------------------------------

/// 磁盘大小缓存的生存时间（TTL - Time To Live）
///
/// 用于缓存目录大小计算结果，避免频繁的文件系统访问。
/// 在状态刷新间隔（约 2 秒）内重复查询相同目录时，可以直接使用缓存结果，
/// 减少对磁盘的 I/O 压力。
const DISK_SIZE_TTL: Duration = Duration::from_secs(30);

/// CLI 消息：正在应用沙箱配置
#[cfg(feature = "cli")]
const APPLY_CONFIG_MSG: &str = "Applying sandbox configuration";

/// CLI 消息：正在启动沙箱
#[cfg(feature = "cli")]
const START_SANDBOXES_MSG: &str = "Starting sandboxes";

/// CLI 消息：正在停止沙箱
#[cfg(feature = "cli")]
const STOP_SANDBOXES_MSG: &str = "Stopping sandboxes";

/// 全局磁盘大小缓存：路径 -> (大小字节数，上次更新时间)
///
/// 使用 `RwLock`（读写锁）保护，允许多个读者同时访问，但写入时独占。
/// 缓存键为目录路径字符串，值为元组（大小，时间戳）。
///
/// # 为什么使用缓存？
///
/// 计算目录大小需要递归遍历所有文件，是昂贵的磁盘 I/O 操作。
/// 在 CLI 实时显示模式（每 2 秒刷新）下，缓存可以显著减少磁盘访问频率。
static DISK_SIZE_CACHE: Lazy<RwLock<HashMap<String, (u64, Instant)>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

//--------------------------------------------------------------------------------------------------
// 类型（Types）
//--------------------------------------------------------------------------------------------------

/// 沙箱资源使用状态信息
///
/// 该结构体用于描述一个沙箱的当前运行状态和资源使用情况。
/// 通常在调用 `status` 函数后获得此类信息。
///
/// # 字段说明
///
/// - `name`: 沙箱名称，对应配置文件中的键
/// - `running`: 运行状态，`true` 表示沙箱正在运行
/// - `supervisor_pid`: 监督器进程 ID，管理沙箱生命周期的父进程
/// - `microvm_pid`: 微虚拟机进程 ID，实际运行沙箱隔离环境的进程
/// - `cpu_usage`: CPU 使用率百分比（0-100）
/// - `memory_usage`: 内存使用量（单位：MiB）
/// - `disk_usage`: 磁盘使用量（单位：字节），主要是可写层（RW layer）的大小
/// - `rootfs_paths`: 根文件系统路径，可能是 overlayfs 或 native 格式
///
/// # 示例
///
/// ```no_run
/// use microsandbox_core::management::orchestra::{SandboxStatus, status};
///
/// # async fn example() -> anyhow::Result<()> {
/// // 获取沙箱状态
/// let statuses = status(vec!["my-sandbox".to_string()], None, None).await?;
///
/// for status in statuses {
///     println!("沙箱：{}", status.name);
///     println!("运行状态：{}", if status.running { "运行中" } else { "已停止" });
///     if status.running {
///         println!("CPU: {:?}%", status.cpu_usage);
///         println!("内存：{:?} MiB", status.memory_usage);
///         println!("磁盘：{:?} B", status.disk_usage);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct SandboxStatus {
    /// 沙箱名称（The name of the sandbox）
    pub name: String,

    /// 沙箱是否正在运行（Whether the sandbox is running）
    pub running: bool,

    /// 监督器进程的 PID（The PID of the supervisor process）
    ///
    /// 监督器进程是管理沙箱生命周期的父进程，负责启动和监控 microVM。
    pub supervisor_pid: Option<u32>,

    /// 微虚拟机进程的 PID（The PID of the microVM process）
    ///
    /// microVM 是实际运行沙箱隔离环境的轻量级虚拟机进程。
    pub microvm_pid: Option<u32>,

    /// CPU 使用率百分比（CPU usage percentage）
    ///
    /// 取值范围通常为 0-100，超过 100 表示使用了多个 CPU 核心。
    pub cpu_usage: Option<f32>,

    /// 内存使用量（Memory usage in MiB）
    ///
    /// 单位为 MiB（Mebibyte），1 MiB = 1048576 字节。
    pub memory_usage: Option<u64>,

    /// 可写层（RW layer）的磁盘使用量（Disk usage of the RW layer in bytes）
    ///
    /// 对于 overlayfs 类型的根文件系统，这表示可写层的大小。
    /// 只读层（如基础镜像）的大小不计入此项。
    pub disk_usage: Option<u64>,

    /// 根文件系统路径（Rootfs paths）
    ///
    /// 可能是以下格式之一：
    /// - `overlayfs:<lowerdir>:<upperdir>`: overlay 文件系统
    /// - `native:<path>`: 原生文件系统
    pub rootfs_paths: Option<String>,
}

//--------------------------------------------------------------------------------------------------
// 函数（Functions）
//--------------------------------------------------------------------------------------------------

/// 应用配置：使运行中的沙箱与配置保持一致（reconcile）
///
/// 该函数确保运行中的沙箱集合与配置文件中定义的完全匹配，通过以下方式：
///
/// - **启动**配置文件中存在但未运行的沙箱
/// - **停止**正在运行但配置文件中不存在的沙箱
///
/// ## 工作原理
///
/// 1. 加载配置文件，获取所有沙箱定义
/// 2. 从数据库查询当前正在运行的沙箱
/// 3. 计算差集：
///    - 需要启动的沙箱 = 配置中的沙箱 - 运行中的沙箱
///    - 需要停止的沙箱 = 运行中的沙箱 - 配置中的沙箱
/// 4. 执行启动/停止操作
///
/// ## 参数
///
/// * `project_dir` - 项目目录的可选路径。如果为 `None`，默认为当前目录
/// * `config_file` - Microsandbox 配置文件的可选路径。如果为 `None`，使用默认文件名
/// * `detach` - 是否以分离模式运行沙箱
///   - `true`: 分离模式，沙箱在后台运行，不阻塞当前终端
///   - `false`: 附加模式，沙箱输出会 multiplexed 显示在当前终端
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<()>` 表示成功或失败。可能的失败原因包括：
/// - 配置文件未找到或无效
/// - 数据库错误
/// - 沙箱启动/停止失败
///
/// ## CLI 行为
///
/// 当启用 `cli` 特性时：
/// - 显示旋转加载动画（spinner）
/// - 在非分离模式下，完成 spinner 后显示带彩色前缀的多路复用输出
///
/// ## 示例
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // 应用默认的 microsandbox.yaml 配置变更（分离模式）
///     orchestra::apply(None, None, true).await?;
///
///     // 或者指定自定义项目目录和配置文件，使用附加模式（非分离）
///     orchestra::apply(
///         Some(&PathBuf::from("/path/to/project")),
///         Some("custom-config.yaml"),
///         false,
///     ).await?;
///     Ok(())
/// }
/// ```
///
/// ## Reconciles the running sandboxes with the configuration.
///
/// This function ensures that the set of running sandboxes matches what is defined in the
/// configuration by:
/// - Starting any sandboxes that are in the config but not running
/// - Stopping any sandboxes that are running but not in the config
///
/// The function uses a file-based lock to prevent concurrent apply operations.
/// If another apply operation is in progress, this function will fail immediately.
/// The lock is automatically released when the function completes or if it fails.
///
/// ## Arguments
///
/// * `project_dir` - Optional path to the project directory. If None, defaults to current directory
/// * `config_file` - Optional path to the Microsandbox config file. If None, uses default filename
/// * `detach` - Whether to run sandboxes in detached mode (true) or with prefixed output (false)
///
/// ## Returns
///
/// Returns `MicrosandboxResult<()>` indicating success or failure. Possible failures include:
/// - Config file not found or invalid
/// - Database errors
/// - Sandbox start/stop failures
///
/// ## Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // Apply configuration changes from the default microsandbox.yaml
///     orchestra::apply(None, None, true).await?;
///
///     // Or specify a custom project directory and config file, in non-detached mode
///     orchestra::apply(
///         Some(&PathBuf::from("/path/to/project")),
///         Some("custom-config.yaml"),
///         false,
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn apply(
    project_dir: Option<&Path>,
    config_file: Option<&str>,
    detach: bool,
) -> MicrosandboxResult<()> {
    // 创建 CLI 加载动画 spinner
    #[cfg(feature = "cli")]
    let apply_config_sp = term::create_spinner(APPLY_CONFIG_MSG.to_string(), None, None);

    // 首先加载配置，在获取锁之前验证配置存在
    let (config, canonical_project_dir, config_file) =
        match config::load_config(project_dir, config_file).await {
            Ok(result) => result,
            Err(e) => {
                #[cfg(feature = "cli")]
                term::finish_with_error(&apply_config_sp);
                return Err(e);
            }
        };

    // 确保 menv（微沙箱环境）文件存在
    let menv_path = canonical_project_dir.join(MICROSANDBOX_ENV_DIR);
    menv::ensure_menv_files(&menv_path).await?;

    // 获取数据库连接池
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);
    let pool = match db::get_or_create_pool(&db_path, &db::SANDBOX_DB_MIGRATOR).await {
        Ok(pool) => pool,
        Err(e) => {
            #[cfg(feature = "cli")]
            term::finish_with_error(&apply_config_sp);
            return Err(e);
        }
    };

    // 获取配置中定义的所有沙箱
    let config_sandboxes = config.get_sandboxes();

    // 从数据库获取所有正在运行的沙箱
    let running_sandboxes = match db::get_running_config_sandboxes(&pool, &config_file).await {
        Ok(sandboxes) => sandboxes,
        Err(e) => {
            #[cfg(feature = "cli")]
            term::finish_with_error(&apply_config_sp);
            return Err(e);
        }
    };
    let running_sandbox_names: Vec<String> =
        running_sandboxes.iter().map(|s| s.name.clone()).collect();

    // 收集需要启动的沙箱（在配置中但未运行）
    let sandboxes_to_start: Vec<&String> = config_sandboxes
        .keys()
        .filter(|name| !running_sandbox_names.contains(*name))
        .collect();

    if sandboxes_to_start.is_empty() {
        tracing::info!("No new sandboxes to start");
    } else if detach {
        // 以分离模式启动沙箱
        for name in sandboxes_to_start {
            tracing::info!("starting sandbox: {}", name);
            sandbox::run(
                name,
                Some(START_SCRIPT_NAME),
                Some(&canonical_project_dir),
                Some(&config_file),
                vec![],
                true, // detached mode（分离模式）
                None,
                true,
            )
            .await?
        }
    } else {
        // 以附加模式启动沙箱，使用多路复用输出
        let sandbox_commands = match prepare_sandbox_commands(
            &sandboxes_to_start,
            Some(START_SCRIPT_NAME),
            &canonical_project_dir,
            &config_file,
        )
        .await
        {
            Ok(commands) => commands,
            Err(e) => {
                #[cfg(feature = "cli")]
                term::finish_with_error(&apply_config_sp);
                return Err(e);
            }
        };

        if !sandbox_commands.is_empty() {
            // 在运行带输出的命令之前完成 spinner
            #[cfg(feature = "cli")]
            apply_config_sp.finish();

            run_commands_with_prefixed_output(sandbox_commands).await?;

            // 提前返回，因为我们已经完成 spinner
            return Ok(());
        }
    }

    // 停止活跃但不在配置中的沙箱
    for sandbox in running_sandboxes {
        if !config_sandboxes.contains_key(&sandbox.name) {
            tracing::info!("stopping sandbox: {}", sandbox.name);
            // 发送 SIGTERM 信号进行优雅关闭
            if let Err(e) = signal::kill(
                Pid::from_raw(sandbox.supervisor_pid as i32),
                Signal::SIGTERM,
            ) {
                #[cfg(feature = "cli")]
                term::finish_with_error(&apply_config_sp);
                return Err(e.into());
            }
        }
    }

    #[cfg(feature = "cli")]
    apply_config_sp.finish();

    Ok(())
}

/// 启动沙箱（up）：启动配置文件中指定的沙箱
///
/// 该函数确保指定的沙箱处于运行状态，通过以下方式：
///
/// - **启动**指定的、在配置文件中存在但未运行的沙箱
/// - **忽略**未指定或已经运行的沙箱
///
/// ## 与 `apply` 函数的区别
///
/// - `up` 只启动沙箱，不停止任何沙箱
/// - `apply` 会同时启动和停止沙箱，以保持与配置一致
///
/// ## 参数
///
/// * `sandbox_names` - 要启动的沙箱名称列表
///   - 如果为空列表 `vec![]`，则启动配置文件中定义的**所有**沙箱
///   - 如果指定名称，则只启动对应的沙箱
/// * `project_dir` - 项目目录的可选路径。如果为 `None`，默认为当前目录
/// * `config_file` - Microsandbox 配置文件的可选路径。如果为 `None`，使用默认文件名
/// * `detach` - 是否以分离模式运行沙箱
///   - `true`: 分离模式，沙箱在后台运行，不阻塞当前终端
///   - `false`: 附加模式，沙箱输出会 multiplexed 显示在当前终端
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<()>` 表示成功或失败。可能的失败原因包括：
/// - 配置文件未找到或无效
/// - 数据库错误
/// - 沙箱启动失败
/// - 指定的沙箱名称不在配置文件中
///
/// ## CLI 行为
///
/// 当启用 `cli` 特性时：
/// - 显示旋转加载动画（spinner）
/// - 在非分离模式下，完成 spinner 后显示带彩色前缀的多路复用输出
///
/// ## 示例
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // 从默认的 microsandbox.yaml 启动特定沙箱（分离模式）
///     orchestra::up(vec!["sandbox1".to_string(), "sandbox2".to_string()], None, None, true).await?;
///
///     // 或者指定自定义项目目录和配置文件，使用附加模式（非分离）
///     orchestra::up(
///         vec!["sandbox1".to_string()],
///         Some(&PathBuf::from("/path/to/project")),
///         Some("custom-config.yaml"),
///         false,
///     ).await?;
///
///     // 启动所有沙箱（空列表表示全部）
///     orchestra::up(vec![], None, None, true).await?;
///     Ok(())
/// }
/// ```
///
/// Starts specified sandboxes from the configuration if they are not already running.
///
/// This function ensures that the specified sandboxes are running by:
/// - Starting any specified sandboxes that are in the config but not running
/// - Ignoring sandboxes that are not specified or already running
///
/// ## Arguments
///
/// * `sandbox_names` - List of sandbox names to start
/// * `project_dir` - Optional path to the project directory. If None, defaults to current directory
/// * `config_file` - Optional path to the Microsandbox config file. If None, uses default filename
/// * `detach` - Whether to run sandboxes in detached mode (true) or with prefixed output (false)
///
/// ## Returns
///
/// Returns `MicrosandboxResult<()>` indicating success or failure. Possible failures include:
/// - Config file not found or invalid
/// - Database errors
/// - Sandbox start failures
///
/// ## Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // Start specific sandboxes from the default microsandbox.yaml in detached mode
///     orchestra::up(vec!["sandbox1".to_string(), "sandbox2".to_string()], None, None, true).await?;
///
///     // Or specify a custom project directory and config file, in non-detached mode
///     orchestra::up(
///         vec!["sandbox1".to_string()],
///         Some(&PathBuf::from("/path/to/project")),
///         Some("custom-config.yaml"),
///         false,
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn up(
    sandbox_names: Vec<String>,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
    detach: bool,
) -> MicrosandboxResult<()> {
    // 创建 CLI 加载动画 spinner
    #[cfg(feature = "cli")]
    let start_sandboxes_sp = term::create_spinner(START_SANDBOXES_MSG.to_string(), None, None);

    // 首先加载配置，验证配置存在
    let (config, canonical_project_dir, config_file) =
        match config::load_config(project_dir, config_file).await {
            Ok(result) => result,
            Err(e) => {
                #[cfg(feature = "cli")]
                term::finish_with_error(&start_sandboxes_sp);
                return Err(e);
            }
        };

    // 获取配置中定义的所有沙箱
    let config_sandboxes = config.get_sandboxes();

    // 如果未指定沙箱名称，则使用配置中的所有沙箱名称
    let sandbox_names_to_start = if sandbox_names.is_empty() {
        // 使用配置中的所有沙箱名称
        config_sandboxes.keys().cloned().collect()
    } else {
        // 在继续之前验证所有沙箱名称存在于配置中
        validate_sandbox_names(
            &sandbox_names,
            &config,
            &canonical_project_dir,
            &config_file,
        )?;

        sandbox_names
    };

    // 确保 menv（微沙箱环境）文件存在
    let menv_path = canonical_project_dir.join(MICROSANDBOX_ENV_DIR);
    menv::ensure_menv_files(&menv_path).await?;

    // 获取数据库连接池
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);
    let pool = match db::get_or_create_pool(&db_path, &db::SANDBOX_DB_MIGRATOR).await {
        Ok(pool) => pool,
        Err(e) => {
            #[cfg(feature = "cli")]
            term::finish_with_error(&start_sandboxes_sp);
            return Err(e);
        }
    };

    // 从数据库获取所有正在运行的沙箱
    let running_sandboxes = match db::get_running_config_sandboxes(&pool, &config_file).await {
        Ok(sandboxes) => sandboxes,
        Err(e) => {
            #[cfg(feature = "cli")]
            term::finish_with_error(&start_sandboxes_sp);
            return Err(e);
        }
    };
    let running_sandbox_names: Vec<String> =
        running_sandboxes.iter().map(|s| s.name.clone()).collect();

    // 收集需要启动的沙箱（在启动列表中且在配置中但未运行）
    let sandboxes_to_start: Vec<&String> = config_sandboxes
        .keys()
        .filter(|name| {
            sandbox_names_to_start.contains(*name) && !running_sandbox_names.contains(*name)
        })
        .collect();

    if sandboxes_to_start.is_empty() {
        tracing::info!("No new sandboxes to start");
        #[cfg(feature = "cli")]
        start_sandboxes_sp.finish();
        return Ok(());
    }

    if detach {
        // 以分离模式启动指定的沙箱
        for name in sandboxes_to_start {
            tracing::info!("starting sandbox: {}", name);
            sandbox::run(
                name,
                None,
                Some(&canonical_project_dir),
                Some(&config_file),
                vec![],
                true, // detached mode（分离模式）
                None,
                true,
            )
            .await?
        }
    } else {
        // 以附加模式启动沙箱，使用多路复用输出
        let sandbox_commands = match prepare_sandbox_commands(
            &sandboxes_to_start,
            None, // 普通 up 操作时 start script 为 None
            &canonical_project_dir,
            &config_file,
        )
        .await
        {
            Ok(commands) => commands,
            Err(e) => {
                #[cfg(feature = "cli")]
                term::finish_with_error(&start_sandboxes_sp);
                return Err(e);
            }
        };

        if !sandbox_commands.is_empty() {
            // 在运行带输出的命令之前完成 spinner
            #[cfg(feature = "cli")]
            start_sandboxes_sp.finish();

            run_commands_with_prefixed_output(sandbox_commands).await?;

            // 提前返回，因为我们已经完成 spinner
            return Ok(());
        }
    }

    #[cfg(feature = "cli")]
    start_sandboxes_sp.finish();

    Ok(())
}

/// 停止沙箱（down）：停止配置文件中指定的沙箱
///
/// 该函数确保指定的沙箱被停止，通过以下方式：
///
/// - **停止**指定的、在配置文件中存在且正在运行的沙箱
/// - **忽略**未指定、不在配置文件中或未运行的沙箱
///
/// ## 与 `apply` 函数的区别
///
/// - `down` 只停止沙箱，不启动任何沙箱
/// - `apply` 会同时启动和停止沙箱，以保持与配置一致
///
/// ## 工作原理
///
/// 1. 加载配置文件，获取所有沙箱定义
/// 2. 从数据库查询当前正在运行的沙箱
/// 3. 找出交集：指定的 + 在配置中的 + 正在运行的沙箱
/// 4. 向这些沙箱的监督器进程发送 SIGTERM 信号
///
/// ## 参数
///
/// * `sandbox_names` - 要停止的沙箱名称列表
///   - 如果为空列表 `vec![]`，则停止配置文件中定义的**所有**正在运行的沙箱
///   - 如果指定名称，则只停止对应的沙箱
/// * `project_dir` - 项目目录的可选路径。如果为 `None`，默认为当前目录
/// * `config_file` - Microsandbox 配置文件的可选路径。如果为 `None`，使用默认文件名
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<()>` 表示成功或失败。可能的失败原因包括：
/// - 配置文件未找到或无效
/// - 数据库错误
/// - 发送信号失败
/// - 指定的沙箱名称不在配置文件中
///
/// ## 优雅关闭
///
/// 本函数使用 `SIGTERM` 信号进行优雅关闭：
/// - 监督器进程接收到 SIGTERM 后会清理并关闭 microVM
/// - 沙箱内的应用有机会进行清理工作（如保存数据、关闭连接）
///
/// ## 示例
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // 从默认的 microsandbox.yaml 停止特定沙箱
///     orchestra::down(vec!["sandbox1".to_string(), "sandbox2".to_string()], None, None).await?;
///
///     // 或者指定自定义项目目录和配置文件
///     orchestra::down(
///         vec!["sandbox1".to_string()],
///         Some(&PathBuf::from("/path/to/project")),
///         Some("custom-config.yaml"),
///     ).await?;
///
///     // 停止所有沙箱（空列表表示全部）
///     orchestra::down(vec![], None, None).await?;
///     Ok(())
/// }
/// ```
///
/// Stops specified sandboxes that are both in the configuration and currently running.
///
/// This function ensures that the specified sandboxes are stopped by:
/// - Stopping any specified sandboxes that are both in the config and currently running
/// - Ignoring sandboxes that are not specified, not in config, or not running
///
/// ## Arguments
///
/// * `sandbox_names` - List of sandbox names to stop
/// * `project_dir` - Optional path to the project directory. If None, defaults to current directory
/// * `config_file` - Optional path to the Microsandbox config file. If None, uses default filename
///
/// ## Returns
///
/// Returns `MicrosandboxResult<()>` indicating success or failure. Possible failures include:
/// - Config file not found or invalid
/// - Database errors
/// - Sandbox stop failures
///
/// ## Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // Stop specific sandboxes from the default microsandbox.yaml
///     orchestra::down(vec!["sandbox1".to_string(), "sandbox2".to_string()], None, None).await?;
///
///     // Or specify a custom project directory and config file
///     orchestra::down(
///         vec!["sandbox1".to_string()],
///         Some(&PathBuf::from("/path/to/project")),
///         Some("custom-config.yaml"),
///     ).await?;
///     Ok(())
/// }
/// ```
pub async fn down(
    sandbox_names: Vec<String>,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<()> {
    // 创建 CLI 加载动画 spinner
    #[cfg(feature = "cli")]
    let stop_sandboxes_sp = term::create_spinner(STOP_SANDBOXES_MSG.to_string(), None, None);

    // 首先加载配置，验证配置存在
    let (config, canonical_project_dir, config_file) =
        match config::load_config(project_dir, config_file).await {
            Ok(result) => result,
            Err(e) => {
                #[cfg(feature = "cli")]
                term::finish_with_error(&stop_sandboxes_sp);
                return Err(e);
            }
        };

    // 获取配置中定义的所有沙箱
    let config_sandboxes = config.get_sandboxes();

    // 如果未指定沙箱名称，则使用配置中的所有沙箱名称
    let sandbox_names_to_stop = if sandbox_names.is_empty() {
        // 使用配置中的所有沙箱名称
        config_sandboxes.keys().cloned().collect()
    } else {
        // 在继续之前验证所有沙箱名称存在于配置中
        validate_sandbox_names(
            &sandbox_names,
            &config,
            &canonical_project_dir,
            &config_file,
        )?;

        sandbox_names
    };

    // 确保 menv（微沙箱环境）文件存在
    let menv_path = canonical_project_dir.join(MICROSANDBOX_ENV_DIR);
    menv::ensure_menv_files(&menv_path).await?;

    // 获取数据库连接池
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);
    let pool = match db::get_or_create_pool(&db_path, &db::SANDBOX_DB_MIGRATOR).await {
        Ok(pool) => pool,
        Err(e) => {
            #[cfg(feature = "cli")]
            term::finish_with_error(&stop_sandboxes_sp);
            return Err(e);
        }
    };

    // 从数据库获取所有正在运行的沙箱
    let running_sandboxes = match db::get_running_config_sandboxes(&pool, &config_file).await {
        Ok(sandboxes) => sandboxes,
        Err(e) => {
            #[cfg(feature = "cli")]
            term::finish_with_error(&stop_sandboxes_sp);
            return Err(e);
        }
    };

    // 停止指定的、在配置中且正在运行的沙箱
    for sandbox in running_sandboxes {
        if sandbox_names_to_stop.contains(&sandbox.name)
            && config_sandboxes.contains_key(&sandbox.name)
        {
            tracing::info!("stopping sandbox: {}", sandbox.name);
            // 发送 SIGTERM 信号进行优雅关闭
            if let Err(e) = signal::kill(
                Pid::from_raw(sandbox.supervisor_pid as i32),
                Signal::SIGTERM,
            ) {
                #[cfg(feature = "cli")]
                term::finish_with_error(&stop_sandboxes_sp);
                return Err(e.into());
            }
        }
    }

    #[cfg(feature = "cli")]
    stop_sandboxes_sp.finish();

    Ok(())
}

/// 获取沙箱状态信息（status）
///
/// 该函数检索指定沙箱的当前状态和资源使用情况：
///
/// - 仅报告配置文件中存在的沙箱
/// - 对每个沙箱，报告其运行状态和资源使用情况（如果正在运行）
/// - 如果未指定沙箱名称（空列表），返回配置文件中所有沙箱的状态
///
/// ## 参数
///
/// * `sandbox_names` - 要获取状态的沙箱名称列表
///   - 如果为空 `vec![]`，返回配置中的**所有**沙箱状态
///   - 如果指定名称，只返回对应沙箱的状态
/// * `project_dir` - 项目目录的可选路径。如果为 `None`，默认为当前目录
/// * `config_file` - Microsandbox 配置文件的可选路径。如果为 `None`，使用默认文件名
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<Vec<SandboxStatus>>`，包含每个沙箱的状态信息。
/// 可能的失败原因包括：
/// - 配置文件未找到或无效
/// - 数据库错误
/// - 指定的沙箱名称不在配置文件中
///
/// ## 资源使用情况
///
/// 对于正在运行的沙箱，本函数会获取：
///
/// - **CPU 使用率**: 通过 `psutil` crate 读取 microVM 进程的 CPU 使用百分比
/// - **内存使用量**: 读取 microVM 进程的 RSS（Resident Set Size），转换为 MiB
/// - **磁盘使用量**: 对于 overlayfs 或 native 根文件系统，计算可写层的目录大小
///
/// ## 缓存优化
///
/// 磁盘大小计算使用缓存（`DISK_SIZE_CACHE`），TTL 为 30 秒，避免频繁的文件系统访问。
///
/// ## 示例
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // 从默认的 microsandbox.yaml 获取特定沙箱的状态
///     let statuses = orchestra::status(
///         vec!["sandbox1".to_string(), "sandbox2".to_string()],
///         None,
///         None
///     ).await?;
///
///     // 或者获取默认的 microsandbox.yaml 所有沙箱的状态
///     let all_statuses = orchestra::status(
///         vec![], // 空列表表示获取所有沙箱
///         None,
///         None
///     ).await?;
///
///     for status in statuses {
///         println!("沙箱：{}, 运行状态：{}", status.name, status.running);
///         if status.running {
///             println!("  CPU: {:?}%, 内存：{:?}MiB, 磁盘：{:?}B",
///                 status.cpu_usage, status.memory_usage, status.disk_usage);
///         }
///     }
///
///     Ok(())
/// }
/// ```
///
/// Gets status information about specified sandboxes.
///
/// This function retrieves the current status and resource usage of the specified sandboxes:
/// - Only reports on sandboxes that exist in the configuration
/// - For each sandbox, reports whether it's running and resource usage if it is
/// - If no sandbox names are specified (empty list), returns status for all sandboxes in the configuration
///
/// ## Arguments
///
/// * `sandbox_names` - List of sandbox names to get status for. If empty, all sandboxes in config are included.
/// * `project_dir` - Optional path to the project directory. If None, defaults to current directory
/// * `config_file` - Optional path to the Microsandbox config file. If None, uses default filename
///
/// ## Returns
///
/// Returns `MicrosandboxResult<Vec<SandboxStatus>>` containing status information for each sandbox.
/// Possible failures include:
/// - Config file not found or invalid
/// - Database errors
///
/// ## Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // Get status of specific sandboxes from the default microsandbox.yaml
///     let statuses = orchestra::status(
///         vec!["sandbox1".to_string(), "sandbox2".to_string()],
///         None,
///         None
///     ).await?;
///
///     // Or get status of all sandboxes from the default microsandbox.yaml
///     let all_statuses = orchestra::status(
///         vec![], // empty list means get all sandboxes
///         None,
///         None
///     ).await?;
///
///     for status in statuses {
///         println!("Sandbox: {}, Running: {}", status.name, status.running);
///         if status.running {
///             println!("  CPU: {:?}%, Memory: {:?}MiB, Disk: {:?}B",
///                 status.cpu_usage, status.memory_usage, status.disk_usage);
///         }
///     }
///
///     Ok(())
/// }
/// ```
pub async fn status(
    sandbox_names: Vec<String>,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<Vec<SandboxStatus>> {
    // 首先加载配置，验证配置存在
    let (config, canonical_project_dir, config_file) =
        config::load_config(project_dir, config_file).await?;

    // 获取配置中定义的所有沙箱
    let config_sandboxes = config.get_sandboxes();

    // 如果未指定沙箱名称，则使用配置中的所有沙箱名称
    let sandbox_names_to_check = if sandbox_names.is_empty() {
        // 使用配置中的所有沙箱名称
        config_sandboxes.keys().cloned().collect()
    } else {
        // 在继续之前验证所有沙箱名称存在于配置中
        validate_sandbox_names(
            &sandbox_names,
            &config,
            &canonical_project_dir,
            &config_file,
        )?;

        sandbox_names
    };

    // 确保 menv（微沙箱环境）文件存在
    let menv_path = canonical_project_dir.join(MICROSANDBOX_ENV_DIR);
    menv::ensure_menv_files(&menv_path).await?;

    // 获取数据库连接池
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);
    let pool = db::get_or_create_pool(&db_path, &db::SANDBOX_DB_MIGRATOR).await?;

    // 从数据库获取所有正在运行的沙箱
    let running_sandboxes = db::get_running_config_sandboxes(&pool, &config_file).await?;

    // 创建 HashMap 以便快速查找运行中的沙箱
    let running_sandbox_map: std::collections::HashMap<String, crate::models::Sandbox> =
        running_sandboxes
            .into_iter()
            .map(|s| (s.name.clone(), s))
            .collect();

    // 获取每个要检查的沙箱名称的状态
    let mut statuses = Vec::new();
    for sandbox_name in &sandbox_names_to_check {
        // 仅处理配置中存在的沙箱
        if config_sandboxes.contains_key(sandbox_name) {
            // 创建包含名称和运行状态的基本状态
            let mut sandbox_status = SandboxStatus {
                name: sandbox_name.clone(),
                running: running_sandbox_map.contains_key(sandbox_name),
                supervisor_pid: None,
                microvm_pid: None,
                cpu_usage: None,
                memory_usage: None,
                disk_usage: None,
                rootfs_paths: None,
            };

            // 如果沙箱正在运行，获取额外的统计信息
            if sandbox_status.running
                && let Some(sandbox) = running_sandbox_map.get(sandbox_name)
            {
                sandbox_status.supervisor_pid = Some(sandbox.supervisor_pid);
                sandbox_status.microvm_pid = Some(sandbox.microvm_pid);
                sandbox_status.rootfs_paths = Some(sandbox.rootfs_paths.clone());

                // 获取 microVM 进程的 CPU 和内存使用情况
                if let Ok(mut process) = psutil::process::Process::new(sandbox.microvm_pid) {
                    // CPU 使用率
                    if let Ok(cpu_percent) = process.cpu_percent() {
                        sandbox_status.cpu_usage = Some(cpu_percent);
                    }

                    // 内存使用情况
                    if let Ok(memory_info) = process.memory_info() {
                        // 将字节转换为 MiB
                        sandbox_status.memory_usage = Some(memory_info.rss() / (1024 * 1024));
                    }
                }

                // 如果是 overlayfs，获取 RW 层的磁盘使用量
                if sandbox.rootfs_paths.starts_with("overlayfs:") {
                    let paths: Vec<&str> = sandbox.rootfs_paths.split(':').collect();
                    if paths.len() > 1 {
                        // 最后一个路径应该是 RW 层
                        let rw_path = paths.last().unwrap();
                        if let Ok(metadata) = tokio::fs::metadata(rw_path).await {
                            // 对于目录，需要计算总大小
                            if metadata.is_dir() {
                                if let Ok(size) = get_directory_size(rw_path).await {
                                    sandbox_status.disk_usage = Some(size);
                                }
                            } else {
                                sandbox_status.disk_usage = Some(metadata.len());
                            }
                        }
                    }
                } else if sandbox.rootfs_paths.starts_with("native:") {
                    // 对于 native 根文件系统，获取 rootfs 的大小
                    let path = sandbox.rootfs_paths.strip_prefix("native:").unwrap();
                    if let Ok(metadata) = tokio::fs::metadata(path).await {
                        if metadata.is_dir() {
                            if let Ok(size) = get_directory_size(path).await {
                                sandbox_status.disk_usage = Some(size);
                            }
                        } else {
                            sandbox_status.disk_usage = Some(metadata.len());
                        }
                    }
                }
            }

            statuses.push(sandbox_status);
        }
    }

    Ok(statuses)
}

/// 显示沙箱状态（CLI 模式）
///
/// 该函数以表格形式显示沙箱的当前状态，支持实时刷新模式。
///
/// ## 参数
///
/// * `names` - 要显示状态的沙箱名称列表
/// * `path` - Microsandbox 配置文件的路径（可选）
/// * `config` - 要使用的配置文件名（可选）
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<()>` 表示成功或失败。可能的失败原因包括：
/// - 配置文件未找到或无效
/// - 数据库错误
/// - 沙箱状态获取失败
///
/// ## 实时视图模式
///
/// 当在 TTY（终端）环境中运行时，本函数会进入实时视图模式：
/// - 每 2 秒自动刷新一次状态
/// - 清屏并重新显示最新状态
/// - 按 Ctrl+C 退出实时视图
///
/// 在非 TTY 环境（如管道、重定向）中，只显示一次状态。
///
/// ## 示例
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     orchestra::show_status(
///         &["sandbox1".to_string(), "sandbox2".to_string()],
///         None,
///         None
///     ).await?;
///     Ok(())
/// }
/// ```
///
/// Show the status of the sandboxes
///
/// ## Arguments
///
/// * `names` - The names of the sandboxes to show the status of
/// * `path` - The path to the microsandbox config file
/// * `config` - The config file to use
///
/// ## Returns
///
/// Returns `MicrosandboxResult<()>` indicating success or failure. Possible failures include:
/// - Config file not found or invalid
/// - Database errors
/// - Sandbox status retrieval failures
///
/// ## Example
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     orchestra::show_status(
///         &["sandbox1".to_string(), "sandbox2".to_string()],
///         None,
///         None
///     ).await?;
///     Ok(())
/// }
/// ```
#[cfg(feature = "cli")]
pub async fn show_status(
    names: &[String],
    path: Option<&Path>,
    config: Option<&str>,
) -> MicrosandboxResult<()> {
    // 检查是否在 TTY 环境中，以决定是否进行实时更新
    let is_tty = io::stdin().is_terminal();
    let live_view = is_tty;
    let update_interval = std::time::Duration::from_secs(2);

    if live_view {
        println!("{}", style("Press Ctrl+C to exit live view").dim());
        // 使用循环和 tokio sleep 进行实时更新
        loop {
            // 通过打印 ANSI 转义码清屏
            print!("\x1B[2J\x1B[1;1H");

            display_status(names, path, config).await?;

            // 显示更新消息
            println!(
                "\n{}",
                style("Updating every 2 seconds. Press Ctrl+C to exit.").dim()
            );

            // 等待更新间隔
            tokio::time::sleep(update_interval).await;
        }
    } else {
        // 非 TTY 环境只显示一次
        display_status(names, path, config).await?;
    }

    Ok(())
}

/// 显示多个项目的沙箱状态（CLI 模式）
///
/// 该函数以 Consolidated 视图显示多个项目的沙箱状态。
/// 在服务器模式下非常有用，可以查看所有项目的所有沙箱。
///
/// ## 参数
///
/// * `names` - 要显示状态的沙箱名称列表。如果为空，显示所有沙箱。
/// * `projects_parent_dir` - 包含各项目目录的父目录
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<()>` 表示成功或失败。可能的失败原因包括：
/// - 配置文件未找到或无效
/// - 数据库错误
/// - 沙箱状态获取失败
///
/// ## 实时视图模式
///
/// 与 `show_status` 类似，在 TTY 环境中会进入实时视图模式，每 2 秒刷新一次。
///
/// ## 项目排序
///
/// 项目按照活跃度排序：
/// 1. 运行中的沙箱数量（降序）
/// 2. 总 CPU 使用量（降序）
/// 3. 总内存使用量（降序）
/// 4. 项目名称（字母顺序，作为稳定 tiebreaker）
///
/// ## 示例
///
/// ```no_run
/// use std::path::Path;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // 包含所有项目子目录的父目录
///     let projects_parent = Path::new("/path/to/projects");
///
///     // 显示所有项目中所有沙箱的状态
///     orchestra::show_status_projects(&[], projects_parent).await?;
///
///     // 或者显示特定沙箱的状态
///     orchestra::show_status_projects(
///         &["sandbox1".to_string(), "sandbox2".to_string()],
///         projects_parent
///     ).await?;
///
///     Ok(())
/// }
/// ```
///
/// Show status of sandboxes across multiple projects
///
/// This function displays the status of sandboxes from multiple projects in a consolidated view.
/// It's useful for server mode when you want to see all sandboxes across all projects.
///
/// ## Arguments
///
/// * `names` - List of sandbox names to show status for. If empty, shows all sandboxes.
/// * `projects_parent_dir` - The parent directory containing project directories
///
/// ## Returns
///
/// Returns `MicrosandboxResult<()>` indicating success or failure. Possible failures include:
/// - Config file not found or invalid
/// - Database errors
/// - Sandbox status retrieval failures
///
/// ## Example
///
/// ```no_run
/// use std::path::Path;
/// use microsandbox_core::management::orchestra;
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     // Parent directory containing all project subdirectories
///     let projects_parent = Path::new("/path/to/projects");
///
///     // Show status for all sandboxes in all projects
///     orchestra::show_status_projects(&[], projects_parent).await?;
///
///     // Or show status for specific sandboxes
///     orchestra::show_status_projects(
///         &["sandbox1".to_string(), "sandbox2".to_string()],
///         projects_parent
///     ).await?;
///
///     Ok(())
/// }
/// ```
#[cfg(feature = "cli")]
pub async fn show_status_projects(
    names: &[String],
    projects_parent_dir: &Path,
) -> MicrosandboxResult<()> {
    // 检查是否在 TTY 环境中，以决定是否进行实时更新
    let is_tty = io::stdin().is_terminal();
    let live_view = is_tty;
    let update_interval = std::time::Duration::from_secs(2);

    if live_view {
        println!("{}", style("Press Ctrl+C to exit live view").dim());
        // 使用循环和 tokio sleep 进行实时更新
        loop {
            // 通过打印 ANSI 转义码清屏
            print!("\x1B[2J\x1B[1;1H");

            display_status_projects(names, projects_parent_dir).await?;

            // 显示更新消息
            println!(
                "\n{}",
                style("Updating every 2 seconds. Press Ctrl+C to exit.").dim()
            );

            // Wait for the update interval
            tokio::time::sleep(update_interval).await;
        }
    } else {
        // Just display once for non-TTY
        display_status_projects(names, projects_parent_dir).await?;
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 辅助函数（Helper Functions）
//--------------------------------------------------------------------------------------------------

/// 为多个沙箱准备命令
///
/// 该辅助函数为每个指定的沙箱调用 `sandbox::prepare_run`，准备运行命令。
///
/// ## 参数
///
/// * `sandbox_names` - 沙箱名称引用列表
/// * `script_name` - 启动脚本名称（可选）
/// * `project_dir` - 项目目录
/// * `config_file` - 配置文件名
///
/// ## 返回值
///
/// 返回 `(沙箱名称，Command)` 元组的向量，每个元组包含沙箱名称和对应的 `tokio::process::Command`。
///
/// ## 注意
///
/// 此函数不打印任何沙箱准备日志，日志由调用者负责。
async fn prepare_sandbox_commands(
    sandbox_names: &[&String],
    script_name: Option<&str>,
    project_dir: &Path,
    config_file: &str,
) -> MicrosandboxResult<Vec<(String, tokio::process::Command)>> {
    let mut commands = Vec::new();

    for &name in sandbox_names {
        // 不打印任何单个沙箱的准备日志

        let (command, _) = sandbox::prepare_run(
            name,
            script_name,
            Some(project_dir),
            Some(config_file),
            vec![],
            false, // 非分离模式
            None,
            true,
        )
        .await?;

        commands.push((name.clone(), command));
    }

    Ok(commands)
}

/// 运行多个命令并带前缀输出
///
/// 该辅助函数并发运行多个命令，并将每个命令的输出用彩色沙箱名称前缀标识。
///
/// ## 工作原理
///
/// 1. 为每个命令启动一个子进程，捕获 stdout 和 stderr
/// 2. 为每个沙箱分配一个颜色（7 色循环：绿、蓝、红、黄、品红、青、白）
/// 3. 为每个进程创建两个任务：一个处理 stdout，一个处理 stderr
/// 4. 创建监控任务等待所有子进程完成
/// 5. 如果有沙箱失败，返回带彩色前缀的错误信息
///
/// ## 输出格式
///
/// 每行输出格式为：`[沙箱名称] | 输出内容`
/// 沙箱名称和分隔符都有相同的颜色，便于区分不同沙箱的输出。
///
/// ## 参数
///
/// * `commands` - `(沙箱名称，Command)` 元组的向量
///
/// ## 返回值
///
/// 返回 `MicrosandboxResult<()>`。如果任何沙箱失败，返回 `SupervisorError`，
/// 包含所有失败的沙箱名称和退出码。
///
/// ## 示例输出
///
/// ```text
/// sandbox1 | Starting application...
/// sandbox2 | Initializing...
/// sandbox1 | Ready to serve requests
/// sandbox2 | Connected to database
/// ```
async fn run_commands_with_prefixed_output(
    commands: Vec<(String, tokio::process::Command)>,
) -> MicrosandboxResult<()> {
    use console::style;
    use futures::future::join_all;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};

    // 如果没有要运行的命令，提前返回
    if commands.is_empty() {
        return Ok(());
    }

    // 这将保存我们的子进程句柄和相关的任务
    let mut children = Vec::new();
    let mut output_tasks = Vec::new();

    // 生成所有子进程
    for (i, (sandbox_name, mut command)) in commands.into_iter().enumerate() {
        // 配置命令以管道 stdout 和 stderr
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        // 生成子进程
        let mut child = command.spawn()?;
        let sandbox_name_clone = sandbox_name.clone();

        // 根据索引为沙箱名称着色
        let styled_name = match i % 7 {
            0 => style(&sandbox_name).green().bold(),
            1 => style(&sandbox_name).blue().bold(),
            2 => style(&sandbox_name).red().bold(),
            3 => style(&sandbox_name).yellow().bold(),
            4 => style(&sandbox_name).magenta().bold(),
            5 => style(&sandbox_name).cyan().bold(),
            _ => style(&sandbox_name).white().bold(),
        };

        // 为分隔符应用相同的颜色
        let styled_separator = match i % 7 {
            0 => style("|").green(),
            1 => style("|").blue(),
            2 => style("|").red(),
            3 => style("|").yellow(),
            4 => style("|").magenta(),
            5 => style("|").cyan(),
            _ => style("|").white(),
        };

        tracing::info!(
            "{} {} started supervisor process with PID: {}",
            styled_name,
            styled_separator,
            child.id().unwrap_or(0)
        );

        // 创建处理 stdout 的任务
        let stdout = child.stdout.take().expect("Failed to capture stdout");
        let name_stdout = sandbox_name.clone();
        let color_index = i;
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                // 为沙箱名称和分隔符着色，但保持消息为纯文本
                let styled_name = match color_index % 7 {
                    0 => style(&name_stdout).green().bold(),
                    1 => style(&name_stdout).blue().bold(),
                    2 => style(&name_stdout).red().bold(),
                    3 => style(&name_stdout).yellow().bold(),
                    4 => style(&name_stdout).magenta().bold(),
                    5 => style(&name_stdout).cyan().bold(),
                    _ => style(&name_stdout).white().bold(),
                };

                // 为分隔符应用相同的颜色
                let styled_separator = match color_index % 7 {
                    0 => style("|").green(),
                    1 => style("|").blue(),
                    2 => style("|").red(),
                    3 => style("|").yellow(),
                    4 => style("|").magenta(),
                    5 => style("|").cyan(),
                    _ => style("|").white(),
                };

                println!("{} {} {}", styled_name, styled_separator, line);
            }
        });

        // 创建处理 stderr 的任务
        let stderr = child.stderr.take().expect("Failed to capture stderr");
        let color_index = i;
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                // 为沙箱名称和分隔符着色，但保持消息为纯文本
                let styled_name = match color_index % 7 {
                    0 => style(&sandbox_name_clone).green().bold(),
                    1 => style(&sandbox_name_clone).blue().bold(),
                    2 => style(&sandbox_name_clone).red().bold(),
                    3 => style(&sandbox_name_clone).yellow().bold(),
                    4 => style(&sandbox_name_clone).magenta().bold(),
                    5 => style(&sandbox_name_clone).cyan().bold(),
                    _ => style(&sandbox_name_clone).white().bold(),
                };

                // 为分隔符应用相同的颜色
                let styled_separator = match color_index % 7 {
                    0 => style("|").green(),
                    1 => style("|").blue(),
                    2 => style("|").red(),
                    3 => style("|").yellow(),
                    4 => style("|").magenta(),
                    5 => style("|").cyan(),
                    _ => style("|").white(),
                };

                eprintln!("{} {} {}", styled_name, styled_separator, line);
            }
        });

        // 添加到我们的集合中
        children.push((sandbox_name, child));
        output_tasks.push(stdout_task);
        output_tasks.push(stderr_task);
    }

    // 创建监控子进程的任务
    let monitor_task = tokio::spawn(async move {
        let mut statuses = Vec::new();

        for (name, mut child) in children {
            match child.wait().await {
                Ok(status) => {
                    let exit_code = status.code().unwrap_or(-1);
                    let success = status.success();
                    statuses.push((name, exit_code, success));
                }
                Err(_e) => {
                    #[cfg(feature = "cli")]
                    eprintln!("Error waiting for sandbox {}: {}", name, _e);
                    statuses.push((name, -1, false));
                }
            }
        }

        statuses
    });

    // 等待所有子进程完成和输出任务完成
    let statuses = monitor_task.await?;
    join_all(output_tasks).await;

    // 检查结果，如果任何沙箱失败则返回错误
    let failed_sandboxes: Vec<(String, i32)> = statuses
        .into_iter()
        .filter(|(_, _, success)| !success)
        .map(|(name, code, _)| (name, code))
        .collect();

    if !failed_sandboxes.is_empty() {
        // 格式化带彩色沙箱名称的失败消息
        let error_msg = failed_sandboxes
            .iter()
            .enumerate()
            .map(|(i, (name, code))| {
                // 根据索引直接应用颜色
                let styled_name = match i % 7 {
                    0 => style(name).green().bold(),
                    1 => style(name).blue().bold(),
                    2 => style(name).red().bold(),
                    3 => style(name).yellow().bold(),
                    4 => style(name).magenta().bold(),
                    5 => style(name).cyan().bold(),
                    _ => style(name).white().bold(),
                };

                // 为分隔符应用相同的颜色
                let styled_separator = match i % 7 {
                    0 => style("|").green(),
                    1 => style("|").blue(),
                    2 => style("|").red(),
                    3 => style("|").yellow(),
                    4 => style("|").magenta(),
                    5 => style("|").cyan(),
                    _ => style("|").white(),
                };

                format!("{} {} exit code: {}", styled_name, styled_separator, code)
            })
            .collect::<Vec<_>>()
            .join(", ");

        return Err(MicrosandboxError::SupervisorError(format!(
            "The following sandboxes failed: {}",
            error_msg
        )));
    }

    Ok(())
}
// 提取状态显示逻辑到单独的函数
#[cfg(feature = "cli")]
/// 显示沙箱状态（内部函数）
///
/// 该函数获取并显示沙箱的状态信息，以表格形式输出。
///
/// ## 排序规则
///
/// 状态按以下顺序排序，以确保在实时更新时条目不会移动：
/// 1. 运行状态（运行中的优先）
/// 2. CPU 使用率（降序）
/// 3. 内存使用量（降序）
/// 4. 磁盘使用量（降序）
/// 5. 名称（字母顺序，作为稳定的 tiebreaker）
///
/// ## 参数
///
/// * `names` - 沙箱名称列表
/// * `path` - 项目目录路径（可选）
/// * `config` - 配置文件名（可选）
#[cfg(feature = "cli")]
async fn display_status(
    names: &[String],
    path: Option<&Path>,
    config: Option<&str>,
) -> MicrosandboxResult<()> {
    let mut statuses = status(names.to_vec(), path, config).await?;

    // 按稳定顺序排序状态，防止条目在更新之间移动
    // 排序顺序：运行状态（运行中优先）、CPU 使用率（降序）、
    // 内存使用量（降序）、磁盘使用量（降序）、最后是名称（字母顺序）
    statuses.sort_by(|a, b| {
        // 首先比较运行状态（运行中的沙箱优先）
        let running_order = b.running.cmp(&a.running);
        if running_order != std::cmp::Ordering::Equal {
            return running_order;
        }

        // 然后比较 CPU 使用率（降序）
        let cpu_order = b
            .cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal);
        if cpu_order != std::cmp::Ordering::Equal {
            return cpu_order;
        }

        // 然后比较内存使用量（降序）
        let memory_order = b
            .memory_usage
            .partial_cmp(&a.memory_usage)
            .unwrap_or(std::cmp::Ordering::Equal);
        if memory_order != std::cmp::Ordering::Equal {
            return memory_order;
        }

        // 然后比较磁盘使用量（降序）
        let disk_order = b
            .disk_usage
            .partial_cmp(&a.disk_usage)
            .unwrap_or(std::cmp::Ordering::Equal);
        if disk_order != std::cmp::Ordering::Equal {
            return disk_order;
        }

        // 最后按名称排序（字母顺序）
        a.name.cmp(&b.name)
    });

    // 获取当前时间戳
    let now = chrono::Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S");

    // 显示时间戳
    println!("{}", style(format!("Last updated: {}", timestamp)).dim());

    // 打印表格形式的状态信息
    println!(
        "\n{:<15} {:<10} {:<15} {:<12} {:<12} {:<12}",
        style("SANDBOX").bold(),
        style("STATUS").bold(),
        style("PIDS").bold(),
        style("CPU").bold(),
        style("MEMORY").bold(),
        style("DISK").bold()
    );

    println!("{}", style("─".repeat(80)).dim());

    for status in statuses {
        let (status_text, pids, cpu, memory, disk) = format_status_columns(&status);

        println!(
            "{:<15} {:<10} {:<15} {:<12} {:<12} {:<12}",
            style(&status.name).bold(),
            status_text,
            pids,
            cpu,
            memory,
            disk
        );
    }

    Ok(())
}

/// 显示多个项目的沙箱状态（内部函数）
///
/// 该函数扫描父目录中的所有项目，并显示每个项目的沙箱状态。
///
/// ## 参数
///
/// * `names` - 沙箱名称列表
/// * `projects_parent_dir` - 包含项目目录的父目录
///
/// ## 项目排序
///
/// 项目按照活跃度排序：
/// 1. 运行中的沙箱数量（降序）
/// 2. 总 CPU 使用量（降序）
/// 3. 总内存使用量（降序）
/// 4. 项目名称（字母顺序，作为稳定 tiebreaker）
#[cfg(feature = "cli")]
async fn display_status_projects(
    names: &[String],
    projects_parent_dir: &Path,
) -> MicrosandboxResult<()> {
    // 创建结构体来保存带项目信息的状态
    #[derive(Clone)]
    struct ProjectStatus {
        project: String,
        status: SandboxStatus,
    }

    // 收集所有项目的状态
    let mut all_statuses = Vec::new();
    let mut project_count = 0;

    // 检查父目录是否存在
    if !projects_parent_dir.exists() {
        return Err(MicrosandboxError::PathNotFound(format!(
            "Projects directory not found at {}",
            projects_parent_dir.display()
        )));
    }

    // 扫描父目录中的项目
    let mut entries = tokio::fs::read_dir(projects_parent_dir).await?;
    let mut project_dirs = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            project_dirs.push(path);
        }
    }

    // 按字母顺序排序项目目录（初始排序确保确定性行为）
    project_dirs.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    // 处理每个项目目录
    for project_dir in &project_dirs {
        // 从路径提取项目名称
        let project = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        project_count += 1;

        // 获取该项目的状态
        match status(names.to_vec(), Some(project_dir), None).await {
            Ok(statuses) => {
                // 为每个状态添加项目信息
                for status in statuses {
                    all_statuses.push(ProjectStatus {
                        project: project.clone(),
                        status,
                    });
                }
            }
            Err(e) => {
                // 记录错误但继续处理其他项目
                tracing::warn!("Error getting status for project {}: {}", project, e);
            }
        }
    }

    // 按项目分组状态
    let mut statuses_by_project: std::collections::HashMap<String, Vec<SandboxStatus>> =
        std::collections::HashMap::new();

    for project_status in all_statuses {
        statuses_by_project
            .entry(project_status.project)
            .or_default()
            .push(project_status.status);
    }

    // 获取当前时间戳
    let now = chrono::Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S");

    // 显示时间戳
    println!("{}", style(format!("Last updated: {}", timestamp)).dim());

    // 准备带活动度量的项目用于排序
    #[derive(Clone)]
    struct ProjectActivity {
        name: String,
        running_count: usize,
        total_cpu: f32,
        total_memory: u64,
        statuses: Vec<SandboxStatus>,
    }

    let mut project_activities = Vec::new();

    // 计算每个项目的活动度量
    for (project, statuses) in statuses_by_project {
        if statuses.is_empty() {
            continue;
        }

        let running_count = statuses.iter().filter(|s| s.running).count();
        let total_cpu: f32 = statuses.iter().filter_map(|s| s.cpu_usage).sum();
        let total_memory: u64 = statuses.iter().filter_map(|s| s.memory_usage).sum();

        project_activities.push(ProjectActivity {
            name: project,
            running_count,
            total_cpu,
            total_memory,
            statuses,
        });
    }

    // 按活动级别排序项目（运行数量优先，然后是资源使用）
    project_activities.sort_by(|a, b| {
        // 首先按运行中沙箱数量排序（降序）
        let running_order = b.running_count.cmp(&a.running_count);
        if running_order != std::cmp::Ordering::Equal {
            return running_order;
        }

        // 然后按总 CPU 使用量排序（降序）
        let cpu_order = b
            .total_cpu
            .partial_cmp(&a.total_cpu)
            .unwrap_or(std::cmp::Ordering::Equal);
        if cpu_order != std::cmp::Ordering::Equal {
            return cpu_order;
        }

        // 然后按总内存使用量排序（降序）
        let memory_order = b.total_memory.cmp(&a.total_memory);
        if memory_order != std::cmp::Ordering::Equal {
            return memory_order;
        }

        // 最后按名称排序（字母顺序）作为稳定 tiebreaker
        a.name.cmp(&b.name)
    });

    // 捕获沙箱计数
    let mut total_sandboxes = 0;
    let mut is_first = true;

    // 显示项目及其状态，带标题
    for activity in project_activities {
        // 在项目之间添加间距
        if !is_first {
            println!();
        }
        is_first = false;

        // 打印项目标题
        print_project_header(&activity.name);

        // 按稳定顺序排序状态
        let mut statuses = activity.statuses;
        statuses.sort_by(|a, b| {
            // 首先比较运行状态（运行中的沙箱优先）
            let running_order = b.running.cmp(&a.running);
            if running_order != std::cmp::Ordering::Equal {
                return running_order;
            }

            // 然后比较 CPU 使用率（降序）
            let cpu_order = b
                .cpu_usage
                .partial_cmp(&a.cpu_usage)
                .unwrap_or(std::cmp::Ordering::Equal);
            if cpu_order != std::cmp::Ordering::Equal {
                return cpu_order;
            }

            // 然后比较内存使用量（降序）
            let memory_order = b
                .memory_usage
                .partial_cmp(&a.memory_usage)
                .unwrap_or(std::cmp::Ordering::Equal);
            if memory_order != std::cmp::Ordering::Equal {
                return memory_order;
            }

            // 然后比较磁盘使用量（降序）
            let disk_order = b
                .disk_usage
                .partial_cmp(&a.disk_usage)
                .unwrap_or(std::cmp::Ordering::Equal);
            if disk_order != std::cmp::Ordering::Equal {
                return disk_order;
            }

            // 最后按名称排序（字母顺序）
            a.name.cmp(&b.name)
        });

        total_sandboxes += statuses.len();

        // 为这个项目的沙箱打印表格标题
        println!(
            "{:<15} {:<10} {:<15} {:<12} {:<12} {:<12}",
            style("SANDBOX").bold(),
            style("STATUS").bold(),
            style("PIDS").bold(),
            style("CPU").bold(),
            style("MEMORY").bold(),
            style("DISK").bold()
        );

        println!("{}", style("─".repeat(80)).dim());

        // 显示沙箱状态
        for status in statuses {
            let (status_text, pids, cpu, memory, disk) = format_status_columns(&status);

            println!(
                "{:<15} {:<10} {:<15} {:<12} {:<12} {:<12}",
                style(&status.name).bold(),
                status_text,
                pids,
                cpu,
                memory,
                disk
            );
        }
    }

    // 显示捕获的计数摘要
    println!(
        "\n{}: {}, {}: {}",
        style("Total Projects").dim(),
        project_count,
        style("Total Sandboxes").dim(),
        total_sandboxes
    );

    Ok(())
}

/// 打印项目标题（内部函数）
///
/// 为项目显示创建一个风格化的标题。
///
/// ## 参数
///
/// * `project` - 项目名称
#[cfg(feature = "cli")]
fn print_project_header(project: &str) {
    // 创建简单的标题文本，不带填充
    let title = format!("PROJECT: {}", project);

    // 打印带白色和粗体样式的标题
    println!("\n{}", style(title).white().bold());

    // 打印分隔线
    println!("{}", style("─".repeat(80)).dim());
}

/// 格式化状态列显示（内部函数）
///
/// 将 `SandboxStatus` 格式化为用于显示的列。
///
/// ## 返回值
///
/// 返回元组：
/// - 状态文本（RUNNING/STOPPED，带颜色）
/// - PID 信息（supervisor_pid/microvm_pid）
/// - CPU 使用率（百分比或"-"）
/// - 内存使用量（MiB 或"-"）
/// - 磁盘使用量（自动选择单位或"-"）
#[cfg(feature = "cli")]
fn format_status_columns(
    status: &SandboxStatus,
) -> (
    console::StyledObject<String>,
    String,
    String,
    String,
    String,
) {
    let status_text = if status.running {
        style("RUNNING".to_string()).green()
    } else {
        style("STOPPED".to_string()).red()
    };

    let pids = if status.running {
        format!(
            "{}/{}",
            status.supervisor_pid.unwrap_or(0),
            status.microvm_pid.unwrap_or(0)
        )
    } else {
        "-".to_string()
    };

    let cpu = if let Some(cpu_usage) = status.cpu_usage {
        format!("{:.1}%", cpu_usage)
    } else {
        "-".to_string()
    };

    let memory = if let Some(memory_usage) = status.memory_usage {
        format!("{} MiB", memory_usage)
    } else {
        "-".to_string()
    };

    let disk = if let Some(disk_usage) = status.disk_usage {
        if disk_usage > 1024 * 1024 * 1024 {
            format!("{:.2} GB", disk_usage as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if disk_usage > 1024 * 1024 {
            format!("{:.2} MB", disk_usage as f64 / (1024.0 * 1024.0))
        } else if disk_usage > 1024 {
            format!("{:.2} KB", disk_usage as f64 / 1024.0)
        } else {
            format!("{} B", disk_usage)
        }
    } else {
        "-".to_string()
    };

    (status_text, pids, cpu, memory, disk)
}

/// 验证沙箱名称（内部函数）
///
/// 验证所有请求的沙箱名称是否存在于配置文件中。
///
/// ## 参数
///
/// * `sandbox_names` - 要验证的沙箱名称列表
/// * `config` - Microsandbox 配置
/// * `project_dir` - 项目目录
/// * `config_file` - 配置文件名
///
/// ## 返回值
///
/// 如果所有名称都有效，返回 `Ok(())`；否则返回 `SandboxNotFoundInConfig` 错误。
fn validate_sandbox_names(
    sandbox_names: &[String],
    config: &Microsandbox,
    project_dir: &Path,
    config_file: &str,
) -> MicrosandboxResult<()> {
    let config_sandboxes = config.get_sandboxes();

    let missing_sandboxes: Vec<String> = sandbox_names
        .iter()
        .filter(|name| !config_sandboxes.contains_key(*name))
        .cloned()
        .collect();

    if !missing_sandboxes.is_empty() {
        return Err(MicrosandboxError::SandboxNotFoundInConfig(
            missing_sandboxes.join(", "),
            project_dir.join(config_file),
        ));
    }

    Ok(())
}

/// 递归计算目录大小，带缓存
///
/// 该函数递归计算目录的总大小（以字节为单位），但会将结果缓存一小段时间，
/// 这样调用者（状态刷新间隔约 2 秒）就不会频繁访问文件系统。
///
/// ## 缓存机制
///
/// - 使用全局 `DISK_SIZE_CACHE`（RwLock 保护的 HashMap）
/// - TTL 为 30 秒（`DISK_SIZE_TTL`）
/// - 缓存命中时直接返回，避免重复计算
///
/// ## 参数
///
/// * `path` - 要计算大小的目录路径
///
/// ## 返回值
///
/// 返回目录总大小（字节数）。
///
/// ## 实现细节
///
/// 使用 `tokio::task::spawn_blocking` 在单独线程中执行阻塞式的目录遍历，
/// 避免阻塞 Tokio 运行时。
async fn get_directory_size(path: &str) -> MicrosandboxResult<u64> {
    // 首先尝试从缓存提供
    {
        let cache = DISK_SIZE_CACHE.read().unwrap();
        if let Some((size, ts)) = cache.get(path)
            && ts.elapsed() < DISK_SIZE_TTL
        {
            return Ok(*size);
        }
    }

    // 需要（重新）计算——在单独线程中执行阻塞式遍历，以免阻塞 Tokio
    let path_buf = PathBuf::from(path);
    let size = tokio::task::spawn_blocking(move || -> MicrosandboxResult<u64> {
        use walkdir::WalkDir;

        let mut total: u64 = 0;
        for entry in WalkDir::new(&path_buf).follow_links(false) {
            let entry = entry?; // 传播 walkdir::Error（已包含在 MicrosandboxError 中）
            if entry.file_type().is_file() {
                total += entry.metadata()?.len();
            }
        }
        Ok(total)
    })
    .await??; // 第一个 ? = JoinError，第二个？ = 内部 MicrosandboxError

    // 更新缓存
    {
        let mut cache = DISK_SIZE_CACHE.write().unwrap();
        cache.insert(path.to_string(), (size, Instant::now()));
    }

    Ok(size)
}

/// 检查配置中指定的沙箱是否正在运行（内部函数，未使用）
///
/// 该函数检查配置中指定的沙箱是否正在运行。
///
/// ## 参数
///
/// * `sandbox_names` - 沙箱名称列表
/// * `config` - Microsandbox 配置
/// * `project_dir` - 项目目录
/// * `config_file` - 配置文件名
///
/// ## 返回值
///
/// 返回 `(沙箱名称，是否运行)` 元组的向量。
#[allow(dead_code)]
async fn _check_running(
    sandbox_names: Vec<String>,
    config: &Microsandbox,
    project_dir: &Path,
    config_file: &str,
) -> MicrosandboxResult<Vec<(String, bool)>> {
    // 确保 menv 文件存在
    let canonical_project_dir = project_dir.canonicalize().map_err(|e| {
        MicrosandboxError::InvalidArgument(format!(
            "Failed to canonicalize project directory: {}",
            e
        ))
    })?;
    let menv_path = canonical_project_dir.join(MICROSANDBOX_ENV_DIR);
    menv::ensure_menv_files(&menv_path).await?;

    // 获取数据库连接池
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);
    let pool = db::get_or_create_pool(&db_path, &db::SANDBOX_DB_MIGRATOR).await?;

    // 获取配置中定义的所有沙箱
    let config_sandboxes = config.get_sandboxes();

    // 从数据库获取所有正在运行的沙箱
    let running_sandboxes = db::get_running_config_sandboxes(&pool, config_file).await?;
    let running_sandbox_names: Vec<String> =
        running_sandboxes.iter().map(|s| s.name.clone()).collect();

    // 检查指定沙箱的状态
    let mut statuses = Vec::new();
    for sandbox_name in sandbox_names {
        // 仅检查配置中存在的沙箱
        if config_sandboxes.contains_key(&sandbox_name) {
            let is_running = running_sandbox_names.contains(&sandbox_name);
            statuses.push((sandbox_name, is_running));
        }
    }

    Ok(statuses)
}
