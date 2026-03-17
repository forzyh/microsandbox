//! # 命令处理器模块
//!
//! 本模块包含所有 msb 子命令的处理函数。
//! 每个函数对应一个子命令，负责：
//! 1. 参数验证
//! 2. 调用核心库函数执行实际操作
//! 3. 错误处理和用户反馈
//!
//! ## 设计模式
//!
//! 本模块采用"薄处理器"模式：
//! - 处理器只负责参数转换和错误处理
//! - 实际业务逻辑在 `microsandbox_core` 和 `microsandbox_server` 中
//!
//! ## 命名约定
//!
//! 所有处理函数命名为 `<command>_subcommand`，例如：
//! - `run_subcommand`: 处理 `msb run` 命令
//! - `init_subcommand`: 处理 `msb init` 命令

use clap::{CommandFactory, error::ErrorKind};
use microsandbox_cli::{
    AnsiStyles, MicrosandboxArgs, MicrosandboxCliError, MicrosandboxCliResult, SelfAction,
};
use microsandbox_core::{
    config::START_SCRIPT_NAME,
    management::{
        config::{self, Component, ComponentType, SandboxConfig},
        home, menv, orchestra, sandbox, toolchain,
    },
    oci::Reference,
};
use microsandbox_server::MicrosandboxServerResult;
use microsandbox_utils::{PROJECTS_SUBDIR, env};
use std::{collections::HashMap, path::PathBuf};
use typed_path::Utf8UnixPathBuf;

//--------------------------------------------------------------------------------------------------
// Constants - 常量定义
//--------------------------------------------------------------------------------------------------

/// ### 沙箱脚本分隔符
///
/// 用于分隔沙箱名称和脚本名称的特殊字符。
/// 例如：`myapp~shell` 表示在 `myapp` 沙箱中运行 `shell` 脚本。
///
/// ### 为什么选择 `~`？
/// - 在文件路径中不常见，避免冲突
/// - 易于输入和识别
/// - 在 shell 中通常不需要转义
const SANDBOX_SCRIPT_SEPARATOR: char = '~';

//--------------------------------------------------------------------------------------------------
// Functions: Handlers - 命令处理函数
//--------------------------------------------------------------------------------------------------

/// ## 设置日志级别
///
/// 根据命令行参数设置 Rust 日志系统的输出级别。
///
/// ### 日志级别优先级（从高到低）
/// ```text
/// trace > debug > info > warn > error
/// ```
/// 高级别包含低级别的输出。
///
/// ### 参数
/// - `args`: 解析后的命令行参数
///
/// ### 实现原理
/// 设置 `RUST_LOG` 环境变量，`tracing` 库会读取此变量配置日志行为。
/// 格式：`microsandbox=level,msb=level`
///
/// ### `unsafe` 说明
/// `std::env::set_var` 标记为 unsafe 是因为：
/// - 环境变量是全局状态
/// - 多线程环境下可能导致数据竞争
/// - 但在此场景中，程序启动时设置是安全的
pub fn log_level(args: &MicosandboxArgs) {
    // 按优先级检查日志级别标志（从最详细到最简洁）
    let level = if args.trace {
        Some("trace")
    } else if args.debug {
        Some("debug")
    } else if args.info {
        Some("info")
    } else if args.warn {
        Some("warn")
    } else if args.error {
        Some("error")
    } else {
        None  // 没有指定级别，使用默认
    };

    // 仅当指定了级别时才设置环境变量
    if let Some(level) = level {
        unsafe { std::env::set_var("RUST_LOG", format!("microsandbox={},msb={}", level, level)) };
    }
}

/// ## 添加沙箱处理函数
///
/// 处理 `msb add` 命令，向项目配置中添加新的沙箱定义。
///
/// ### 参数说明
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | sandbox | bool | 是否应用于普通沙箱 |
/// | build | bool | 是否应用于构建沙箱 |
/// | names | Vec<String> | 沙箱名称列表 |
/// | image | String | OCI 镜像名称 |
/// | memory | Option<u32> | 内存限制（MiB） |
/// | cpus | Option<u32> | CPU 核心数 |
/// | volumes | Vec<String> | 卷挂载配置 |
/// | ports | Vec<String> | 端口映射配置 |
/// | envs | Vec<String> | 环境变量 |
/// | env_file | Option<Utf8UnixPathBuf> | 环境变量文件 |
/// | depends_on | Vec<String> | 依赖沙箱 |
/// | workdir | Option<Utf8UnixPathBuf> | 工作目录 |
/// | shell | Option<String> | Shell 类型 |
/// | scripts | Vec<(String, String)> | 自定义脚本 |
/// | start | Option<String> | 启动脚本 |
/// | imports | Vec<(String, String)> | 导入文件 |
/// | exports | Vec<(String, String)> | 导出文件 |
/// | scope | Option<String> | 网络作用域 |
/// | path | Option<PathBuf> | 项目路径 |
/// | config | Option<String> | 配置文件名 |
///
/// ### `#[allow(clippy::too_many_arguments)]` 说明
/// Clippy 默认警告参数过多的函数，但这里是必要的：
/// - 每个参数对应一个命令行选项
/// - 拆分函数会降低代码可读性
#[allow(clippy::too_many_arguments)]
pub async fn add_subcommand(
    sandbox: bool,
    build: bool,
    names: Vec<String>,
    image: String,
    memory: Option<u32>,
    cpus: Option<u32>,
    volumes: Vec<String>,
    ports: Vec<String>,
    envs: Vec<String>,
    env_file: Option<Utf8UnixPathBuf>,
    depends_on: Vec<String>,
    workdir: Option<Utf8UnixPathBuf>,
    shell: Option<String>,
    scripts: Vec<(String, String)>,
    start: Option<String>,
    imports: Vec<(String, String)>,
    exports: Vec<(String, String)>,
    scope: Option<String>,
    path: Option<PathBuf>,
    config: Option<String>,
) -> MicrosandboxCliResult<()> {
    // 验证 --build 和 --sandbox 标志冲突
    validate_build_sandbox_conflict(build, sandbox, "add", Some("[NAMES]"), None);

    // 检查 --build 标志是否支持（当前版本不支持）
    unsupported_build_error(build, "add", Some("[NAMES]"));

    // 将脚本向量转换为 HashMap，便于后续处理
    let mut scripts = scripts.into_iter().collect::<HashMap<String, String>>();

    // 如果指定了启动脚本，添加到脚本集合中
    // START_SCRIPT_NAME 是常量 "start"
    if let Some(start) = start {
        scripts.insert(START_SCRIPT_NAME.to_string(), start);
    }

    // 创建沙箱配置组件
    // Box::new() 用于在堆上分配，因为 Component 是递归枚举
    let component = Component::Sandbox(Box::new(SandboxConfig {
        image,
        memory,
        cpus,
        volumes,
        ports,
        envs,
        env_file,
        depends_on,
        workdir,
        shell,
        scripts,
        // 将 imports 和 exports 的 String 值转换为 Utf8UnixPathBuf
        imports: imports.into_iter().map(|(k, v)| (k, v.into())).collect(),
        exports: exports.into_iter().map(|(k, v)| (k, v.into())).collect(),
        scope,
    }));

    // 调用配置模块的 add 函数，将组件添加到配置文件
    // .await 表示这是一个异步操作
    // ? 操作符自动将错误转换为 MicrosandboxCliError
    config::add(&names, &component, path.as_deref(), config.as_deref()).await?;

    Ok(())
}

/// ## 删除沙箱处理函数
///
/// 处理 `msb remove` / `msb rm` 命令。
pub async fn remove_subcommand(
    sandbox: bool,
    build: bool,
    names: Vec<String>,
    file: Option<PathBuf>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "remove", Some("[NAMES]"), None);
    unsupported_build_error(build, "remove", Some("[NAMES]"));

    // 解析文件路径为（项目路径，配置文件名）元组
    let (path, config) = parse_file_path(file);

    // 调用配置模块删除组件
    config::remove(
        ComponentType::Sandbox,  // 指定组件类型
        &names,                  // 要删除的沙箱名称
        path.as_deref(),         // 项目路径
        config.as_deref(),       // 配置文件名
    )
    .await?;

    Ok(())
}

/// ## 列出沙箱处理函数
///
/// 处理 `msb list` 命令，显示项目中定义的沙箱。
pub async fn list_subcommand(
    sandbox: bool,
    build: bool,
    file: Option<PathBuf>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "list", None, None);
    unsupported_build_error(build, "list", None);

    // 解析文件路径
    let (path, config) = parse_file_path(file);

    // 加载配置文件
    // 返回元组：(配置对象，项目路径，配置文件路径)
    let (config, _, _) = config::load_config(path.as_deref(), config.as_deref()).await?;

    // 使用 menv 模块的 show_list 函数显示沙箱列表
    menv::show_list(config.get_sandboxes());

    Ok(())
}

/// ## 初始化项目处理函数
///
/// 处理 `msb init` 命令，创建新的微沙箱项目。
pub async fn init_subcommand(path: Option<PathBuf>) -> MicrosandboxCliResult<()> {
    // 调用 menv 模块初始化项目环境
    menv::initialize(path).await?;
    Ok(())
}

/// ## 运行沙箱处理函数
///
/// 处理 `msb run` / `msb r` 命令。
///
/// ### 名称格式解析
/// `name` 参数可以是：
/// - `myapp`: 仅沙箱名称
/// - `myapp~shell`: 沙箱名称 + 脚本名称
pub async fn run_subcommand(
    sandbox: bool,
    build: bool,
    name: String,
    file: Option<PathBuf>,
    detach: bool,
    exec: Option<String>,
    args: Vec<String>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "run", Some("[NAME]"), Some("<ARGS>"));

    unsupported_build_error(build, "run", Some("[NAME]"));

    // 解析名称和脚本
    // 例如："myapp~shell" -> ("myapp", Some("shell"))
    let (sandbox, script) = parse_name_and_script(&name);

    // 检查是否同时指定了脚本和 --exec 选项（这是冲突的）
    // matches! 宏用于模式匹配
    if matches!((script, &exec), (Some(_), Some(_))) {
        MicrosandboxArgs::command()
            .override_usage(usage("run", Some("[NAME[~SCRIPT]]"), Some("<ARGS>")))
            .error(
                ErrorKind::ArgumentConflict,
                format!(
                    "cannot specify both a script and an `{}` option.",
                    "--exec".placeholder()
                ),
            )
            .exit();  // 显示错误并退出程序
    }

    // 解析文件路径
    let (path, config) = parse_file_path(file);

    // 调用 sandbox 模块运行沙箱
    sandbox::run(
        sandbox,        // 沙箱名称
        script,         // 脚本名称（可选）
        path.as_deref(),
        config.as_deref(),
        args,           // 传递给脚本的参数
        detach,         // 是否后台运行
        exec.as_deref(), // 要执行的命令（可选）
        true,           // 是否继承终端
    )
    .await?;

    Ok(())
}

/// ## 脚本运行处理函数
///
/// 处理指定脚本的运行命令（如 `msb shell`）。
pub async fn script_run_subcommand(
    sandbox: bool,
    build: bool,
    name: String,
    script: String,
    file: Option<PathBuf>,
    detach: bool,
    args: Vec<String>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, &script, Some("[NAME]"), Some("<ARGS>"));

    unsupported_build_error(build, &script, Some("[NAME]"));

    let (path, config) = parse_file_path(file);
    sandbox::run(
        &name,
        Some(&script),  // 指定脚本名称
        path.as_deref(),
        config.as_deref(),
        args,
        detach,
        None,           // 不使用 --exec
        true,
    )
    .await?;

    Ok(())
}

/// ## 临时沙箱处理函数
///
/// 处理 `msb exe` / `msb x` 命令，直接从镜像运行临时沙箱。
#[allow(clippy::too_many_arguments)]
pub async fn exe_subcommand(
    name: String,
    cpus: Option<u8>,
    memory: Option<u32>,
    volumes: Vec<String>,
    ports: Vec<String>,
    envs: Vec<String>,
    workdir: Option<Utf8UnixPathBuf>,
    scope: Option<String>,
    exec: Option<String>,
    args: Vec<String>,
) -> MicrosandboxCliResult<()> {
    // 解析名称和脚本
    let (image, script) = parse_name_and_script(&name);

    // 将字符串解析为 OCI 引用格式
    let image = image.parse::<Reference>()?;

    // 检查脚本和 --exec 冲突
    if matches!((script, &exec), (Some(_), Some(_))) {
        MicrosandboxArgs::command()
            .override_usage(usage("exe", Some("[NAME[~SCRIPT]]"), Some("<ARGS>")))
            .error(
                ErrorKind::ArgumentConflict,
                format!(
                    "cannot specify both a script and an `{}` option.",
                    "--exec".placeholder()
                ),
            )
            .exit();
    }

    // 调用 sandbox 模块运行临时沙箱
    sandbox::run_temp(
        &image,
        script,
        cpus,
        memory,
        volumes,
        ports,
        envs,
        workdir,
        scope,
        exec.as_deref(),
        args,
        true,
    )
    .await?;

    Ok(())
}

/// ## 启动沙箱处理函数（up）
///
/// 处理 `msb up` 命令，启动项目中的沙箱。
pub async fn up_subcommand(
    sandbox: bool,
    build: bool,
    names: Vec<String>,
    file: Option<PathBuf>,
    detach: bool,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "up", Some("[NAMES]"), None);
    unsupported_build_error(build, "up", Some("[NAMES]"));

    let (path, config) = parse_file_path(file);
    // orchestra 模块负责编排多个沙箱
    orchestra::up(names, path.as_deref(), config.as_deref(), detach).await?;

    Ok(())
}

/// ## 停止沙箱处理函数（down）
///
/// 处理 `msb down` 命令，停止项目中的沙箱。
pub async fn down_subcommand(
    sandbox: bool,
    build: bool,
    names: Vec<String>,
    file: Option<PathBuf>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "down", Some("[NAMES]"), None);
    unsupported_build_error(build, "down", Some("[NAMES]"));

    let (path, config) = parse_file_path(file);
    orchestra::down(names, path.as_deref(), config.as_deref()).await?;

    Ok(())
}

/// ## 显示状态处理函数
///
/// 处理 `msb status` / `msb ps` 命令，显示沙箱资源使用情况。
pub async fn status_subcommand(
    sandbox: bool,
    build: bool,
    names: Vec<String>,
    file: Option<PathBuf>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "status", Some("[NAMES]"), None);
    unsupported_build_error(build, "status", Some("[NAMES]"));

    let (path, config) = parse_file_path(file);
    orchestra::show_status(&names, path.as_deref(), config.as_deref()).await?;

    Ok(())
}

/// ## 查看日志处理函数
///
/// 处理 `msb log` 命令，显示沙箱日志。
pub async fn log_subcommand(
    sandbox: bool,
    build: bool,
    name: String,
    file: Option<PathBuf>,
    follow: bool,
    tail: Option<usize>,
) -> MicrosandboxCliResult<()> {
    validate_build_sandbox_conflict(build, sandbox, "log", Some("[NAME]"), None);
    unsupported_build_error(build, "log", Some("[NAME]"));

    // 检查系统是否安装了 tail 命令（用于 follow 模式）
    if follow {
        let tail_exists = which::which("tail").is_ok();
        if !tail_exists {
            MicrosandboxArgs::command()
                .override_usage(usage("log", Some("[NAME]"), None))
                .error(
                    ErrorKind::InvalidValue,
                    "'tail' command not found. Please install it to use the follow (-f) option.",
                )
                .exit();
        }
    }

    let (project_dir, config_file) = parse_file_path(file);
    menv::show_log(
        project_dir.as_ref(),
        config_file.as_deref(),
        &name,
        follow,
        tail,
    )
    .await?;

    Ok(())
}

/// ## 清理处理函数
///
/// 处理 `msb clean` 命令，清理缓存和临时文件。
///
/// ### 清理范围
/// | 选项 | 清理内容 |
/// |------|----------|
/// | 无 | 当前项目的 .menv 目录 |
/// | --user | 用户级缓存 ($MICROSANDBOX_HOME) |
/// | --all | 以上全部 |
/// | name | 指定沙箱的缓存 |
pub async fn clean_subcommand(
    _sandbox: bool,
    name: Option<String>,
    user: bool,
    all: bool,
    file: Option<PathBuf>,
    force: bool,
) -> MicrosandboxCliResult<()> {
    // 用户级清理
    if user || all {
        // 清理微沙箱主目录
        home::clean(force).await?;
        tracing::info!("user microsandbox home directory cleaned");

        // 清理用户脚本（MSB-ALIAS）
        if force {
            toolchain::clean().await?;
        }

        tracing::info!("user microsandbox scripts cleaned");
    }

    // 项目级清理
    if !user || all {
        if let Some(sandbox_name) = name {
            // 清理指定沙箱
            tracing::info!("cleaning sandbox: {}", sandbox_name);
            let (path, config) = parse_file_path(file);
            menv::clean(path, config.as_deref(), Some(&sandbox_name), force).await?;
        } else {
            // 清理整个项目环境
            tracing::info!("cleaning entire project environment");
            let (path, config) = parse_file_path(file);
            menv::clean(path, config.as_deref(), None, force).await?;
        }
    }

    Ok(())
}

/// ## 服务器启动处理函数
///
/// 处理 `msb server start` 命令。
pub async fn server_start_subcommand(
    host: Option<String>,
    port: Option<u16>,
    project_dir: Option<PathBuf>,
    dev_mode: bool,
    key: Option<String>,
    detach: bool,
    reset_key: bool,
) -> MicrosandboxCliResult<()> {
    // 调用 server 模块启动服务器
    microsandbox_server::start(key, host, port, project_dir, dev_mode, detach, reset_key).await?;
    Ok(())
}

/// ## 服务器停止处理函数
///
/// 处理 `msb server stop` 命令。
pub async fn server_stop_subcommand() -> MicrosandboxServerResult<()> {
    microsandbox_server::stop().await?;
    Ok(())
}

/// ## 服务器密钥生成处理函数
///
/// 处理 `msb server keygen` 命令。
pub async fn server_keygen_subcommand(expire: Option<String>) -> MicrosandboxCliResult<()> {
    // 将字符串格式的过期时间转换为 chrono::Duration
    let duration = if let Some(expire_str) = expire {
        Some(parse_duration_string(&expire_str)?)
    } else {
        None
    };

    microsandbox_server::keygen(duration).await?;

    Ok(())
}

/// ## 服务器 SSH 处理函数
///
/// 处理 `msb server ssh` 命令。
/// （当前版本未实现）
pub async fn server_ssh_subcommand(_sandbox: bool, _name: String) -> MicrosandboxCliResult<()> {
    MicrosandboxArgs::command()
        .override_usage(usage("ssh", Some("[NAME]"), None))
        .error(
            ErrorKind::InvalidValue,
            "SSH functionality is not yet implemented",
        )
        .exit();
}

/// ## 自管理处理函数
///
/// 处理 `msb self` 命令，管理 microsandbox 自身。
pub async fn self_subcommand(action: SelfAction) -> MicrosandboxCliResult<()> {
    match action {
        SelfAction::Upgrade => {
            println!(
                "{} upgrade functionality is not yet implemented",
                "error:".error()
            );
            return Ok(());
        }
        SelfAction::Uninstall => {
            // 1. 清理主目录
            home::clean(true).await?;

            // 2. 清理用户脚本
            toolchain::clean().await?;

            // 3. 卸载二进制文件和库
            toolchain::uninstall().await?;
        }
    }

    Ok(())
}

/// ## 安装脚本处理函数
///
/// 处理 `msb install` / `msb i` 命令，从镜像安装全局脚本。
#[allow(clippy::too_many_arguments)]
pub async fn install_subcommand(
    name: String,
    alias: Option<String>,
    cpus: Option<u8>,
    memory: Option<u32>,
    volumes: Vec<String>,
    ports: Vec<String>,
    envs: Vec<String>,
    workdir: Option<Utf8UnixPathBuf>,
    scope: Option<String>,
    exec: Option<String>,
    args: Vec<String>,
) -> MicrosandboxCliResult<()> {
    let (image, script) = parse_name_and_script(&name);
    let image = image.parse::<Reference>()?;

    // 检查脚本和 --exec 冲突
    if matches!((script, &exec), (Some(_), Some(_))) {
        MicrosandboxArgs::command()
            .override_usage(usage(
                "install",
                Some("[NAME[~SCRIPT]] [ALIAS]"),
                Some("<ARGS>"),
            ))
            .error(
                ErrorKind::ArgumentConflict,
                format!(
                    "cannot specify both a script and an `{}` option.",
                    "--exec".placeholder()
                ),
            )
            .exit();
    }

    // 如果提供了额外参数，发出警告（这些参数在安装时会被忽略）
    if !args.is_empty() {
        tracing::warn!(
            "Extra arguments will be ignored during install. They will be passed to the sandbox when the alias is used."
        );
    }

    // 调用 home 模块安装脚本
    home::install(
        &image,
        script,
        alias.as_deref(),
        cpus,
        memory,
        volumes,
        ports,
        envs,
        workdir,
        scope,
        exec.as_deref(),
        args,
        true,
    )
    .await?;

    Ok(())
}

/// ## 卸载脚本处理函数
///
/// 处理 `msb uninstall` 命令。
pub async fn uninstall_subcommand(script: Option<String>) -> MicrosandboxCliResult<()> {
    match script {
        Some(script_name) => {
            // 卸载指定脚本
            home::uninstall(&script_name).await?;
            tracing::info!("Successfully uninstalled script: {}", script_name);
        }
        None => {
            // 未指定脚本名称，显示错误
            MicrosandboxArgs::command()
                .override_usage(usage("uninstall", Some("[SCRIPT]"), None))
                .error(
                    ErrorKind::InvalidValue,
                    "Please specify the name of the script to uninstall.",
                )
                .exit();
        }
    }

    Ok(())
}

/// ## 服务器日志处理函数
///
/// 处理 `msb server log` 命令。
pub async fn server_log_subcommand(
    _sandbox: bool,
    name: String,
    follow: bool,
    tail: Option<usize>,
) -> MicrosandboxCliResult<()> {
    // 使用项目目录
    let project_path = env::get_microsandbox_home_path().join(PROJECTS_SUBDIR);

    if !project_path.exists() {
        return Err(MicrosandboxCliError::NotFound(
            "Project directory not found".to_string(),
        ));
    }

    // 复用相同的日志显示功能
    menv::show_log(Some(project_path), None, &name, follow, tail).await?;

    Ok(())
}

/// ## 服务器列表处理函数
///
/// 处理 `msb server list` 命令。
pub async fn server_list_subcommand() -> MicrosandboxCliResult<()> {
    // 获取项目目录
    let microsandbox_home_path = env::get_microsandbox_home_path();
    let project_path = microsandbox_home_path.join(PROJECTS_SUBDIR);

    if !project_path.exists() {
        return Err(MicrosandboxCliError::NotFound(
            "Project directory not found".to_string(),
        ));
    }

    // 从项目目录加载配置
    let config_result = config::load_config(Some(project_path.as_path()), None).await;
    match config_result {
        Ok((config, _, _)) => {
            // 使用通用的 show_list 函数显示沙箱
            menv::show_list(config.get_sandboxes());
        }
        Err(err) => {
            return Err(MicrosandboxCliError::ConfigError(format!(
                "Failed to load configuration: {}",
                err
            )));
        }
    }

    Ok(())
}

/// ## 服务器状态处理函数
///
/// 处理 `msb server status` 命令。
pub async fn server_status_subcommand(
    _sandbox: bool,
    names: Vec<String>,
) -> MicrosandboxCliResult<()> {
    // 获取项目目录
    let microsandbox_home_path = env::get_microsandbox_home_path();
    let project_path = microsandbox_home_path.join(PROJECTS_SUBDIR);

    if !project_path.exists() {
        return Err(MicrosandboxCliError::NotFound(
            "Project directory not found".to_string(),
        ));
    }

    orchestra::show_status(&names, Some(project_path.as_path()), None).await?;

    Ok(())
}

/// ## 登录处理函数
///
/// 处理 `msb login` 命令。（未实现）
pub async fn login_subcommand() -> MicrosandboxCliResult<()> {
    println!(
        "{} login functionality is not yet implemented",
        "error:".error()
    );
    Ok(())
}

/// ## 推送处理函数
///
/// 处理 `msb push` 命令。（未实现）
pub async fn push_subcommand(_image: bool, _name: String) -> MicrosandboxCliResult<()> {
    println!(
        "{} push functionality is not yet implemented",
        "error:".error()
    );
    Ok(())
}

//--------------------------------------------------------------------------------------------------
// Functions: Common Errors - 通用错误处理
//--------------------------------------------------------------------------------------------------

/// ## 生成不支持 --build 标志的错误
///
/// 当用户在不支持 --build 的命令中使用该标志时调用。
///
/// ### 参数
/// - `build`: --build 标志是否被设置
/// - `command`: 命令名称，用于错误消息
/// - `positional_placeholder`: 位置参数占位符
fn unsupported_build_error(build: bool, command: &str, positional_placeholder: Option<&str>) {
    if build {
        MicrosandboxArgs::command()
            .override_usage(usage(command, positional_placeholder, None))
            .error(
                ErrorKind::ArgumentConflict,
                format!(
                    "`{}` and `{}` flags are not yet supported.",
                    "--build".literal(),
                    "-b".literal()
                ),
            )
            .exit();
    }
}

//--------------------------------------------------------------------------------------------------
// Functions: Helpers - 辅助函数
//--------------------------------------------------------------------------------------------------

/// ## 生成命令使用格式字符串
///
/// 用于生成错误消息中的使用提示。
///
/// ### 参数
/// - `command`: 命令名称
/// - `positional_placeholder`: 位置参数占位符（如 "[NAMES]"）
/// - `varargs`: 可变参数占位符（如 "<ARGS>"）
///
/// ### 返回值示例
/// ```text
/// msb run [OPTIONS] [NAME[~SCRIPT]] [-- <ARGS...>]
/// ```
fn usage(command: &str, positional_placeholder: Option<&str>, varargs: Option<&str>) -> String {
    let mut usage = format!(
        "{} {} {} {}",
        "msb".literal(),
        command.literal(),
        "[OPTIONS]".placeholder(),
        positional_placeholder.unwrap_or("").placeholder()
    );

    // 如果有可变参数，添加 [-- <ARGS...>] 部分
    if let Some(varargs) = varargs {
        usage.push_str(&format!(
            " {} {} {}",
            "[--".literal(),
            format!("{}...", varargs).placeholder(),
            "]".literal()
        ));
    }

    usage
}

/// ## 解析沙箱名称和脚本名称
///
/// 将 `name~script` 格式的字符串分割为名称和脚本。
///
/// ### 参数
/// - `name_and_script`: 格式为 "name" 或 "name~script" 的字符串
///
/// ### 返回值
/// - `("name", None)`: 仅指定名称
/// - `("name", Some("script"))`: 指定名称和脚本
///
/// ### 示例
/// ```rust,ignore
/// parse_name_and_script("myapp")       // -> ("myapp", None)
/// parse_name_and_script("myapp~shell") // -> ("myapp", Some("shell"))
/// ```
fn parse_name_and_script(name_and_script: &str) -> (&str, Option<&str>) {
    // split_once 在指定分隔符处分割字符串，返回第一个匹配
    let (name, script) = match name_and_script.split_once(SANDBOX_SCRIPT_SEPARATOR) {
        Some((name, script)) => (name, Some(script)),
        None => (name_and_script, None),
    };

    (name, script)
}

/// ## 解析文件路径
///
/// 将用户提供的文件路径解析为（项目路径，配置文件名）元组。
///
/// ### 处理逻辑
///
/// | 输入 | 输出 | 说明 |
/// |------|------|------|
/// | None | (None, None) | 未指定路径 |
/// | /dir (目录) | (Some(/dir), None) | 目录作为项目路径 |
/// | /dir/config.yaml (文件) | (Some(/dir), Some("config.yaml")) | 父目录为项目路径，文件名为配置名 |
/// | config.yaml (无父目录) | (Some(.), Some("config.yaml")) | 使用当前目录 |
///
/// ### 参数
/// - `file`: 可选的文件路径
///
/// ### 返回值
/// - `(Option<PathBuf>, Option<String>)`: 项目路径和配置文件名
pub fn parse_file_path(file: Option<PathBuf>) -> (Option<PathBuf>, Option<String>) {
    let (project_path, config_name) = match file {
        Some(file_path) => {
            if file_path.is_dir() {
                tracing::debug!("File path is a directory: {:?}", file_path);
                // 如果是目录，直接作为项目路径
                (Some(file_path), None)
            } else {
                // 获取配置文件名
                let config_name = file_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(String::from);

                // 获取父目录
                let parent = file_path.parent();

                // 处理三种情况：
                // 1. 没有父目录 (None)
                // 2. 父目录为空字符串 (Some(""))
                // 3. 有效的父目录
                let project_path = match parent {
                    Some(p) if p.as_os_str().is_empty() => {
                        // 父目录为空，使用当前目录
                        Some(PathBuf::from("."))
                    }
                    Some(p) => {
                        // 有效父目录
                        Some(PathBuf::from(p))
                    }
                    None => {
                        // 没有父目录，使用当前目录
                        Some(PathBuf::from("."))
                    }
                };

                (project_path, config_name)
            }
        }
        None => (None, None),
    };

    (project_path, config_name)
}

/// ## 解析时间 duration 字符串
///
/// 将格式如 "1s", "1m", "3h", "2d" 的字符串解析为 `chrono::Duration`。
///
/// ### 支持的时间单位
///
/// | 单位 | 含义 | 示例 |
/// |------|------|------|
/// | s | 秒 | 30s = 30 秒 |
/// | m | 分钟 | 5m = 5 分钟 |
/// | h | 小时 | 2h = 2 小时 |
/// | d | 天 | 7d = 7 天 |
/// | w | 周 | 2w = 14 天 |
/// | mo | 月 | 1mo = 30 天（近似） |
/// | y | 年 | 1y = 365 天（近似） |
/// | (无) | 默认小时 | 5 = 5 小时 |
///
/// ### 参数
/// - `duration_str`: 时间 duration 字符串，如 "1h30m" 或 "7d"
///
/// ### 返回值
/// - `Ok(chrono::Duration)`: 解析成功
/// - `Err(MicrosandboxCliError)`: 解析失败
fn parse_duration_string(duration_str: &str) -> MicrosandboxCliResult<chrono::Duration> {
    // 去除首尾空白字符
    let duration_str = duration_str.trim();

    // 检查空字符串
    if duration_str.is_empty() {
        return Err(MicrosandboxCliError::InvalidArgument(
            "Empty duration string".to_string(),
        ));
    }

    // 分离数字部分和单位部分
    // position 找到第一个非数字字符的位置
    let (value_str, unit) = duration_str.split_at(
        duration_str
            .chars()
            .position(|c| !c.is_ascii_digit())
            .unwrap_or(duration_str.len()),
    );

    // 检查是否有数字部分
    if value_str.is_empty() {
        return Err(MicrosandboxCliError::InvalidArgument(format!(
            "Invalid duration: {}. No numeric value found.",
            duration_str
        )));
    }

    // 解析数字部分
    let value: i64 = value_str.parse().map_err(|_| {
        MicrosandboxCliError::InvalidArgument(format!(
            "Invalid numeric value in duration: {}",
            value_str
        ))
    })?;

    // 根据单位创建对应的 Duration
    match unit {
        "s" => Ok(chrono::Duration::seconds(value)),
        "m" => Ok(chrono::Duration::minutes(value)),
        "h" => Ok(chrono::Duration::hours(value)),
        "d" => Ok(chrono::Duration::days(value)),
        "w" => Ok(chrono::Duration::weeks(value)),
        "mo" => Ok(chrono::Duration::days(value * 30)), // 近似值
        "y" => Ok(chrono::Duration::days(value * 365)), // 近似值
        "" => Ok(chrono::Duration::hours(value)),       // 默认为小时
        _ => Err(MicrosandboxCliError::InvalidArgument(format!(
            "Invalid duration unit: {}. Expected one of: s, m, h, d, w, mo, y",
            unit
        ))),
    }
}

/// ## 验证 --build 和 --sandbox 标志冲突
///
/// 检查用户是否同时指定了 `--build` 和 `--sandbox` 标志，
/// 这两个标志是互斥的，不能同时使用。
///
/// ### 参数
/// - `build`: --build 标志是否设置
/// - `sandbox`: --sandbox 标志是否设置
/// - `command`: 命令名称，用于错误消息
/// - `positional_placeholder`: 位置参数占位符
/// - `varargs`: 可变参数占位符
fn validate_build_sandbox_conflict(
    build: bool,
    sandbox: bool,
    command: &str,
    positional_placeholder: Option<&str>,
    varargs: Option<&str>,
) {
    if build && sandbox {
        MicrosandboxArgs::command()
            .override_usage(usage(command, positional_placeholder, varargs))
            .error(
                ErrorKind::ArgumentConflict,
                format!(
                    "cannot specify both `{}` and `{}` flags",
                    "--sandbox".literal(),
                    "--build".literal()
                ),
            )
            .exit();
    }
}
