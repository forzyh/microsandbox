//! Microsandbox 沙箱管理模块。
//!
//! # 概述
//!
//! 本模块提供 Microsandbox 沙箱的管理功能，包括沙箱的创建、配置和执行。
//! 沙箱是基于 Microsandbox 配置文件定义的隔离执行环境。
//!
//! # 核心概念
//!
//! ## 什么是沙箱（Sandbox）？
//!
//! 沙箱是一个隔离的执行环境，用于安全地运行应用程序。每个沙箱都有：
//! - **独立的文件系统**（rootfs）- 可以基于 OCI 镜像或本地目录
//! - **资源限制** - CPU、内存配额
//! - **网络隔离** - 支持私有网络和端口映射
//! - **卷映射** - 宿主机目录挂载到沙箱
//!
//! ## 沙箱架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     宿主机系统                               │
//! │  ┌─────────────────────────────────────────────────────┐    │
//! │  │                  Microsandbox CLI                    │    │
//! │  │                      (msb)                           │    │
//! │  └─────────────────────────┬───────────────────────────┘    │
//! │                            │                                 │
//! │  ┌─────────────────────────▼───────────────────────────┐    │
//! │  │                  沙箱管理器                          │    │
//! │  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │    │
//! │  │  │  沙箱 A     │  │  沙箱 B     │  │  沙箱 C     │  │    │
//! │  │  │  (dev)      │  │  (test)     │  │  (prod)     │  │    │
//! │  │  │  ┌───────┐  │  │  ┌───────┐  │  │  ┌───────┐  │  │    │
//! │  │  │  │ rootfs│  │  │  │ rootfs│  │  │  │ rootfs│  │  │    │
//! │  │  │  └───────┘  │  │  └───────┘  │  │  └───────┘  │  │    │
//! │  │  └─────────────┘  └─────────────┘  └─────────────┘  │    │
//! │  └─────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Rootfs 类型
//!
//! Microsandbox 支持两种 rootfs 类型：
//!
//! ### 1. 镜像型 Rootfs（Image-based）
//!
//! 基于 OCI 容器镜像构建，支持分层存储：
//!
//! ```text
//! ┌─────────────────────┐
//! │     Top RW Layer    │  ← 可写层（沙箱运行时数据）
//! ├─────────────────────┤
//! │     Patch Layer     │  ← 补丁层（脚本、配置）
//! ├─────────────────────┤
//! │    Image Layer N    │  │
//! ├─────────────────────┤  │
//! │    Image Layer 1    │  │ OCI 镜像层（只读）
//! ├─────────────────────┤  │
//! │    Image Layer 0    │  ← 基础镜像
//! └─────────────────────┘
//! ```
//!
//! ### 2. 本地型 Rootfs（Native）
//!
//! 直接使用本地目录作为 rootfs：
//!
//! ```text
//! /path/to/rootfs/
//! ├── bin/
//! ├── lib/
//! ├── etc/
//! └── ...
//! ```
//!
//! # 主要功能
//!
//! | 函数 | 功能描述 |
//! |------|----------|
//! | `run()` | 运行沙箱（阻塞直到沙箱退出） |
//! | `prepare_run()` | 准备沙箱命令（不执行） |
//! | `run_temp()` | 运行临时沙箱（OCI 镜像，无需配置） |
//!
//! # 沙箱运行流程
//!
//! ```text
//! run(sandbox_name, script)
//!     │
//!     ├── 1. 加载配置文件
//!     │
//!     ├── 2. 确保 .menv 环境存在
//!     │
//!     ├── 3. 获取沙箱配置
//!     │
//!     ├── 4. 设置 rootfs
//!     │   ├── 镜像型：拉取镜像，准备 overlayfs 层
│   │   └── 本地型：准备本地 rootfs 目录
//!     │
//!     ├── 5. 确定执行命令
//!     │   ├── 使用 exec 参数
//!     │   ├── 使用 script 参数
│   │   ├── 使用 start 脚本
│   │   └── 使用 shell
│     │
│     ├── 6. 构建 supervisor 命令
│     │
│     └── 7. 执行并等待
│         ├── 阻塞模式：等待进程结束
│         └── 分离模式：立即返回
│ ```
//!
//! # 使用示例
//!
//! ## 运行命名沙箱
//!
//! ```no_run
//! use microsandbox_core::management::sandbox;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // 运行名为 "dev" 的沙箱，执行 "start" 脚本
//! sandbox::run(
//!     "dev",           // 沙箱名称
//!     Some("start"),   // 脚本名称
//!     None,            // 使用当前目录
//!     None,            // 使用默认配置文件
//!     vec![],          // 无额外参数
//!     false,           // 阻塞模式
//!     None,            // 无自定义命令
//!     true             // 使用镜像默认值
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## 运行临时沙箱
//!
//! ```no_run
//! use microsandbox_core::management::sandbox;
//! use microsandbox_core::oci::Reference;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let image = "ubuntu:22.04".parse::<Reference>()?;
//!
//! // 从 OCI 镜像运行临时沙箱
//! sandbox::run_temp(
//!     &image,          // OCI 镜像
//!     Some("shell"),   // 运行 shell
//!     Some(2),         // 2 个 CPU
//!     Some(1024),      // 1GB 内存
//!     vec![],          // 无卷映射
//!     vec![],          // 无端口映射
//!     vec![],          // 无环境变量
//!     None,            // 默认工作目录
//!     None,            // 默认网络范围
//!     None,            // 无自定义命令
//!     vec![],          // 无额外参数
//!     true             // 使用镜像默认值
//! ).await?;
//! # Ok(())
//! # }
//! ```

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use chrono::{DateTime, Utc};
use microsandbox_utils::{
    DEFAULT_MSBRUN_EXE_PATH, DEFAULT_SHELL, EXTRACTED_LAYER_SUFFIX, LAYERS_SUBDIR, LOG_SUBDIR,
    MICROSANDBOX_CONFIG_FILENAME, MICROSANDBOX_ENV_DIR, MSBRUN_EXE_ENV_VAR, OCI_DB_FILENAME,
    PATCH_SUBDIR, RW_SUBDIR, SANDBOX_DB_FILENAME, SANDBOX_DIR, SCRIPTS_DIR, SHELL_SCRIPT_NAME, env,
};
use sqlx::{Pool, Sqlite};
use tempfile;
use tokio::{fs, process::Command};
use typed_path::Utf8UnixPathBuf;

use crate::{
    MicrosandboxError, MicrosandboxResult,
    config::{
        EnvPair, Microsandbox, PathPair, PortPair, ReferenceOrPath, START_SCRIPT_NAME, Sandbox,
    },
    management::{config, db, menv, rootfs},
    oci::{Image, Reference},
    vm::Rootfs,
};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// 临时沙箱的名称常量。
///
/// # 说明
///
/// 当使用 `run_temp()` 运行临时沙箱时，会在内部创建一个临时的
/// Microsandbox 配置，其中沙箱名称为此常量值。
const TEMPORARY_SANDBOX_NAME: &str = "tmp";

//--------------------------------------------------------------------------------------------------
// 公开函数
//--------------------------------------------------------------------------------------------------

/// 运行指定配置和脚本的沙箱。
///
/// # 功能说明
///
/// 此函数根据 Microsandbox 配置文件中定义的沙箱配置，执行沙箱环境。
/// 它支持基于本地 rootfs 和基于镜像的 rootfs 两种类型。
///
/// ## 工作流程
///
/// 1. **准备阶段**（`prepare_run()`）：
///    - 加载配置文件
///    - 设置 rootfs
///    - 构建 supervisor 命令
///
/// 2. **执行阶段**：
///    - 生成 supervisor 进程
///    - 阻塞模式：等待进程结束
///    - 分离模式：立即返回
///
/// # 参数
///
/// * `sandbox` - Microsandbox 配置文件中定义的沙箱名称
/// * `script` - 要在沙箱内执行的脚本名称（如 "start", "shell"）
/// * `project_dir` - 可选的项目目录路径
///   - `None`: 使用当前目录
///   - `Some(path)`: 使用指定路径
/// * `config_file` - 可选的 Microsandbox 配置文件路径
///   - `None`: 使用默认文件名
///   - `Some(path)`: 使用指定文件名
/// * `args` - 传递给沙箱脚本的额外参数
/// * `detach` - 是否在后台运行沙箱
///   - `false`: 阻塞模式，等待沙箱退出
///   - `true`: 分离模式，立即返回
/// * `exec` - 可选的自定义执行命令
///   - `None`: 使用 script 参数或默认脚本
///   - `Some(cmd)`: 直接执行此命令，忽略 script
/// * `use_image_defaults` - 是否应用 OCI 镜像的默认配置
///   - `true`: 从镜像继承 ENTRYPOINT、CMD、ENV 等
///   - `false`: 仅使用配置文件中的设置
///
/// # 返回值
///
/// - `Ok(())` - 沙箱成功运行并退出
/// - `Err(MicrosandboxError)` - 可能的错误：
///   - `ConfigNotFound` - 配置文件不存在
///   - `SandboxNotFoundInConfig` - 指定的沙箱不在配置中
///   - `SupervisorError` - supervisor 进程启动或运行失败
///   - `Io` - 文件系统操作失败
///
/// # Supervisor 进程
///
/// 沙箱由 `msbrun supervisor` 进程管理，该进程负责：
/// - 创建和初始化虚拟机/容器
/// - 挂载文件系统
/// - 设置网络
/// - 执行用户脚本
/// - 清理资源
///
/// # 示例
///
/// ```no_run
/// use std::path::PathBuf;
/// use microsandbox_core::management::sandbox;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // 运行名为 "dev" 的沙箱，执行 "start" 脚本
///     sandbox::run(
///         "dev",
///         Some("start"),
///         None,
///         None,
///         vec![],
///         false,
///         None,
///         true
///     ).await?;
///     Ok(())
/// }
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn run(
    sandbox_name: &str,
    script_name: Option<&str>,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
    args: Vec<String>,
    detach: bool,
    exec: Option<&str>,
    use_image_defaults: bool,
) -> MicrosandboxResult<()> {
    // 准备命令
    let (mut command, is_detached) = prepare_run(
        sandbox_name,
        script_name,
        project_dir,
        config_file,
        args,
        detach,
        exec,
        use_image_defaults,
    )
    .await?;

    // 生成子进程
    let mut child = command.spawn()?;

    // 记录 supervisor 进程 PID
    tracing::info!(
        "started supervisor process with PID: {}",
        child.id().unwrap_or(0)
    );

    // 如果是分离模式，不等待子进程完成
    if is_detached {
        return Ok(());
    }

    // 等待子进程完成
    let status = child.wait().await?;
    if !status.success() {
        tracing::error!(
            "child process — supervisor — exited with status: {}",
            status
        );
        return Err(MicrosandboxError::SupervisorError(format!(
            "child process — supervisor — failed with exit status: {}",
            status
        )));
    }

    Ok(())
}

/// 准备沙箱命令以供执行，但不实际运行。
///
/// # 功能说明
///
/// 此函数执行运行沙箱所需的所有准备工作，但不执行命令。
/// 它与 `run()` 函数的区别在于：
/// - `run()`: 准备并执行命令，等待完成
/// - `prepare_run()`: 仅准备命令，返回给调用者自行执行
///
/// 这种设计模式允许调用者：
/// - 自定义进程执行方式
/// - 在多个沙箱之间批处理
/// - 实现测试桩（mock）
///
/// # 参数
///
/// 参数与 `run()` 函数完全相同，请参见 `run()` 的文档。
///
/// # 返回值
///
/// 返回一个元组：
/// - `Command` - 准备好的命令，可以执行
/// - `bool` - 是否应该以分离模式运行
///
/// # 工作流程
///
/// ```text
/// prepare_run()
///     │
///     ├── 1. 加载配置
///     │   └── 获取沙箱配置
///     │
///     ├── 2. 确保 .menv 环境存在
///     │   └── 创建必需的文件和目录
///     │
///     ├── 3. 设置 rootfs
///     │   ├── 镜像型：拉取镜像，准备 overlayfs
///     │   └── 本地型：准备 native rootfs
///     │
///     ├── 4. 确定执行命令
///     │   └── exec > script > start > shell
///     │
///     └── 5. 构建 supervisor 命令
///         ├── 基本参数（日志、沙箱名等）
///         ├── 资源限制（CPU、内存）
///         ├── 卷映射
///         ├── 端口映射
///         └── rootfs 类型
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn prepare_run(
    sandbox_name: &str,
    script_name: Option<&str>,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
    args: Vec<String>,
    detach: bool,
    exec: Option<&str>,
    use_image_defaults: bool,
) -> MicrosandboxResult<(Command, bool)> {
    // 加载配置文件
    // 返回：(配置对象，规范化的项目目录路径，配置文件名)
    let (config, canonical_project_dir, config_file) =
        config::load_config(project_dir, config_file).await?;

    // 配置文件完整路径
    let config_path = canonical_project_dir.join(&config_file);

    // 确保 .menv 环境文件存在
    // 如果 .menv 目录或必需文件不存在，会创建它们
    let menv_path = canonical_project_dir.join(MICROSANDBOX_ENV_DIR);
    menv::ensure_menv_files(&menv_path).await?;

    // 获取沙箱配置
    let Some(mut sandbox_config) = config.get_sandbox(sandbox_name).cloned() else {
        return Err(MicrosandboxError::SandboxNotFoundInConfig(
            sandbox_name.to_string(),
            config_path,
        ));
    };

    // 记录调试日志：原始沙箱配置
    tracing::debug!("original sandbox config: {:#?}", sandbox_config);

    // 沙箱数据库路径（用于记录沙箱状态）
    let sandbox_db_path = menv_path.join(SANDBOX_DB_FILENAME);

    // 获取沙箱数据库连接池
    let sandbox_pool = db::get_or_create_pool(&sandbox_db_path, &db::SANDBOX_DB_MIGRATOR).await?;

    // 获取配置文件的最后修改时间戳
    // 用于检测配置是否发生变化，决定是否需要重新 patch rootfs
    let config_last_modified: DateTime<Utc> = fs::metadata(&config_path).await?.modified()?.into();

    // 根据镜像类型设置 rootfs
    let rootfs = match sandbox_config.get_image().clone() {
        // 本地 rootfs 类型：直接使用本地目录
        ReferenceOrPath::Path(root_path) => {
            setup_native_rootfs(
                &canonical_project_dir.join(root_path),
                sandbox_name,
                &sandbox_config,
                &config_file,
                &config_last_modified,
                &sandbox_pool,
            )
            .await?
        }
        // OCI 镜像类型：拉取镜像，准备 overlayfs
        ReferenceOrPath::Reference(ref reference) => {
            setup_image_rootfs(
                reference,
                sandbox_name,
                &mut sandbox_config,
                &menv_path,
                &config_file,
                &config_last_modified,
                &sandbox_pool,
                use_image_defaults,
            )
            .await?
        }
    };

    // 确定执行路径和参数
    // 优先级：exec 参数 > script 参数 > start 脚本 > 镜像命令 > shell
    let (exec_path, exec_args) =
        determine_exec_path_and_args(exec, script_name, &sandbox_config, sandbox_name)?;

    // 日志目录
    let log_dir = menv_path.join(LOG_SUBDIR);
    fs::create_dir_all(&log_dir).await?;

    // 记录准备日志
    tracing::info!("preparing sandbox supervisor...");
    tracing::debug!("rootfs: {:?}", rootfs);
    tracing::debug!("exec_path: {}", exec_path);
    tracing::debug!("exec_args: {:?}", exec_args);

    // 解析 msbrun 可执行文件路径
    // 优先使用环境变量 MSBRUN_EXE，否则使用默认路径
    let msbrun_path =
        microsandbox_utils::path::resolve_env_path(MSBRUN_EXE_ENV_VAR, &*DEFAULT_MSBRUN_EXE_PATH)?;

    // 创建 supervisor 命令
    let mut command = Command::new(msbrun_path);
    command
        .arg("supervisor")
        .arg("--log-dir")
        .arg(&log_dir)
        .arg("--sandbox-name")
        .arg(sandbox_name)
        .arg("--config-file")
        .arg(&config_file)
        .arg("--config-last-modified")
        .arg(config_last_modified.to_rfc3339())
        .arg("--sandbox-db-path")
        .arg(&sandbox_db_path)
        .arg("--scope")
        .arg(sandbox_config.get_scope().to_string())
        .arg("--exec-path")
        .arg(&exec_path);

    // CPU 配置
    if let Some(cpus) = sandbox_config.get_cpus() {
        command.arg("--num-vcpus").arg(cpus.to_string());
    }

    // 内存配置
    if let Some(memory) = sandbox_config.get_memory() {
        command.arg("--memory-mib").arg(memory.to_string());
    }

    // 工作目录
    if let Some(workdir) = sandbox_config.get_workdir() {
        command.arg("--workdir-path").arg(workdir);
    }

    // 环境变量
    for env in sandbox_config.get_envs() {
        command.arg("--env").arg(env.to_string());
    }

    // 端口映射
    for port in sandbox_config.get_ports() {
        command.arg("--port-map").arg(port.to_string());
    }

    // 卷映射
    for volume in sandbox_config.get_volumes() {
        match volume {
            PathPair::Distinct { host, guest } => {
                if host.is_absolute() {
                    // 绝对路径，直接使用
                    command.arg("--mapped-dir").arg(volume.to_string());
                } else {
                    // 相对路径，与项目目录拼接
                    let host_path = canonical_project_dir.join(host.as_str());
                    let combined_volume = format!("{}:{}", host_path.display(), guest);
                    command.arg("--mapped-dir").arg(combined_volume);
                }
            }
            PathPair::Same(path) => {
                if path.is_absolute() {
                    // 绝对路径，直接使用
                    command.arg("--mapped-dir").arg(volume.to_string());
                } else {
                    // 相对路径，与项目目录拼接
                    let host_path = canonical_project_dir.join(path.as_str());
                    let combined_volume = format!("{}:{}", host_path.display(), path);
                    command.arg("--mapped-dir").arg(combined_volume);
                }
            }
        }
    }

    // 传递 rootfs
    // 根据类型使用不同的参数
    match rootfs {
        // 本地 rootfs：单个路径
        Rootfs::Native(path) => {
            command.arg("--native-rootfs").arg(path);
        }
        // Overlayfs：多个层路径
        Rootfs::Overlayfs(paths) => {
            for path in paths {
                command.arg("--overlayfs-layer").arg(path);
            }
        }
    }

    // 仅在环境变量中设置了 RUST_LOG 时才传递
    // 这样可以保持子进程与父进程的日志级别一致
    if let Some(rust_log) = std::env::var_os("RUST_LOG") {
        tracing::debug!("using existing RUST_LOG: {:?}", rust_log);
        command.env("RUST_LOG", rust_log);
    }

    // 在分离模式下，忽略 supervisor 进程的输入/输出
    if detach {
        // 安全说明：
        // 调用 `libc::setsid()` 将子进程与父进程的会话和控制终端分离。
        //
        // 这个调用在我们的上下文中是安全的，因为：
        // - 它只是为子进程创建新的会话和进程组，这正是我们想要的
        // - 我们没有修改任何共享的可变状态
        // - 调用除了分离进程外没有副作用
        //
        // ASCII 图示说明分离过程：
        //
        //      [ 主进程 ]
        //           │
        //           ├── 生成 ──► [ Supervisor ]
        //                             │
        //                             └─ 调用 setsid() ──► [ 新会话和进程组 ]
        //                                            (分离)
        //
        // 这确保了即使协调器退出，supervisor 也能独立运行。
        //
        // 注意：pre_exec 是 unsafe 的，因为它接受一个裸函数指针，
        // 该函数会在 fork() 之后、exec() 之前执行。
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        // TODO: 重定向到日志文件
        // 将输入/输出重定向到 /dev/null
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.stdin(Stdio::null());
    } else {
        // 非分离模式：转发输出到当前终端
        command.arg("--forward-output");
    }

    // 最后传递额外参数
    // 这些参数会被传递给沙箱内的脚本或命令
    if !args.is_empty() {
        command.arg("--");
        for arg in args {
            command.arg(arg);
        }
    } else if !exec_args.is_empty() {
        // 如果没有显式提供参数但有从命令解析的参数，使用这些
        command.arg("--");
        for arg in exec_args {
            command.arg(arg);
        }
    }

    Ok((command, detach))
}

/// 从 OCI 镜像创建并运行临时沙箱。
///
/// # 功能说明
///
/// 此函数无需 Microsandbox 配置文件即可快速运行一次性沙箱。
/// 它适用于快速测试、临时执行等场景。
///
/// 临时沙箱的特点：
/// - **无需配置**：直接使用 OCI 镜像
/// - **自动清理**：执行完成后自动删除所有临时文件
/// - **灵活配置**：通过参数指定 CPU、内存、卷等
///
/// # 参数
///
/// * `image` - 用作沙箱基础的 OCI 镜像引用
/// * `script` - 要在沙箱内执行的脚本名称
/// * `cpus` - 可选的虚拟 CPU 数量
/// * `memory` - 可选的内存大小（MiB）
/// * `volumes` - 卷映射列表，格式为 "host_path:guest_path"
/// * `ports` - 端口映射列表，格式为 "host_port:guest_port"
/// * `envs` - 环境变量列表，格式为 "KEY=VALUE"
/// * `workdir` - 可选的沙箱内工作目录路径
/// * `scope` - 可选的网络范围覆盖
/// * `exec` - 可选的自定义执行命令，如果提供则覆盖 `script`
/// * `args` - 传递给指定脚本或命令的额外参数
/// * `use_image_defaults` - 是否应用 OCI 镜像的默认配置
///
/// # 返回值
///
/// - `Ok(())` - 临时沙箱成功运行并退出
/// - `Err(MicrosandboxError)` - 可能的错误：
///   - `ImagePullError` - 无法拉取镜像
///   - `InvalidConfig` - 沙箱配置无效
///   - `SupervisorError` - supervisor 进程失败
///   - `Io` - 文件系统操作失败
///
/// # 内部实现
///
/// 1. **创建临时目录**：使用 `tempfile::tempdir()` 创建
/// 2. **初始化 menv**：在临时目录中创建 .menv 环境
/// 3. **解析参数**：将字符串解析为 PathPair、PortPair、EnvPair
/// 4. **构建配置**：创建临时的 Sandbox 和 Microsandbox 对象
/// 5. **写入配置**：将配置写入临时目录的 sandbox.yaml
/// 6. **运行沙箱**：调用 `run()` 执行
/// 7. **清理**：删除临时目录
///
/// # 示例
///
/// ```no_run
/// use microsandbox_core::oci::Reference;
/// use microsandbox_core::management::sandbox;
/// use typed_path::Utf8UnixPathBuf;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let image = "ubuntu:latest".parse::<Reference>()?;
///
///     // 运行临时的 Ubuntu 沙箱，自定义资源
///     sandbox::run_temp(
///         &image,
///         Some("start"),     // 脚本名称
///         Some(2),           // 2 个 CPU
///         Some(1024),        // 1GB 内存
///         vec![              // 将宿主机的 /tmp 挂载到沙箱的 /data
///             "/tmp:/data".to_string()
///         ],
///         vec![              // 将宿主机的 8080 端口映射到沙箱的 80 端口
///             "8080:80".to_string()
///         ],
///         vec![              // 设置环境变量
///             "DEBUG=1".to_string()
///         ],
///         Some("/app".into()), // 设置工作目录
///         None,              // 无网络范围覆盖
///         None,              // 无自定义命令
///         vec![],            // 无额外参数
///         true               // 使用镜像默认值
///     ).await?;
///     Ok(())
/// }
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn run_temp(
    image: &Reference,
    script: Option<&str>,
    cpus: Option<u8>,
    memory: Option<u32>,
    volumes: Vec<String>,
    ports: Vec<String>,
    envs: Vec<String>,
    workdir: Option<Utf8UnixPathBuf>,
    scope: Option<String>,
    exec: Option<&str>,
    args: Vec<String>,
    use_image_defaults: bool,
) -> MicrosandboxResult<()> {
    // 创建临时目录（不丢失 TempDir guard，以便自动清理）
    let temp_dir = tempfile::tempdir()?;
    let temp_dir_path = temp_dir.path().to_path_buf();

    // 在临时目录中初始化 menv 环境
    menv::initialize(Some(temp_dir_path.clone())).await?;

    // 解析卷、端口、环境变量字符串为对应的类型
    let volumes: Vec<PathPair> = volumes.into_iter().filter_map(|v| v.parse().ok()).collect();
    let ports: Vec<PortPair> = ports.into_iter().filter_map(|p| p.parse().ok()).collect();
    let envs: Vec<EnvPair> = envs.into_iter().filter_map(|e| e.parse().ok()).collect();

    // 构建临时沙箱配置
    // 使用构建器模式，只设置提供的参数
    let sandbox = {
        let mut b = Sandbox::builder().image(ReferenceOrPath::Reference(image.clone()));

        if let Some(cpus) = cpus {
            b = b.cpus(cpus);
        }

        if let Some(memory) = memory {
            b = b.memory(memory);
        }

        if let Some(workdir) = workdir {
            b = b.workdir(workdir);
        }

        if !volumes.is_empty() {
            b = b.volumes(volumes);
        }

        if !ports.is_empty() {
            b = b.ports(ports);
        }

        if !envs.is_empty() {
            b = b.envs(envs);
        }

        if let Some(scope) = scope {
            b = b.scope(scope.parse()?);
        }

        b.build()
    };

    // 创建包含临时沙箱的 Microsandbox 配置
    let config = Microsandbox::builder()
        .sandboxes([(TEMPORARY_SANDBOX_NAME.to_string(), sandbox)])
        .build_unchecked();

    // 将配置写入临时目录
    let config_path = temp_dir_path.join(MICROSANDBOX_CONFIG_FILENAME);
    tokio::fs::write(&config_path, serde_yaml::to_string(&config)?).await?;

    // 使用临时配置运行沙箱
    run(
        TEMPORARY_SANDBOX_NAME,
        script,
        Some(&temp_dir_path),
        None,
        args,
        false,
        exec,
        use_image_defaults,
    )
    .await?;

    // 显式关闭 TempDir 以清理临时目录
    // 注意：temp_dir 在作用域结束时会自动清理，但显式调用可以更清楚地表达意图
    temp_dir.close()?;
    tracing::info!("temporary sandbox directory cleaned up");

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 内部辅助函数
//--------------------------------------------------------------------------------------------------

/// 为基于 OCI 镜像的沙箱设置 rootfs。
///
/// # 功能说明
///
/// 此函数负责准备基于 OCI 镜像的沙箱的根文件系统。它会：
///
/// 1. **拉取镜像**：如果本地没有则从 registry 拉取
/// 2. **获取镜像层**：从数据库查询镜像的所有层
/// 3. **验证层路径**：确保提取的层文件存在
/// 4. **应用镜像默认值**：如果启用，从镜像继承配置
/// 5. **创建补丁目录**：用于存放沙箱脚本
/// 6. **创建可写层**：overlayfs 的顶层
/// 7. **检测配置变化**：决定是否需要 patch rootfs
/// 8. **Patch rootfs**（如果需要）：
///    - 添加沙箱脚本
///    - 配置 DNS
///    - 配置 virtio-fs 挂载
///    - 设置权限覆盖
///
/// # 参数
///
/// * `image` - OCI 镜像引用
/// * `sandbox_name` - 沙箱名称
/// * `sandbox_config` - 沙箱配置（可变，可能被镜像默认值修改）
/// * `menv_path` - .menv 环境目录路径
/// * `config_file` - 配置文件名
/// * `config_last_modified` - 配置文件最后修改时间
/// * `sandbox_pool` - 沙箱数据库连接池
/// * `use_image_defaults` - 是否应用镜像默认配置
///
/// # OverlayFS 结构
///
/// ```text
/// 最终 rootfs = overlay(
///     lowerdir = [层 0, 层 1, ..., 层 N],
///     upperdir = top_rw_path,
///     workdir = ...
/// )
///
/// 其中：
/// - 层 0..N: OCI 镜像层（只读）
/// - patch_dir: 沙箱脚本和补丁
/// - top_rw_path: 运行时可写层
/// ```
///
/// # 返回值
///
/// 返回 `Rootfs::Overlayfs` 包含所有层的路径列表。
#[allow(clippy::too_many_arguments)]
async fn setup_image_rootfs(
    image: &Reference,
    sandbox_name: &str,
    sandbox_config: &mut Sandbox,
    menv_path: &Path,
    config_file: &str,
    config_last_modified: &DateTime<Utc>,
    sandbox_pool: &Pool<Sqlite>,
    use_image_defaults: bool,
) -> MicrosandboxResult<Rootfs> {
    // 拉取镜像
    tracing::info!(?image, "pulling image");
    Image::pull(image.clone(), None).await?;

    // 获取 microsandbox 主目录和数据库路径
    let microsandbox_home_path = env::get_microsandbox_home_path();
    let db_path = microsandbox_home_path.join(OCI_DB_FILENAME);
    let layers_dir = microsandbox_home_path.join(LAYERS_SUBDIR);

    // 获取或创建数据库连接池
    let pool = db::get_or_create_pool(&db_path, &db::OCI_DB_MIGRATOR).await?;

    // 如果启用，应用镜像配置默认值
    // 这会从镜像的 Config 中继承 ENTRYPOINT、CMD、ENV、WORKDIR 等
    if use_image_defaults {
        config::apply_image_defaults(sandbox_config, image, &pool).await?;
        tracing::debug!("updated sandbox config: {:#?}", sandbox_config);
    }

    // 获取镜像的层
    // 首先获取所有层的 digest
    let digests = db::get_image_layer_digests(&pool, &image.to_string()).await?;
    // 然后根据 digest 获取层的详细信息
    let layers = db::get_layers_by_digest(&pool, &digests).await?;
    tracing::info!("found {} layers for image {}", layers.len(), image);

    // 获取提取的层路径
    // TODO: 切换到使用 `LayerOps` trait
    let mut layer_paths = Vec::new();
    for layer in &layers {
        // 构建提取后的层目录路径
        let layer_path = layers_dir.join(format!("{}.{}", layer.digest, EXTRACTED_LAYER_SUFFIX));
        if !layer_path.exists() {
            return Err(MicrosandboxError::PathNotFound(format!(
                "extracted layer {} not found at {}",
                layer.digest,
                layer_path.display()
            )));
        }
        tracing::info!("found extracted layer: {}", layer_path.display());
        layer_paths.push(layer_path);
    }

    // 获取沙箱的限定名称（配置文件名/沙箱名）
    // 用于支持多配置文件的层级结构
    let scoped_name = PathBuf::from(config_file).join(sandbox_name);

    // 创建脚本目录
    // 路径：.menv/patch/<config>/<sandbox>/sandbox/scripts/
    let patch_dir = menv_path.join(PATCH_SUBDIR).join(&scoped_name);
    let script_dir = patch_dir.join(SANDBOX_DIR).join(SCRIPTS_DIR);
    fs::create_dir_all(&script_dir).await?;
    tracing::info!("script_dir: {}", script_dir.display());

    // 创建顶层可写路径
    // 路径：.menv/rw/<config>/<sandbox>/
    // 这是 overlayfs 的 upperdir，沙箱的所有写入都会到这里
    let top_rw_path = menv_path.join(RW_SUBDIR).join(&scoped_name);
    fs::create_dir_all(&top_rw_path).await?;
    tracing::info!("top_rw_path: {}", top_rw_path.display());

    // 检查是否需要 patch rootfs（脚本、卷挂载等）
    // 如果沙箱是新的或配置已更改，则需要 patch
    let should_patch = has_sandbox_config_changed(
        sandbox_pool,
        sandbox_name,
        config_file,
        config_last_modified,
    )
    .await?;

    // 只在沙箱不存在或配置已更改时 patch
    if should_patch {
        tracing::info!("patching sandbox - config has changed");

        // 如果顶层存在 `/.sandbox` 目录，删除它
        // 这样可以确保 patch 是幂等的
        let rw_scripts_dir = top_rw_path.join(SANDBOX_DIR);
        if rw_scripts_dir.exists() {
            fs::remove_dir_all(&rw_scripts_dir).await?;
        }

        // 添加沙箱脚本
        // 脚本会被写入 patch_dir/sandbox/scripts/
        rootfs::patch_with_sandbox_scripts(
            &script_dir,
            sandbox_config.get_scripts(),
            sandbox_config
                .get_shell()
                .as_ref()
                .unwrap_or(&DEFAULT_SHELL.to_string()),
        )
        .await?;

        // 配置默认 DNS 设置 - 检查所有层
        // 只有在没有任何层包含 nameserver 时才会添加
        let mut all_layers = layer_paths.clone();
        all_layers.push(patch_dir.clone());
        rootfs::patch_with_default_dns_settings(&all_layers).await?;

        // 如果定义了卷挂载，则配置 virtio-fs 挂载点
        let volumes = &sandbox_config.get_volumes();
        if !volumes.is_empty() {
            tracing::info!("patching with {} volume mounts", volumes.len());
            // 挂载点配置会被写入 patch_dir 的 /etc/fstab
            rootfs::patch_with_virtiofs_mounts(&patch_dir, volumes).await?;
        }

        // 设置 rootfs 的权限覆盖
        // 这确保容器内的文件权限正确
        rootfs::patch_with_stat_override(&top_rw_path).await?;
    } else {
        tracing::info!("skipping sandbox patch - config unchanged");
    }

    // 将脚本目录和 rootfs 目录添加到层路径列表
    // 这些会成为 overlayfs 的最上层
    layer_paths.push(patch_dir);
    layer_paths.push(top_rw_path);

    // 返回 overlayfs rootfs
    Ok(Rootfs::Overlayfs(layer_paths))
}

/// 为本地 rootfs 沙箱设置根文件系统。
///
/// # 功能说明
///
/// 此函数负责准备使用本地目录作为 rootfs 的沙箱。与镜像型 rootfs 不同，
/// 本地 rootfs 直接使用现有的目录，不需要 overlayfs 多层结构。
///
/// ## 工作流程
///
/// 1. **创建脚本目录**：在 rootfs 内创建 `/.sandbox/scripts`
/// 2. **检测配置变化**：检查是否需要 patch
/// 3. **Patch rootfs**（如果需要）：
///    - 添加沙箱脚本
///    - 配置 DNS
///    - 配置 virtio-fs 挂载
///    - 设置权限覆盖
///
/// # 参数
///
/// * `root_path` - rootfs 目录的根路径
/// * `sandbox_name` - 沙箱名称
/// * `sandbox_config` - 沙箱配置
/// * `config_file` - 配置文件名
/// * `config_last_modified` - 配置文件最后修改时间
/// * `sandbox_pool` - 沙箱数据库连接池
///
/// # 与镜像型的区别
///
/// | 特性 | 本地型 | 镜像型 |
/// |------|--------|--------|
/// | 层结构 | 单层 | 多层（overlayfs） |
/// | rootfs 来源 | 本地目录 | OCI 镜像 |
/// | 可写性 | 直接修改 rootfs | 写入 separate RW 层 |
/// | 性能 | 原生性能 | 有 overlay 开销 |
/// | 使用场景 | 开发调试 | 生产部署 |
///
/// # 返回值
///
/// 返回 `Rootfs::Native` 包含 rootfs 路径。
async fn setup_native_rootfs(
    root_path: &Path,
    sandbox_name: &str,
    sandbox_config: &Sandbox,
    config_file: &str,
    config_last_modified: &DateTime<Utc>,
    sandbox_pool: &Pool<Sqlite>,
) -> MicrosandboxResult<Rootfs> {
    // 创建脚本目录
    // 路径：<root_path>/.sandbox/scripts/
    let scripts_dir = root_path.join(SANDBOX_DIR).join(SCRIPTS_DIR);
    fs::create_dir_all(&scripts_dir).await?;

    // 检查是否需要 patch rootfs
    let should_patch = has_sandbox_config_changed(
        sandbox_pool,
        sandbox_name,
        config_file,
        config_last_modified,
    )
    .await?;

    // 只在沙箱不存在或配置已更改时 patch
    if should_patch {
        tracing::info!("patching sandbox - config has changed");

        // 添加沙箱脚本
        rootfs::patch_with_sandbox_scripts(
            &scripts_dir,
            sandbox_config.get_scripts(),
            sandbox_config
                .get_shell()
                .as_ref()
                .unwrap_or(&DEFAULT_SHELL.to_string()),
        )
        .await?;

        // 配置默认 DNS 设置
        // 对于本地 rootfs，只需检查单个 root 路径
        rootfs::patch_with_default_dns_settings(&[root_path.to_path_buf()]).await?;

        // 如果定义了卷挂载，则配置 virtio-fs 挂载点
        let volumes = &sandbox_config.get_volumes();
        if !volumes.is_empty() {
            tracing::info!("patching with {} volume mounts", volumes.len());
            // 对于本地 rootfs，挂载点创建在 root 路径下
            rootfs::patch_with_virtiofs_mounts(root_path, volumes).await?;
        }

        // 设置 rootfs 的权限覆盖
        rootfs::patch_with_stat_override(root_path).await?;
    } else {
        tracing::info!("skipping sandbox patch - config unchanged");
    }

    // 返回本地 rootfs
    Ok(Rootfs::Native(root_path.to_path_buf()))
}

/// 检查沙箱配置是否已更改。
///
/// # 功能说明
///
/// 此函数通过比较当前配置文件的最后修改时间戳与数据库中
/// 存储的时间戳，判断沙箱的配置是否发生了变化。
///
/// ## 为什么需要这个检查？
///
/// Patch rootfs（添加脚本、配置挂载点等）是一个相对耗时的操作。
/// 如果配置没有变化，可以跳过 patch，直接使用之前准备好的 rootfs，
/// 从而加快沙箱启动速度。
///
/// ## 检查逻辑
///
/// ```text
/// has_sandbox_config_changed()
///     │
///     ├── 查询数据库获取沙箱记录
///     │
///     ├── 沙箱不存在？
///     │   └── 返回 true（需要 patch）
///     │
///     └── 沙箱存在
///         │
///         └── 比较时间戳
///             ├── config_last_modified != stored_timestamp
///             │   └── 返回 true（需要 patch）
///             │
///             └── config_last_modified == stored_timestamp
///                 └── 返回 false（跳过 patch）
/// ```
///
/// # 参数
///
/// * `sandbox_pool` - 沙箱数据库连接池
/// * `sandbox_name` - 沙箱名称
/// * `config_file` - 配置文件名
/// * `config_last_modified` - 配置文件最后修改时间
///
/// # 返回值
///
/// - `Ok(true)` - 沙箱不存在或配置已更改，需要 patch
/// - `Ok(false)` - 沙箱存在且配置未更改，可跳过 patch
/// - `Err(...)` - 数据库查询失败
async fn has_sandbox_config_changed(
    sandbox_pool: &Pool<Sqlite>,
    sandbox_name: &str,
    config_file: &str,
    config_last_modified: &DateTime<Utc>,
) -> MicrosandboxResult<bool> {
    // 检查沙箱是否存在且配置未更改
    let sandbox = db::get_sandbox(sandbox_pool, sandbox_name, config_file).await?;
    Ok(match sandbox {
        Some(sandbox) => {
            // 比较时间戳判断配置是否已更改
            sandbox.config_last_modified != *config_last_modified
        }
        None => true, // 沙箱不存在，需要 patch
    })
}

/// 根据配置确定沙箱的执行命令和参数。
///
/// # 功能说明
///
/// 此函数根据优先级顺序确定沙箱应该执行什么命令：
///
/// ## 优先级顺序
///
/// 1. **显式 exec 参数**（最高优先级）
///    - 用户通过 `--exec` 指定的命令
///    - 直接使用该命令，不添加任何参数
///
/// 2. **指定的 script 参数**
///    - 用户通过 `--script` 指定的脚本
///    - 验证脚本是否存在
///    - 生成脚本路径：`.sandbox/scripts/<script_name>`
///
/// 3. **start 脚本**
///    - 如果沙箱配置中定义了 `start` 脚本
///    - 路径：`.sandbox/scripts/start`
///
/// 4. **沙箱配置的 exec 命令**
///    - 从镜像的 `CMD` 或 `ENTRYPOINT` 继承
///    - 拆分为命令路径和参数
///
/// 5. **shell 命令**（最低优先级）
///    - 如果以上都没有，使用配置的 shell
///    - 路径：`.sandbox/scripts/shell`
///
/// # 参数
///
/// * `exec` - 可选的显式执行命令
/// * `script_name` - 可选的脚本名称
/// * `sandbox_config` - 沙箱配置
/// * `sandbox_name` - 沙箱名称（用于错误报告）
///
/// # 返回值
///
/// 返回 `(exec_path, args)` 元组：
/// - `exec_path` - 要执行的可执行文件路径
/// - `args` - 传递给命令的参数列表
///
/// # 示例
///
/// ```text
/// // exec 参数优先
/// exec = Some("/bin/echo hello")
/// → ("/bin/echo hello", [])
///
/// // 指定脚本
/// script_name = Some("start")
/// → (".sandbox/scripts/start", [])
///
/// // start 脚本
/// sandbox_config.scripts = {"start": "..."}
/// → (".sandbox/scripts/start", [])
///
/// // 镜像命令
/// sandbox_config.command = ["node", "app.js"]
/// → ("node", ["app.js"])
///
/// // shell 回退
/// sandbox_config.shell = "/bin/sh"
/// → ("/bin/sh", [])
/// ```
///
/// # 错误处理
///
/// - 如果指定的脚本不存在，返回 `ScriptNotFoundInSandbox` 错误
/// - 如果没有任何可用的执行方式，返回 `MissingStartOrExecOrShell` 错误
pub fn determine_exec_path_and_args(
    exec: Option<&str>,
    script_name: Option<&str>,
    sandbox_config: &Sandbox,
    sandbox_name: &str,
) -> MicrosandboxResult<(String, Vec<String>)> {
    match exec {
        // 情况 1：有显式 exec 参数
        Some(exec) => Ok((exec.to_string(), Vec::new())),
        None => match script_name {
            // 情况 2：有指定的 script 参数
            Some(script_name) => {
                // 验证脚本是否存在
                // SHELL_SCRIPT_NAME ("shell") 是特殊脚本，始终存在
                if script_name != SHELL_SCRIPT_NAME
                    && !sandbox_config.get_scripts().contains_key(script_name)
                {
                    return Err(MicrosandboxError::ScriptNotFoundInSandbox(
                        script_name.to_string(),
                        sandbox_name.to_string(),
                    ));
                }

                // 生成脚本路径
                let script_path = format!("{}/{}/{}", SANDBOX_DIR, SCRIPTS_DIR, script_name);
                Ok((script_path, Vec::new()))
            }
            None => match sandbox_config.get_scripts().get(START_SCRIPT_NAME) {
                // 情况 3：有 start 脚本
                Some(_) => {
                    let script_path =
                        format!("{}/{}/{}", SANDBOX_DIR, SCRIPTS_DIR, START_SCRIPT_NAME);
                    Ok((script_path, Vec::new()))
                }
                None => {
                    let command = sandbox_config.get_command();
                    if !command.is_empty() {
                        // 情况 4：有镜像命令
                        // 第一个元素是命令，其余是参数
                        let cmd = command[0].clone();
                        let args = command.iter().skip(1).cloned().collect();
                        Ok((cmd, args))
                    } else {
                        // 情况 5：使用 shell
                        sandbox_config
                            .get_shell()
                            .as_ref()
                            .map(|s| (s.to_string(), Vec::new()))
                            .ok_or(MicrosandboxError::MissingStartOrExecOrShell)
                    }
                }
            },
        },
    }
}
