//! Microsandbox 环境管理模块。
//!
//! # 概述
//!
//! 本模块负责 Microsandbox 环境（menv）的初始化和管理工作。
//!
//! ## 什么是 Microsandbox 环境？
//!
//! Microsandbox 环境（简称 `.menv`）是一个项目目录中的特殊子目录，它包含
//! 运行沙箱所需的所有组件：
//!
//! ```text
//! project/
//! ├── .menv/                    # Microsandbox 环境目录
//! │   ├── config.yaml           # Microsandbox 配置文件
//! │   ├── sandbox.db            # 沙箱数据库（SQLite）
//! │   ├── log/                  # 日志目录
//! │   │   └── <config>/<sandbox>.log
//! │   ├── rw/                   # 可写层目录（用于 overlayfs）
//! │   │   └── <config>/<sandbox>/
//! │   └── patch/                # 补丁目录（存放脚本等）
//! │       └── <config>/<sandbox>/
//! ├── .gitignore                # Git 忽略文件（自动更新）
//! └── sandbox.yaml              # 沙箱配置文件
//! ```
//!
//! # 主要功能
//!
//! | 函数 | 功能描述 |
//! |------|----------|
//! | `initialize()` | 在项目目录中初始化 `.menv` 环境 |
//! | `clean()` | 清理沙箱环境（可选择清理整个项目或单个沙箱） |
//! | `show_log()` | 显示沙箱日志（支持 follow 模式） |
//! | `show_list()` | 显示沙箱列表（CLI 功能） |
//! | `show_list_projects()` | 显示多项目沙箱列表（服务器模式） |
//!
//! # 架构设计
//!
//! ## 环境初始化流程
//!
//! ```text
//! initialize()
//!     │
//!     ├── 1. 创建 .menv 目录
//!     │
//!     ├── 2. 创建必需的子目录和文件
//!     │   ├── log/           - 日志目录
//!     │   ├── rw/            - 可写层目录
//!     │   └── sandbox.db     - SQLite 数据库
//!     │
//!     ├── 3. 创建默认配置文件（如果不存在）
//!     │
//!     └── 4. 更新 .gitignore（添加 .menv 条目）
//! ```
//!
//! ## 清理流程
//!
//! ```text
//! clean(sandbox_name)
//!     │
//!     ├── sandbox_name == None?
//!     │   │
//!     │   ├── Yes: 删除整个 .menv 目录
//!     │   │
//!     │   └── No: 只清理指定沙箱
//!     │       ├── 删除 rw/<config>/<sandbox>/ 目录
//!     │       ├── 删除 patch/<config>/<sandbox>/ 目录
//!     │       ├── 删除 log/<config>/<sandbox>.log 文件
//!     │       └── 从数据库删除沙箱记录
//! ```
//!
//! # 使用示例
//!
//! ```no_run
//! use microsandbox_core::management::menv;
//! use std::path::PathBuf;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // 在当前目录初始化环境
//! menv::initialize(None).await?;
//!
//! // 在指定目录初始化环境
//! menv::initialize(Some("my_project".into())).await?;
//!
//! // 清理整个项目环境
//! menv::clean(None, None, None, false).await?;
//!
//! // 清理指定沙箱
//! menv::clean(None, None, Some("dev"), false).await?;
//!
//! // 查看沙箱日志
//! menv::show_log(None::<&std::path::Path>, None, "dev", false, Some(100)).await?;
//! # Ok(())
//! # }
//! ```

use crate::{MicrosandboxError, MicrosandboxResult};

#[cfg(feature = "cli")]
use microsandbox_utils::term;
use microsandbox_utils::{
    DEFAULT_CONFIG, LOG_SUBDIR, MICROSANDBOX_CONFIG_FILENAME, MICROSANDBOX_ENV_DIR, PATCH_SUBDIR,
    RW_SUBDIR, SANDBOX_DB_FILENAME,
};
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};

use super::{config, db};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// [CLI 功能] 移除 .menv 目录的提示信息
#[cfg(feature = "cli")]
const REMOVE_MENV_DIR_MSG: &str = "Remove .menv directory";

/// [CLI 功能] 初始化 .menv 目录的提示信息
#[cfg(feature = "cli")]
const INITIALIZE_MENV_DIR_MSG: &str = "Initialize .menv directory";

/// [CLI 功能] 创建默认配置文件的提示信息
#[cfg(feature = "cli")]
const CREATE_DEFAULT_CONFIG_MSG: &str = "Create default config file";

/// [CLI 功能] 清理沙箱的提示信息
#[cfg(feature = "cli")]
const CLEAN_SANDBOX_MSG: &str = "Clean sandbox";

//--------------------------------------------------------------------------------------------------
// 公开函数
//--------------------------------------------------------------------------------------------------

/// 在指定路径初始化新的 Microsandbox 环境。
///
/// # 功能说明
///
/// 此函数会在项目目录中创建 `.menv` 环境，包括：
///
/// 1. **创建 `.menv` 目录**：如果不存在则创建
/// 2. **创建必需的子目录**：
///    - `log/` - 存放沙箱运行日志
///    - `rw/` - overlayfs 的可写层
/// 3. **初始化数据库**：创建 `sandbox.db` SQLite 数据库
/// 4. **创建默认配置文件**：如果 `sandbox.yaml` 不存在则创建
/// 5. **更新 `.gitignore`**：确保 `.menv/` 被 Git 忽略
///
/// # 参数
///
/// * `project_dir` - 可选的项目目录路径
///   - `None`: 使用当前目录
///   - `Some(path)`: 使用指定路径
///
/// # 目录结构
///
/// 初始化后的目录结构：
///
/// ```text
/// project/
/// ├── .menv/
/// │   ├── sandbox.db          # 沙箱数据库
/// │   ├── log/                # 日志目录
/// │   └── rw/                 # 可写层目录
/// ├── sandbox.yaml            # 默认配置文件（如不存在）
/// └── .gitignore              # 已添加 .menv/ 条目
/// ```
///
/// # 示例
///
/// ```no_run
/// use microsandbox_core::management::menv;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 在当前目录初始化
/// menv::initialize(None).await?;
///
/// // 在指定目录初始化
/// menv::initialize(Some("my_project".into())).await?;
/// # Ok(())
/// # }
/// ```
pub async fn initialize(project_dir: Option<PathBuf>) -> MicrosandboxResult<()> {
    // 获取目标路径，如果未指定则默认为当前目录
    let project_dir = project_dir.unwrap_or_else(|| PathBuf::from("."));
    let menv_path = project_dir.join(MICROSANDBOX_ENV_DIR);

    // [CLI 功能] 创建进度提示（如果 .menv 目录不存在）
    #[cfg(feature = "cli")]
    let initialize_menv_dir_sp = if !menv_path.exists() {
        Some(term::create_spinner(
            INITIALIZE_MENV_DIR_MSG.to_string(),
            None,
            None,
        ))
    } else {
        None
    };

    // 创建 .menv 目录（如果不存在）
    // create_dir_all 是幂等的：目录已存在时不会报错
    fs::create_dir_all(&menv_path).await?;

    // 创建 Microsandbox 环境所需的文件和子目录
    ensure_menv_files(&menv_path).await?;

    // 创建默认配置文件（如果不存在）
    create_default_config(&project_dir).await?;
    tracing::info!(
        "config file at {}",
        project_dir.join(MICROSANDBOX_CONFIG_FILENAME).display()
    );

    // 更新 .gitignore，确保包含 .menv 目录
    update_gitignore(&project_dir).await?;

    // [CLI 功能] 完成进度提示
    #[cfg(feature = "cli")]
    if let Some(sp) = initialize_menv_dir_sp {
        sp.finish();
    }

    Ok(())
}

/// 清理项目或指定沙箱的 Microsandbox 环境。
///
/// # 功能说明
///
/// 此函数有两种工作模式：
///
/// ## 模式 1：清理整个项目（`sandbox_name = None`）
///
/// 删除整个 `.menv` 目录及其所有内容：
/// - 所有沙箱的数据
/// - 所有日志文件
/// - 数据库文件
///
/// **注意**：如果配置文件存在且 `force = false`，则不会执行清理，
/// 这是为了防止意外删除正在使用的环境。
///
/// ## 模式 2：清理指定沙箱（`sandbox_name = Some(name)`）
///
/// 只删除指定沙箱的相关数据：
/// - `rw/<config>/<sandbox>/` - 沙箱的可写层目录
/// - `patch/<config>/<sandbox>/` - 沙箱的补丁目录
/// - `log/<config>/<sandbox>.log` - 沙箱日志文件
/// - 数据库中的沙箱记录
///
/// **注意**：如果沙箱存在于配置文件中且 `force = false`，则不会执行清理。
///
/// # 参数
///
/// * `project_dir` - 可选的项目目录路径
///   - `None`: 使用当前目录
///   - `Some(path)`: 使用指定路径
/// * `config_file` - 可选的配置文件路径
///   - `None`: 使用默认文件名 `sandbox.yaml`
///   - `Some(path)`: 使用指定文件名
/// * `sandbox_name` - 可选的沙箱名称
///   - `None`: 清理整个项目
///   - `Some(name)`: 只清理指定沙箱
/// * `force` - 是否强制清理
///   - `false`: 如果配置文件或沙箱存在，拒绝清理
///   - `true`: 忽略存在性检查，强制清理
///
/// # 目录层级结构
///
/// Microsandbox 支持多配置文件，因此使用层级结构：
///
/// ```text
/// .menv/
/// ├── rw/
/// │   └── sandbox.yaml/       # 配置文件名作为目录
/// │       └── dev/            # 沙箱名
/// ├── patch/
/// │   └── sandbox.yaml/
/// │       └── dev/
/// └── log/
///     └── sandbox.yaml/
///         └── dev.log
/// ```
///
/// # 示例
///
/// ```no_run
/// use microsandbox_core::management::menv;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 清理当前目录的整个项目环境
/// menv::clean(None, None, None, false).await?;
///
/// // 清理当前目录的指定沙箱
/// menv::clean(None, None, Some("dev"), false).await?;
///
/// // 使用自定义配置文件，强制清理
/// menv::clean(None, Some("custom.yaml"), Some("dev"), true).await?;
/// # Ok(())
/// # }
/// ```
pub async fn clean(
    project_dir: Option<PathBuf>,
    config_file: Option<&str>,
    sandbox_name: Option<&str>,
    force: bool,
) -> MicrosandboxResult<()> {
    // 获取目标路径，如果未指定则默认为当前目录
    let project_dir = project_dir.unwrap_or_else(|| PathBuf::from("."));
    let menv_path = project_dir.join(MICROSANDBOX_ENV_DIR);

    // 尝试加载配置文件（如果存在）
    let config_result =
        crate::management::config::load_config(Some(&project_dir), config_file).await;

    // 如果未指定沙箱名称，清理整个项目
    if sandbox_name.is_none() {
        #[cfg(feature = "cli")]
        let remove_menv_dir_sp = term::create_spinner(REMOVE_MENV_DIR_MSG.to_string(), None, None);

        // 如果配置文件存在且未强制，拒绝清理
        if config_result.is_ok() && !force {
            #[cfg(feature = "cli")]
            term::finish_with_error(&remove_menv_dir_sp);

            #[cfg(feature = "cli")]
            println!(
                "Configuration file exists. Use {} to clean the entire environment",
                console::style("--force").yellow()
            );

            tracing::info!(
                "Configuration file exists. Use --force to clean the entire environment"
            );
            return Ok(());
        }

        // 检查 .menv 目录是否存在
        if menv_path.exists() {
            // 删除 .menv 目录及其所有内容
            fs::remove_dir_all(&menv_path).await?;
            tracing::info!(
                "Removed microsandbox environment at {}",
                menv_path.display()
            );
        } else {
            tracing::info!(
                "No microsandbox environment found at {}",
                menv_path.display()
            );
        }

        #[cfg(feature = "cli")]
        remove_menv_dir_sp.finish();

        return Ok(());
    }

    // 执行到这里说明要清理指定的沙箱
    let sandbox_name = sandbox_name.unwrap();
    let config_file = config_file.unwrap_or(MICROSANDBOX_CONFIG_FILENAME);

    #[cfg(feature = "cli")]
    let clean_sandbox_sp = term::create_spinner(
        format!("{} '{}'", CLEAN_SANDBOX_MSG, sandbox_name),
        None,
        None,
    );

    // 如果沙箱存在于配置中且未强制，拒绝清理
    if let Ok((config, _, _)) = config_result
        && config.get_sandbox(sandbox_name).is_some()
        && !force
    {
        #[cfg(feature = "cli")]
        term::finish_with_error(&clean_sandbox_sp);

        #[cfg(feature = "cli")]
        println!(
            "Sandbox '{}' exists in configuration. Use {} to clean it",
            sandbox_name,
            console::style("--force").yellow()
        );

        tracing::info!(
            "Sandbox '{}' exists in configuration. Use --force to clean it",
            sandbox_name
        );
        return Ok(());
    }

    // 获取沙箱的限定名称（配置文件名/沙箱名）
    // 例如："sandbox.yaml/dev"
    let scoped_name = PathBuf::from(config_file).join(sandbox_name);

    // 清理沙箱特定的目录
    let rw_path = menv_path.join(RW_SUBDIR).join(&scoped_name);
    let patch_path = menv_path.join(PATCH_SUBDIR).join(&scoped_name);

    // 删除沙箱目录（如果存在）
    if rw_path.exists() {
        fs::remove_dir_all(&rw_path).await?;
        tracing::info!("Removed sandbox RW directory at {}", rw_path.display());
    }

    if patch_path.exists() {
        fs::remove_dir_all(&patch_path).await?;
        tracing::info!(
            "Removed sandbox patch directory at {}",
            patch_path.display()
        );
    }

    // 删除日志文件（如果存在）
    let log_file = menv_path
        .join(LOG_SUBDIR)
        .join(config_file)
        .join(format!("{}.log", sandbox_name));

    if log_file.exists() {
        fs::remove_file(&log_file).await?;
        tracing::info!("Removed sandbox log file at {}", log_file.display());
    }

    // 从数据库删除沙箱记录
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);
    if db_path.exists() {
        let pool = db::get_or_create_pool(&db_path, &db::SANDBOX_DB_MIGRATOR).await?;
        db::delete_sandbox(&pool, sandbox_name, config_file).await?;
        tracing::info!("Removed sandbox {} from database", sandbox_name);
    }

    #[cfg(feature = "cli")]
    clean_sandbox_sp.finish();

    Ok(())
}

/// 显示沙箱的日志。
///
/// # 功能说明
///
/// 此函数支持两种日志查看模式：
///
/// ## 模式 1：Follow 模式（`follow = true`）
///
/// - 使用 `tail -f` 命令实时跟踪日志文件
/// - 适合监控正在运行的沙箱
/// - 需要系统安装 `tail` 命令
///
/// ## 模式 2：普通模式（`follow = false`）
///
/// - 读取并显示日志文件内容
/// - 可选择显示全部日志或最后 N 行
/// - 适合查看历史日志
///
/// # 参数
///
/// * `project_dir` - 可选的项目目录路径
///   - `None`: 使用当前目录
///   - `Some(path)`: 使用指定路径
/// * `config_file` - 可选的配置文件路径
///   - `None`: 使用默认文件名
///   - `Some(path)`: 使用指定文件名
/// * `sandbox_name` - 要查看日志的沙箱名称
/// * `follow` - 是否使用 follow 模式（类似 `tail -f`）
/// * `tail` - 可选的行数限制
///   - `None`: 显示全部日志
///   - `Some(n)`: 显示最后 n 行
///
/// # 日志文件路径
///
/// 日志文件遵循层级结构：
/// `<project_dir>/.menv/log/<config>/<sandbox>.log`
///
/// # 错误处理
///
/// - 如果 `follow = true` 但系统没有 `tail` 命令，返回 `CommandNotFound` 错误
/// - 如果日志文件不存在，返回 `LogNotFound` 错误
/// - 如果 `tail -f` 进程异常退出，返回 `ProcessWaitError` 错误
///
/// # 示例
///
/// ```no_run
/// use microsandbox_core::management::menv;
/// use std::path::Path;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 显示沙箱的全部日志
/// menv::show_log(None::<&Path>, None, "my-sandbox", false, None).await?;
///
/// // 显示最后 100 行日志
/// menv::show_log(None::<&Path>, None, "my-sandbox", false, Some(100)).await?;
///
/// // 实时跟踪日志（follow 模式）
/// menv::show_log(None::<&Path>, None, "my-sandbox", true, None).await?;
/// # Ok(())
/// # }
/// ```
pub async fn show_log(
    project_dir: Option<impl AsRef<Path>>,
    config_file: Option<&str>,
    sandbox_name: &str,
    follow: bool,
    tail: Option<usize>,
) -> MicrosandboxResult<()> {
    // 如果请求 follow 模式，检查 tail 命令是否存在
    if follow {
        let tail_exists = which::which("tail").is_ok();
        if !tail_exists {
            return Err(MicrosandboxError::CommandNotFound(
                "tail command not found. Please install it to use the follow (-f) option."
                    .to_string(),
            ));
        }
    }

    // 加载配置以获取规范路径
    let (_, canonical_project_dir, config_file) =
        config::load_config(project_dir.as_ref().map(|p| p.as_ref()), config_file).await?;

    // 构建日志文件路径：
    // <project_dir>/.menv/log/<config>/<sandbox>.log
    let log_path = canonical_project_dir
        .join(MICROSANDBOX_ENV_DIR)
        .join(LOG_SUBDIR)
        .join(&config_file)
        .join(format!("{}.log", sandbox_name));

    // 检查日志文件是否存在
    if !log_path.exists() {
        return Err(MicrosandboxError::LogNotFound(format!(
            "Log file not found at {}",
            log_path.display()
        )));
    }

    if follow {
        // Follow 模式：使用 tokio::process::Command 运行 `tail -f`
        let mut child = tokio::process::Command::new("tail")
            .arg("-f")
            .arg(&log_path)
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .spawn()?;

        // 等待 tail 进程结束
        let status = child.wait().await?;
        if !status.success() {
            return Err(MicrosandboxError::ProcessWaitError(format!(
                "tail process exited with status: {}",
                status
            )));
        }
    } else {
        // 普通模式：读取文件内容
        let contents = tokio::fs::read_to_string(&log_path).await?;

        // 分割为行
        let lines: Vec<&str> = contents.lines().collect();

        // 如果指定了 tail 参数，只显示最后 N 行
        let lines_to_print = if let Some(n) = tail {
            if n >= lines.len() {
                &lines[..]
            } else {
                &lines[lines.len() - n..]
            }
        } else {
            &lines[..]
        };

        // 打印日志行
        for line in lines_to_print {
            println!("{}", line);
        }
    }

    Ok(())
}

/// 显示格式化的沙箱列表。
///
/// # 功能说明
///
/// 此函数以标准化的格式显示沙箱配置信息，包括：
/// - 沙箱名称和序号
/// - 使用的镜像
/// - 资源配置（CPU、内存）
/// - 网络范围
/// - 端口映射
/// - 卷映射
/// - 脚本列表
/// - 依赖关系
///
/// # 参数
///
/// * `sandboxes` - 沙箱配置的 HashMap 引用
///
/// # 输出格式示例
///
/// ```text
/// 1. dev
///    Image: ubuntu:22.04
///    Resources: 2 CPUs, 1024 MiB
///    Network: private
///    Ports: 8080:80, 443:443
///    Volumes: ./data:/data, ./config:/etc/config
///    Scripts: start, shell, test
///    Depends On: db, cache
///
/// Total: 1
/// ```
///
/// # 示例
///
/// ```no_run
/// use microsandbox_core::management::menv;
/// use microsandbox_core::management::config;
/// use std::path::Path;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 显示本地项目的所有沙箱
/// let (config, _, _) = config::load_config(None::<&Path>, None).await?;
/// menv::show_list(config.get_sandboxes());
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "cli")]
pub fn show_list<'a, I>(sandboxes: I)
where
    I: IntoIterator<Item = (&'a String, &'a crate::config::Sandbox)>,
{
    use console::style;
    use std::collections::HashMap;

    // 将迭代器转换为 HashMap，便于处理
    let sandboxes: HashMap<&String, &crate::config::Sandbox> = sandboxes.into_iter().collect();

    // 如果没有沙箱，显示提示信息
    if sandboxes.is_empty() {
        println!("No sandboxes found");
        return;
    }

    // 遍历并显示每个沙箱
    for (i, (name, sandbox)) in sandboxes.iter().enumerate() {
        // 在沙箱之间添加空行
        if i > 0 {
            println!();
        }

        // 序号和名称
        println!("{}. {}", style(i + 1).bold(), style(*name).bold());

        // 镜像
        println!("   {}: {}", style("Image").dim(), sandbox.get_image());

        // 资源
        let mut resources = Vec::new();
        if let Some(cpus) = sandbox.get_cpus() {
            resources.push(format!("{} CPUs", cpus));
        }
        if let Some(memory) = sandbox.get_memory() {
            resources.push(format!("{} MiB", memory));
        }
        if !resources.is_empty() {
            println!("   {}: {}", style("Resources").dim(), resources.join(", "));
        }

        // 网络
        println!("   {}: {}", style("Network").dim(), sandbox.get_scope());

        // 端口
        if !sandbox.get_ports().is_empty() {
            let ports = sandbox
                .get_ports()
                .iter()
                .map(|p| format!("{}:{}", p.get_host(), p.get_guest()))
                .collect::<Vec<_>>()
                .join(", ");
            println!("   {}: {}", style("Ports").dim(), ports);
        }

        // 卷
        if !sandbox.get_volumes().is_empty() {
            let volumes = sandbox
                .get_volumes()
                .iter()
                .map(|v| format!("{}:{}", v.get_host(), v.get_guest()))
                .collect::<Vec<_>>()
                .join(", ");
            println!("   {}: {}", style("Volumes").dim(), volumes);
        }

        // 脚本
        if !sandbox.get_scripts().is_empty() {
            let scripts = sandbox
                .get_scripts()
                .keys()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            println!("   {}: {}", style("Scripts").dim(), scripts);
        }

        // 依赖
        if !sandbox.get_depends_on().is_empty() {
            println!(
                "   {}: {}",
                style("Depends On").dim(),
                sandbox.get_depends_on().join(", ")
            );
        }
    }

    // 显示总数
    println!("\n{}: {}", style("Total").dim(), sandboxes.len());
}

/// 显示跨多个项目的格式化沙箱列表。
///
/// # 功能说明
///
/// 此函数从所有项目中收集沙箱信息并以统一的格式显示。
/// 适用于服务器模式，可以查看所有项目的所有沙箱。
///
/// # 工作流程
///
/// 1. 读取项目父目录
/// 2. 列出所有项目子目录
/// 3. 预加载所有项目的配置文件（避免显示时的延迟）
/// 4. 按字母顺序排序项目
/// 5. 显示每个项目的沙箱列表
/// 6. 显示汇总统计
///
/// # 参数
///
/// * `projects_parent_dir` - 包含项目目录的父目录路径
///
/// # 错误处理
///
/// - 如果项目目录不存在，返回 `PathNotFound` 错误
/// - 如果某个项目的配置文件加载失败，显示错误信息但不中断
///
/// # 示例
///
/// ```no_run
/// use std::path::Path;
/// use microsandbox_core::management::menv;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 显示所有项目的所有沙箱
/// menv::show_list_projects(Path::new("/path/to/projects")).await?;
/// # Ok(())
/// # }
/// ```
#[cfg(feature = "cli")]
pub async fn show_list_projects(projects_parent_dir: &std::path::Path) -> MicrosandboxResult<()> {
    use crate::management::config;
    use console::style;
    use microsandbox_utils::term;
    use std::path::PathBuf;

    // 首先检查项目目录是否存在
    if !projects_parent_dir.exists() {
        return Err(MicrosandboxError::PathNotFound(format!(
            "Projects directory not found at {}",
            projects_parent_dir.display()
        )));
    }

    // 列出所有项目子目录
    let mut entries = tokio::fs::read_dir(projects_parent_dir).await?;
    let mut project_dirs = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.is_dir() {
            project_dirs.push(path);
        }
    }

    // 如果没有找到项目，显示提示
    if project_dirs.is_empty() {
        println!("No projects found");
        return Ok(());
    }

    // 按字母顺序排序项目目录
    project_dirs.sort_by(|a, b| {
        let a_name = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let b_name = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    // 创建加载进度提示
    let loading_sp = term::create_spinner(
        format!("Loading {} projects", project_dirs.len()),
        None,
        None,
    );

    // 预加载所有项目配置的数据结构
    struct ProjectData {
        name: String,
        config: Option<(crate::config::Microsandbox, PathBuf, String)>,
        error: Option<String>,
    }

    let mut project_data = Vec::with_capacity(project_dirs.len());

    // 收集所有项目数据
    for project_dir in &project_dirs {
        let project = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let config_result = config::load_config(Some(project_dir.as_path()), None).await;
        match config_result {
            Ok(config) => {
                project_data.push(ProjectData {
                    name: project,
                    config: Some(config),
                    error: None,
                });
            }
            Err(err) => {
                tracing::warn!("Error loading config from project {}: {}", project, err);
                project_data.push(ProjectData {
                    name: project,
                    config: None,
                    error: Some(format!("{}", err)),
                });
            }
        }
    }

    // 完成加载进度提示
    loading_sp.finish_and_clear();

    // 统计总数
    let project_count = project_dirs.len();
    let mut total_sandboxes = 0;

    // 显示所有项目数据
    for (i, data) in project_data.iter().enumerate() {
        // 在项目之间添加空行
        if i > 0 {
            println!();
        }

        if let Some((config, _, _)) = &data.config {
            // 统计该项目的沙箱数量
            let sandbox_count = config.get_sandboxes().len();
            total_sandboxes += sandbox_count;

            // 只有在有沙箱时才显示
            if sandbox_count > 0 {
                print_project_header(&data.name);
                show_list(config.get_sandboxes());
            }
        } else if let Some(err) = &data.error {
            print_project_header(&data.name);
            println!("  {}: {}", style("Error").red().bold(), err);
        }
    }

    // 显示汇总统计
    println!(
        "\n{}: {}, {}: {}",
        style("Total Projects").dim(),
        project_count,
        style("Total Sandboxes").dim(),
        total_sandboxes
    );

    Ok(())
}

/// 打印项目标题头。
///
/// # 输出格式
///
/// ```text
/// PROJECT: project_name
/// ────────────────────────────────────────────────────────────────────────
/// ```
#[cfg(feature = "cli")]
pub fn print_project_header(project: &str) {
    use console::style;

    // 创建标题文本
    let title = format!("PROJECT: {}", project);

    // 使用白色加粗样式打印标题
    println!("\n{}", style(title).white().bold());

    // 打印分隔线
    println!("{}", style("─".repeat(80)).dim());
}

//--------------------------------------------------------------------------------------------------
// 内部辅助函数
//--------------------------------------------------------------------------------------------------

/// 创建 Microsandbox 环境所需的目录和文件。
///
/// # 创建的内容
///
/// 1. **日志目录** (`log/`) - 存放沙箱运行日志
/// 2. **可写层目录** (`rw/`) - overlayfs 的可写层
/// 3. **沙箱数据库** (`sandbox.db`) - SQLite 数据库，记录沙箱状态
///
/// # 注意
///
/// - 不会创建 `rootfs/` 目录，该目录在 monofs 准备好时创建
/// - 数据库会自动初始化迁移
pub(crate) async fn ensure_menv_files(menv_path: &Path) -> MicrosandboxResult<()> {
    // 创建日志目录
    fs::create_dir_all(menv_path.join(LOG_SUBDIR)).await?;

    // 创建可写层目录（rootfs 稍后在 monofs 准备好时创建）
    fs::create_dir_all(menv_path.join(RW_SUBDIR)).await?;

    // 获取沙箱数据库路径
    let db_path = menv_path.join(SANDBOX_DB_FILENAME);

    // 初始化沙箱数据库（自动执行迁移）
    let _ = db::initialize(&db_path, &db::SANDBOX_DB_MIGRATOR).await?;
    tracing::info!("sandbox database at {}", db_path.display());

    Ok(())
}

/// 创建默认的 Microsandbox 配置文件。
///
/// # 实现细节
///
/// - 只在配置文件不存在时创建
/// - 使用 `DEFAULT_CONFIG` 常量中的预定义内容
/// - [CLI 功能] 显示创建进度提示
pub(crate) async fn create_default_config(project_dir: &Path) -> MicrosandboxResult<()> {
    let config_path = project_dir.join(MICROSANDBOX_CONFIG_FILENAME);

    // 只在配置文件不存在时创建
    if !config_path.exists() {
        #[cfg(feature = "cli")]
        let create_default_config_sp =
            term::create_spinner(CREATE_DEFAULT_CONFIG_MSG.to_string(), None, None);

        // 写入默认配置内容
        let mut file = fs::File::create(&config_path).await?;
        file.write_all(DEFAULT_CONFIG.as_bytes()).await?;

        #[cfg(feature = "cli")]
        create_default_config_sp.finish();
    }

    Ok(())
}

/// 更新或创建 `.gitignore` 文件，确保包含 `.menv` 目录条目。
///
/// # 实现细节
///
/// ## 如果 `.gitignore` 已存在
///
/// 1. 读取文件内容
/// 2. 检查是否已包含 `.menv` 或 `.menv/` 条目
/// 3. 如果不包含，追加新行：
///    - 确保以换行符开头（如果文件不是以换行结尾）
///    - 追加 `.menv/` 条目
///
/// ## 如果 `.gitignore` 不存在
///
/// 创建新文件，内容为 `.menv/\n`
///
/// # 为什么使用 `.menv/` 而不是 `.menv`？
///
/// - `.menv/` 明确表示只忽略目录，不忽略同名文件
/// - 这是 Git 的最佳实践
pub(crate) async fn update_gitignore(project_dir: &Path) -> MicrosandboxResult<()> {
    let gitignore_path = project_dir.join(".gitignore");
    // 使用标准格式：以斜杠结尾表示目录
    let canonical_entry = format!("{}/", MICROSANDBOX_ENV_DIR);
    // 可接受的条目格式（兼容两种写法）
    let acceptable_entries = [MICROSANDBOX_ENV_DIR, &canonical_entry[..]];

    if gitignore_path.exists() {
        let content = fs::read_to_string(&gitignore_path).await?;
        let already_present = content.lines().any(|line| {
            let trimmed = line.trim();
            acceptable_entries.contains(&trimmed)
        });

        // 如果条目不存在，追加到文件末尾
        if !already_present {
            // 确保从新行开始
            let prefix = if content.ends_with('\n') { "" } else { "\n" };
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(&gitignore_path)
                .await?;
            file.write_all(format!("{}{}\n", prefix, canonical_entry).as_bytes())
                .await?;
        }
    } else {
        // 创建新 .gitignore，使用标准格式（.menv/）
        fs::write(&gitignore_path, format!("{}\n", canonical_entry)).await?;
    }

    Ok(())
}
