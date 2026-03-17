//! Microsandbox 主目录管理模块
//!
//! 本模块提供了管理全局 microsandbox 主目录的功能，
//! 该目录包含缓存的镜像、层和数据库。
//! 它还包括清理主目录和检查其存在的功能。
//!
//! ## 主要功能
//!
//! - **主目录管理** - 创建和管理 microsandbox 主目录结构
//! - **沙箱安装** - 从 OCI 镜像安装沙箱并创建别名脚本
//! - **沙箱卸载** - 移除已安装的沙箱别名
//! - **清理功能** - 清理主目录中的所有数据
//!
//! ## 目录结构
//!
//! ```text
//! ~/.microsandbox/
//! ├── installs/          # 安装的沙箱配置
//! │   └── .menv/         # 沙箱数据库
//! ├── oci/               # OCI 镜像缓存
//! ├── layers/            # 镜像层缓存
//! └── *.db               # 数据库文件
//! ```

use crate::{
    MicrosandboxError, MicrosandboxResult,
    config::{EnvPair, Microsandbox, PathPair, PortPair, ReferenceOrPath, Sandbox},
    management::{config, db, menv},
    oci::{Image, Reference},
};
use microsandbox_utils::{
    MICROSANDBOX_CONFIG_FILENAME, MICROSANDBOX_HOME_DIR, OCI_DB_FILENAME, XDG_BIN_DIR,
    XDG_HOME_DIR, env, path::INSTALLS_SUBDIR,
};

#[cfg(feature = "cli")]
use microsandbox_utils::term;
use std::os::unix::fs::PermissionsExt;
use tokio::fs;
use typed_path::Utf8UnixPathBuf;

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

#[cfg(feature = "cli")]
const REMOVE_HOME_DIR_MSG: &str = "Remove microsandbox home";

#[cfg(feature = "cli")]
const INSTALL_SANDBOX_MSG: &str = "Install sandbox";

//--------------------------------------------------------------------------------------------------
// 函数实现
//--------------------------------------------------------------------------------------------------

/// 清理全局 microsandbox 主目录
///
/// 此函数移除整个 microsandbox 主目录及其所有内容，
/// 有效清理所有全局 microsandbox 数据，包括缓存的镜像、层和数据库。
///
/// ## 参数
/// * `force` - 是否强制清理，即使存在配置文件
///
/// ## 示例
/// ```no_run
/// use microsandbox_core::management::home;
///
/// # async fn example() -> anyhow::Result<()> {
/// // force = true 时无条件删除所有内容
/// home::clean(true).await?;
/// # Ok(())
/// # }
/// ```
///
/// ## 处理逻辑
/// 1. 获取主目录路径
/// 2. 检查是否存在配置文件
/// 3. 如果存在配置文件且未强制，则跳过清理
/// 4. 如果强制或无配置文件，删除整个目录
pub async fn clean(force: bool) -> MicrosandboxResult<()> {
    // 从环境变量或默认值获取 microsandbox 主目录路径
    let home_path = env::get_microsandbox_home_path();
    let installs_path = home_path.join(INSTALLS_SUBDIR);

    #[cfg(feature = "cli")]
    let remove_home_dir_sp = term::create_spinner(REMOVE_HOME_DIR_MSG.to_string(), None, None);

    // 检查 installs 目录是否存在且有配置文件
    if installs_path.exists() {
        let config_path = installs_path.join(MICROSANDBOX_CONFIG_FILENAME);

        // 如果配置文件存在且未强制，不清理
        if config_path.exists() && !force {
            #[cfg(feature = "cli")]
            term::finish_with_error(&remove_home_dir_sp);

            #[cfg(feature = "cli")]
            println!(
                "Configuration file exists at {}. Use {} to clean the home directory",
                console::style(config_path.display()).yellow(),
                console::style("--force").yellow()
            );

            tracing::warn!(
                "Configuration file exists at {}. Use force=true to clean the home directory",
                config_path.display()
            );
            return Ok(());
        }
    }

    // 检查主目录是否存在
    if home_path.exists() {
        // 删除主目录及其所有内容
        fs::remove_dir_all(&home_path).await?;
        tracing::info!(
            "Removed microsandbox home directory at {}",
            home_path.display()
        );
    } else {
        tracing::info!(
            "No microsandbox home directory found at {}",
            home_path.display()
        );
    }

    #[cfg(feature = "cli")]
    remove_home_dir_sp.finish();

    Ok(())
}

/// 从镜像安装沙箱并为其创建别名脚本
///
/// 此函数在全局 microsandbox 主目录中创建永久的沙箱配置，
/// 并设置一个别名脚本，可用于运行沙箱。
///
/// ## 参数
/// * `image` - 用作沙箱基础的 OCI 镜像引用
/// * `script` - 要在沙箱中执行的脚本名称
/// * `alias` - 用于脚本的别名名称，如果未提供则使用脚本名称
/// * `cpus` - 可选的分配给沙箱的虚拟 CPU 数量
/// * `memory` - 可选的分配给沙箱的内存大小（MiB）
/// * `volumes` - 卷映射列表，格式为 "host_path:guest_path"
/// * `ports` - 端口映射列表，格式为 "host_port:guest_port"
/// * `envs` - 环境变量列表，格式为 "KEY=VALUE"
/// * `workdir` - 可选的沙箱内工作目录路径
/// * `scope` - 可选的沙箱网络范围
/// * `exec` - 可选的在沙箱中执行的命令
/// * `args` - 要传递给命令的额外参数
/// * `use_image_defaults` - 是否应用 OCI 镜像配置的默认设置
///
/// ## 返回值
/// 如果沙箱安装成功返回 `Ok(())`，否则返回 `MicrosandboxError`：
/// - 镜像无法拉取或找不到
/// - 沙箱配置无效
/// - 文件系统操作失败
/// - 与现有系统命令名称冲突
///
/// ## 示例
/// ```no_run
/// use microsandbox_core::oci::Reference;
/// use microsandbox_core::management::home;
/// use typed_path::Utf8UnixPathBuf;
///
/// # async fn example() -> anyhow::Result<()> {
/// let image = "ubuntu:latest".parse::<Reference>()?;
///
/// // 安装带有自定义名称和资源的 Ubuntu 沙箱
/// home::install(
///     &image,
///     Some("shell"),          // 运行 shell 脚本
///     Some("ubuntu-shell"),   // 自定义别名
///     Some(2),                // 2 个 CPU
///     Some(1024),             // 1GB 内存
///     vec![                   // 将主机的 /tmp 挂载到沙箱的 /data
///         "/tmp:/data".to_string()
///     ],
///     vec![                   // 将主机端口 8080 映射到沙箱端口 80
///         "8080:80".to_string()
///     ],
///     vec![                   // 设置环境变量
///         "DEBUG=1".to_string()
///     ],
///     Some("/app".into()),    // 设置工作目录
///     Some("local".to_string()), // 设置网络范围
///     None,                   // 无 exec 命令
///     vec![],                 // 无额外参数
///     true                    // 使用镜像默认值
/// ).await?;
/// # Ok(())
/// # }
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn install(
    image: &Reference,
    script: Option<&str>,
    alias: Option<&str>,
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
    // 获取 microsandbox 主目录路径
    let home_path = env::get_microsandbox_home_path();
    let installs_path = home_path.join(INSTALLS_SUBDIR);

    // 确定要使用的别名名称：
    // 1. 如果指定则使用提供的别名
    // 2. 如果提供了则使用脚本名称
    // 3. 否则从镜像引用中提取名称
    let alias_name = alias
        .map(|a| a.to_string())
        .or_else(|| script.map(|s| s.to_string()))
        .unwrap_or_else(|| extract_name_from_reference(image));

    tracing::info!("Setting up alias: {}", alias_name);

    // 检查系统 PATH 中是否已存在同名命令
    if command_exists(&alias_name) {
        return Err(MicrosandboxError::CommandExists(alias_name));
    }

    // 在 installs 目录中初始化 .menv（如果不存在）
    // 这会创建必要的目录和沙箱数据库
    menv::initialize(Some(installs_path.clone())).await?;

    // 解析 volume、port 和 env 字符串为各自的类型
    let volumes: Vec<PathPair> = volumes.into_iter().filter_map(|v| v.parse().ok()).collect();
    let ports: Vec<PortPair> = ports.into_iter().filter_map(|p| p.parse().ok()).collect();
    let envs: Vec<EnvPair> = envs.into_iter().filter_map(|e| e.parse().ok()).collect();

    // 构建沙箱配置
    let mut sandbox = {
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

    // 如果启用了镜像配置默认值则应用
    if use_image_defaults {
        // 如果镜像尚未拉取，则从注册表拉取
        Image::pull(image.clone(), None).await?;

        // 获取 OCI 数据库路径并创建连接池
        let db_path = home_path.join(OCI_DB_FILENAME);
        let oci_pool = db::get_or_create_pool(&db_path, &db::OCI_DB_MIGRATOR).await?;

        // 将镜像默认值应用到沙箱配置
        config::apply_image_defaults(&mut sandbox, image, &oci_pool).await?;
        tracing::debug!("applied image defaults to sandbox config");
    }

    // 为 CLI 反馈创建 spinner
    #[cfg(feature = "cli")]
    let install_sandbox_sp = term::create_spinner(
        format!("{} from '{}'", INSTALL_SANDBOX_MSG, image),
        None,
        None,
    );

    // 如果提供了则覆盖 exec 命令
    if let Some(exec) = exec {
        let mut command = Vec::with_capacity(args.len() + 1);
        command.push(exec.to_string());
        command.extend(args);
        sandbox.set_command(command);
    }

    // 创建带有沙箱的 microsandbox 配置
    let config = Microsandbox::builder()
        .sandboxes([(alias_name.clone(), sandbox)])
        .build_unchecked();

    // 将配置写入 installs 目录
    let config_path = installs_path.join(MICROSANDBOX_CONFIG_FILENAME);
    fs::write(&config_path, serde_yaml::to_string(&config)?).await?;
    tracing::info!("Wrote config to {}", config_path.display());

    // 在 ~/.local/bin 中创建别名脚本
    let bin_dir = XDG_HOME_DIR.join(XDG_BIN_DIR);

    // 如果 bin 目录不存在则创建
    fs::create_dir_all(&bin_dir).await?;

    let script_path = bin_dir.join(&alias_name);
    let script_content = generate_alias_script(&alias_name, script);

    // 写入脚本文件
    fs::write(&script_path, script_content).await?;

    // 使脚本可执行
    let mut perms = std::fs::metadata(&script_path)?.permissions();
    perms.set_mode(0o755); // rwxr-xr-x
    std::fs::set_permissions(&script_path, perms)?;

    tracing::info!("Created alias script at {}", script_path.display());

    #[cfg(feature = "cli")]
    install_sandbox_sp.finish();

    Ok(())
}

/// 从本地 bin 目录卸载脚本别名
///
/// 此函数移除之前使用 `install` 安装的脚本别名。
/// 它只移除包含 "MSB-ALIAS" 标记的脚本，以确保不会删除无关文件。
///
/// ## 参数
/// * `script_name` - 要卸载的脚本名称。这应该与别名名称匹配。
///
/// ## 返回值
/// 如果脚本成功卸载返回 `Ok(())`，否则返回 `MicrosandboxError`：
/// - 脚本在 bin 目录中不存在
/// - 脚本不包含 MSB-ALIAS 标记
/// - 文件系统操作失败
///
/// ## 示例
/// ```no_run
/// use microsandbox_core::management::home;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 卸载 "ubuntu-shell" 脚本
/// home::uninstall("ubuntu-shell").await?;
/// # Ok(())
/// # }
/// ```
pub async fn uninstall(script_name: &str) -> MicrosandboxResult<()> {
    // 获取 bin 目录路径
    let bin_dir = XDG_HOME_DIR.join(XDG_BIN_DIR);
    let script_path = bin_dir.join(script_name);

    // 检查脚本是否存在
    if !script_path.exists() {
        return Err(MicrosandboxError::PathNotFound(format!(
            "Script '{}' not found at {}",
            script_name,
            script_path.display()
        )));
    }

    // 读取脚本文件内容
    let script_content = fs::read_to_string(&script_path).await?;

    // 检查是否是 microsandbox 别名脚本（包含 MSB-ALIAS 标记）
    if !script_content.contains("# MSB-ALIAS:") {
        return Err(MicrosandboxError::InvalidArgument(format!(
            "Script '{}' is not a microsandbox alias (missing MSB-ALIAS marker)",
            script_name
        )));
    }

    // 从脚本中提取别名名称进行验证
    let alias_marker = format!("# MSB-ALIAS: {}", script_name);
    if !script_content.contains(&alias_marker) {
        tracing::warn!(
            "Script '{}' has a different alias name in its marker. Continuing with uninstall.",
            script_name
        );
    }

    // 所有检查通过，删除脚本
    fs::remove_file(&script_path).await?;
    tracing::info!("Removed alias script: {}", script_path.display());

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// 检查给定名称的命令是否存在于系统 PATH 中
///
/// 此函数使用 `which` 命令检查命令是否存在于
/// PATH 环境变量列出的任何目录中。
///
/// ## 参数
/// * `command` - 要检查的命令名称
///
/// ## 返回值
/// 如果命令存在于 PATH 中返回 `true`，否则返回 `false`
fn command_exists(command: &str) -> bool {
    use std::process::Command;

    Command::new("which")
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// 从 OCI 镜像引用中提取简单名称
///
/// 例如：
/// - "docker.io/library/ubuntu:latest" -> "ubuntu"
/// - "registry.com/org/app:v1.0" -> "app"
/// - "myapp:stable" -> "myapp"
fn extract_name_from_reference(reference: &Reference) -> String {
    let image_str = reference.to_string();

    // 通过 '/' 分割镜像字符串并取最后一部分
    let name_with_tag = image_str.rsplit('/').next().unwrap_or(&image_str);

    // 通过 ':' 分割以移除标签并取第一部分
    name_with_tag
        .split(':')
        .next()
        .unwrap_or(name_with_tag)
        .to_string()
}

/// 根据别名名称和可选脚本生成别名脚本内容
fn generate_alias_script(alias: &str, script: Option<&str>) -> String {
    let run_command = if let Some(script_name) = script {
        format!(
            "exec \"$MSB_PATH\" run \"{}~{}\" -f \"$HOME/{}\" \"$@\"",
            alias,
            script_name,
            MICROSANDBOX_HOME_DIR.to_string() + "/" + INSTALLS_SUBDIR
        )
    } else {
        format!(
            "exec \"$MSB_PATH\" run \"{}\" -f \"$HOME/{}\" \"$@\"",
            alias,
            MICROSANDBOX_HOME_DIR.to_string() + "/" + INSTALLS_SUBDIR
        )
    };

    format!(
        r#"#!/bin/sh
# MSB-ALIAS: {}
# Alias for 'msb run {}{}' from installed sandbox

# Find the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# Assuming msb is in the same directory as this script
if [ -x "$SCRIPT_DIR/msb" ]; then
  MSB_PATH="$SCRIPT_DIR/msb"
else
  # Otherwise, rely on PATH
  MSB_PATH="msb"
fi

{}
"#,
        alias,
        alias,
        script.map(|s| format!("~{}", s)).unwrap_or_default(),
        run_command
    )
}
