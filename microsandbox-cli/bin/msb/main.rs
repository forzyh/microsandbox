#[path = "mod.rs"]
mod msb;

//! # msb 主程序入口
//!
//! 本文件是 `msb` 命令行工具的主入口点。
//! 它负责解析命令行参数、初始化日志系统，并根据子命令调用相应的处理函数。
//!
//! ## 程序流程
//!
//! ```text
//! 1. 解析命令行参数 (MicosandboxArgs::parse())
//! 2. 设置日志级别 (handlers::log_level())
//! 3. 初始化日志订阅器 (tracing_subscriber::fmt::init())
//! 4. 检查版本标志 (--version)
//! 5. 匹配子命令并调用对应处理函数
//! ```
//!
//! ## Rust 异步入门
//!
//! 本程序使用 `tokio` 异步运行时：
//! - `#[tokio::main]`: 将 main 函数标记为异步入口
//! - `async fn main()`: 异步主函数
//! - `.await`: 等待异步操作完成
//!
//! ### 为什么使用异步？
//! - I/O 操作（文件、网络）不会阻塞线程
//! - 可以同时处理多个并发任务
//! - 更好的性能和资源利用率

use clap::{CommandFactory, Parser};
use microsandbox_cli::{
    AnsiStyles, MicrosandboxArgs, MicrosandboxCliResult, MicrosandboxSubcommand, ServerSubcommand,
};
use microsandbox_core::{management::orchestra, oci::Image};
use msb::handlers;

//--------------------------------------------------------------------------------------------------
// Constants - 常量定义
//--------------------------------------------------------------------------------------------------

/// ### Shell 脚本名称常量
///
/// 定义 `shell` 子命令使用的脚本名称。
/// 使用常量而非硬编码字符串的好处：
/// 1. 避免拼写错误
/// 2. 便于统一修改
/// 3. IDE 可以提供自动补全
const SHELL_SCRIPT: &str = "shell";

//--------------------------------------------------------------------------------------------------
// Functions: main - 主函数
//--------------------------------------------------------------------------------------------------

/// ## 程序主入口
///
/// 这是 msb 命令的异步主函数。
/// 使用 `#[tokio::main]` 属性宏，tokio 会自动：
/// 1. 创建异步运行时（Runtime）
/// 2. 执行 async main 函数
/// 3. 等待所有异步操作完成
///
/// ### 返回类型
/// `MicosandboxCliResult<()>` 是 `Result<(), MicrosandboxCliError>` 的别名
/// - `Ok(())`: 程序成功执行
/// - `Err(e)`: 程序执行失败，返回错误
#[tokio::main]
async fn main() -> MicrosandboxCliResult<()> {
    // --------------------------------------------------------------------------
    // 步骤 1: 解析命令行参数
    // --------------------------------------------------------------------------
    // 使用 clap 的 Parser derive 宏自动从环境变量和命令行解析参数
    // 生成的帮助信息会自动包含所有参数和子命令的文档注释
    let args = MicrosandboxArgs::parse();

    // --------------------------------------------------------------------------
    // 步骤 2: 配置日志级别
    // --------------------------------------------------------------------------
    // 根据用户指定的标志（--debug, --info 等）设置日志级别
    handlers::log_level(&args);

    // 初始化 tracing 订阅器
    // tracing 是 Rust 的结构化日志和追踪库
    tracing_subscriber::fmt::init();

    // --------------------------------------------------------------------------
    // 步骤 3: 处理版本标志
    // --------------------------------------------------------------------------
    // 如果用户指定了 --version，显示版本号并退出
    if args.version {
        // env!("CARGO_PKG_VERSION") 是编译时宏，从 Cargo.toml 获取版本号
        println!("{}", format!("v{}", env!("CARGO_PKG_VERSION")).literal());
        return Ok(());
    }

    // --------------------------------------------------------------------------
    // 步骤 4: 匹配并执行子命令
    // --------------------------------------------------------------------------
    // 使用模式匹配处理每个子命令
    // Some(cmd) 表示有子命令，None 表示没有（显示帮助）
    match args.subcommand {
        // ======================================================================
        // 项目初始化命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Init { file }) => {
            // 解析文件路径：区分是目录还是配置文件
            let (path, _) = handlers::parse_file_path(file);
            // 调用初始化处理函数
            handlers::init_subcommand(path).await?;
        }

        // ======================================================================
        // 添加沙箱命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Add {
            sandbox,
            build,
            names,
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
            start,
            imports,
            exports,
            scope,
            file,
        }) => {
            // 解析配置文件路径
            let (path, config) = handlers::parse_file_path(file);
            // 调用添加处理函数，传递所有参数
            handlers::add_subcommand(
                sandbox, build, names, image, memory, cpus, volumes, ports, envs, env_file,
                depends_on, workdir, shell, scripts, start, imports, exports, scope, path, config,
            )
            .await?;
        }

        // ======================================================================
        // 删除沙箱命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Remove {
            sandbox,
            build,
            names,
            file,
        }) => {
            handlers::remove_subcommand(sandbox, build, names, file).await?;
        }

        // ======================================================================
        // 列出沙箱命令
        // ======================================================================
        Some(MicrosandboxSubcommand::List {
            sandbox,
            build,
            file,
        }) => {
            handlers::list_subcommand(sandbox, build, file).await?;
        }

        // ======================================================================
        // 拉取镜像命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Pull { name, layer_path }) => {
            // 直接调用 Image::pull 异步函数
            // ? 操作符会自动将错误转换为 MicrosandboxCliError
            Image::pull(name, layer_path).await?;
        }

        // ======================================================================
        // 运行沙箱命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Run {
            sandbox,
            build,
            name,
            file,
            detach,
            exec,
            args,
        }) => {
            handlers::run_subcommand(sandbox, build, name, file, detach, exec, args).await?;
        }

        // ======================================================================
        // Shell 命令（特殊的 run 命令，运行 shell 脚本）
        // ======================================================================
        Some(MicrosandboxSubcommand::Shell {
            sandbox,
            build,
            name,
            file,
            detach,
            args,
        }) => {
            // 复用 script_run_subcommand，指定脚本名为 "shell"
            handlers::script_run_subcommand(
                sandbox,
                build,
                name,
                SHELL_SCRIPT.to_string(),
                file,
                detach,
                args,
            )
            .await?;
        }

        // ======================================================================
        // 临时沙箱命令（exe）
        // ======================================================================
        Some(MicrosandboxSubcommand::Exe {
            image: _image,  // _image 表示有意未使用的参数
            name,
            cpus,
            memory,
            volumes,
            ports,
            envs,
            workdir,
            scope,
            exec,
            args,
        }) => {
            handlers::exe_subcommand(
                name, cpus, memory, volumes, ports, envs, workdir, scope, exec, args,
            )
            .await?;
        }

        // ======================================================================
        // 安装脚本命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Install {
            image: _image,
            name,
            alias,
            cpus,
            memory,
            volumes,
            ports,
            envs,
            workdir,
            scope,
            exec,
            args,
        }) => {
            handlers::install_subcommand(
                name, alias, cpus, memory, volumes, ports, envs, workdir, scope, exec, args,
            )
            .await?;
        }

        // ======================================================================
        // 卸载脚本命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Uninstall { script }) => {
            handlers::uninstall_subcommand(script).await?;
        }

        // ======================================================================
        // 应用配置命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Apply { file, detach }) => {
            let (path, config) = handlers::parse_file_path(file);
            // orchestra 模块负责编排多个沙箱的启动/停止
            orchestra::apply(path.as_deref(), config.as_deref(), detach).await?;
        }

        // ======================================================================
        // 启动沙箱命令（up）
        // ======================================================================
        Some(MicrosandboxSubcommand::Up {
            sandbox,
            build,
            names,
            file,
            detach,
        }) => {
            handlers::up_subcommand(sandbox, build, names, file, detach).await?;
        }

        // ======================================================================
        // 停止沙箱命令（down）
        // ======================================================================
        Some(MicrosandboxSubcommand::Down {
            sandbox,
            build,
            names,
            file,
        }) => {
            handlers::down_subcommand(sandbox, build, names, file).await?;
        }

        // ======================================================================
        // 显示状态命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Status {
            sandbox,
            build,
            names,
            file,
        }) => {
            handlers::status_subcommand(sandbox, build, names, file).await?;
        }

        // ======================================================================
        // 查看日志命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Log {
            sandbox,
            build,
            name,
            file,
            follow,
            tail,
        }) => {
            handlers::log_subcommand(sandbox, build, name, file, follow, tail).await?;
        }

        // ======================================================================
        // 清理命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Clean {
            sandbox,
            name,
            user,
            all,
            file,
            force,
        }) => {
            handlers::clean_subcommand(sandbox, name, user, all, file, force).await?;
        }

        // ======================================================================
        // 自管理命令（self）
        // ======================================================================
        Some(MicrosandboxSubcommand::Self_ { action }) => {
            handlers::self_subcommand(action).await?;
        }

        // ======================================================================
        // 服务器管理命令
        // ======================================================================
        // 服务器命令有嵌套的子命令，需要再次匹配
        Some(MicrosandboxSubcommand::Server { subcommand }) => match subcommand {
            ServerSubcommand::Start {
                host,
                port,
                project_dir,
                dev_mode,
                key,
                detach,
                reset_key,
            } => {
                handlers::server_start_subcommand(
                    host,
                    port,
                    project_dir,
                    dev_mode,
                    key,
                    detach,
                    reset_key,
                )
                .await?;
            }
            ServerSubcommand::Stop => {
                handlers::server_stop_subcommand().await?;
            }
            ServerSubcommand::Keygen { expire } => {
                handlers::server_keygen_subcommand(expire).await?;
            }
            ServerSubcommand::Log {
                sandbox,
                name,
                follow,
                tail,
            } => {
                handlers::server_log_subcommand(sandbox, name, follow, tail).await?;
            }
            ServerSubcommand::List => {
                handlers::server_list_subcommand().await?;
            }
            ServerSubcommand::Status { sandbox, names } => {
                handlers::server_status_subcommand(sandbox, names).await?;
            }
            ServerSubcommand::Ssh { sandbox, name } => {
                handlers::server_ssh_subcommand(sandbox, name).await?;
            }
        },

        // ======================================================================
        // 其他命令
        // ======================================================================
        Some(MicrosandboxSubcommand::Login) => {
            handlers::login_subcommand().await?;
        }
        Some(MicrosandboxSubcommand::Push { image, name }) => {
            handlers::push_subcommand(image, name).await?;
        }

        // 未实现的子命令（占位符）
        Some(_) => (), // TODO: 实现其他子命令

        // 没有子命令时，显示帮助信息
        None => {
            MicrosandboxArgs::command().print_help()?;
        }
    }

    // 返回 Ok 表示程序成功执行
    Ok(())
}
