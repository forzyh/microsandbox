//! Microsandbox 运行时配置管理模块
//!
//! 本模块提供了用于修改 Microsandbox 配置的结构和工具。
//! 支持添加、删除和列出配置组件，同时保留原有 YAML 格式。
//!
//! ## 主要功能
//!
//! - **配置加载** - 从 YAML 文件加载 Microsandbox 配置
//! - **组件添加** - 向配置文件添加新的沙箱组件
//! - **组件删除** - 从配置文件移除现有组件
//! - **组件列表** - 列出配置中定义的组件
//! - **镜像默认值** - 从 OCI 镜像配置应用默认值
//!
//! ## 配置组件类型
//!
//! - **Sandbox** - 沙箱配置，包含镜像、资源、挂载等
//! - **Build** - 构建任务配置（待实现）
//! - **Group** - 沙箱组配置（待实现）

use microsandbox_utils::{DEFAULT_SHELL, MICROSANDBOX_CONFIG_FILENAME};
use nondestructive::yaml;
use sqlx::{Pool, Sqlite};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::fs;
use typed_path::Utf8UnixPathBuf;

use crate::{
    MicrosandboxError, MicrosandboxResult,
    config::{EnvPair, Microsandbox, PathSegment, PortPair, Sandbox},
    oci::Reference,
};

use super::db;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// 沙箱组件配置
///
/// 此结构包含了定义一个沙箱所需的所有配置项。
/// 用于 `add` 函数中指定要添加的沙箱配置。
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// 沙箱使用的镜像（OCI 引用或本地路径）
    pub image: String,

    /// 使用的内存大小（MiB）
    pub memory: Option<u32>,

    /// 使用的 CPU 核心数
    pub cpus: Option<u32>,

    /// 要挂载的卷列表（格式："host_path:guest_path"）
    pub volumes: Vec<String>,

    /// 要暴露的端口列表（格式："host_port:guest_port"）
    pub ports: Vec<String>,

    /// 要设置的环境变量列表（格式："KEY=VALUE"）
    pub envs: Vec<String>,

    /// 环境变量文件路径
    pub env_file: Option<Utf8UnixPathBuf>,

    /// 沙箱依赖的其他沙箱名称列表
    pub depends_on: Vec<String>,

    /// 沙箱内的工作目录路径
    pub workdir: Option<Utf8UnixPathBuf>,

    /// 使用的 shell 路径
    pub shell: Option<String>,

    /// 沙箱中可用的脚本映射（名称 -> 内容）
    pub scripts: HashMap<String, String>,

    /// 要导入的文件映射（名称 -> 路径）
    pub imports: HashMap<String, Utf8UnixPathBuf>,

    /// 要导出的文件映射（名称 -> 路径）
    pub exports: HashMap<String, Utf8UnixPathBuf>,

    /// 网络范围配置
    pub scope: Option<String>,
}

/// 要添加到 Microsandbox 配置的组件
///
/// 此枚举定义了可以添加到配置的组件类型。
/// 目前仅支持 Sandbox 组件，Build 和 Group 为占位符。
#[derive(Debug, Clone)]
pub enum Component {
    /// 沙箱组件
    Sandbox(Box<SandboxConfig>),
    /// 构建任务组件（待实现）
    Build {},
    /// 沙箱组组件（待实现）
    Group {},
}

/// 要添加到 Microsandbox 配置的组件类型
///
/// 用于 `list` 和 `remove` 函数中指定操作的组件类型。
#[derive(Debug, Clone)]
pub enum ComponentType {
    /// 沙箱组件
    Sandbox,
    /// 构建任务组件
    Build,
    /// 沙箱组组件
    Group,
}

//--------------------------------------------------------------------------------------------------
// 函数实现
//--------------------------------------------------------------------------------------------------

/// 向 Microsandbox 配置添加一个或多个组件
///
/// 此函数通过添加新组件来修改 Microsandbox 配置文件，
/// 同时保留现有的格式和结构。使用非破坏性 YAML 解析
/// 来保持原有注释和格式。
///
/// ## 参数
/// * `names` - 要添加的组件名称列表
/// * `component` - 要添加的组件规格
/// * `project_dir` - 可选的项目目录路径（默认为当前目录）
/// * `config_file` - 可选的配置文件路径（默认为标准文件名）
///
/// ## 返回值
/// * `Ok(())` - 成功
/// * `Err(MicrosandboxError)` - 文件无法找到/读取/写入，
///   包含无效 YAML，或已存在同名组件
///
/// ## 处理流程
/// 1. 解析配置文件路径
/// 2. 读取 YAML 内容
/// 3. 使用非破坏性方式解析 YAML
/// 4. 为每个名称创建沙箱配置
/// 5. 将修改后的 YAML 写回文件
pub async fn add(
    names: &[String],
    component: &Component,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<()> {
    let (_, _, full_config_path) = resolve_config_paths(project_dir, config_file).await?;

    // 读取配置文件内容
    let config_contents = fs::read_to_string(&full_config_path).await?;

    // 使用非破坏性方式解析 YAML 文档
    let mut doc = yaml::from_slice(config_contents.as_bytes())
        .map_err(|e| MicrosandboxError::ConfigParseError(e.to_string()))?;

    for name in names {
        match &component {
            Component::Sandbox(config) => {
                let doc_mut = doc.as_mut();
                let mut root_mapping = doc_mut.make_mapping();

                // 确保 "sandboxes" 键存在于根映射中
                let mut sandboxes_mapping =
                    if let Some(sandboxes_mut) = root_mapping.get_mut("sandboxes") {
                        // 获取现有的 sandboxes 映射
                        sandboxes_mut.make_mapping()
                    } else {
                        // 如果不存在则创建新的 sandboxes 映射
                        root_mapping
                            .insert("sandboxes", yaml::Separator::Auto)
                            .make_mapping()
                    };

                // 通过尝试获取来检查沙箱是否已存在
                if sandboxes_mapping.get_mut(name).is_some() {
                    return Err(MicrosandboxError::ConfigValidation(format!(
                        "Sandbox with name '{}' already exists",
                        name
                    )));
                }

                // 创建新的沙箱映射
                let mut sandbox_mapping = sandboxes_mapping
                    .insert(name, yaml::Separator::Auto)
                    .make_mapping();

                // 添加 image 字段（必填）
                sandbox_mapping.insert_str("image", &config.image);

                // 添加可选字段
                if let Some(memory_value) = config.memory {
                    sandbox_mapping.insert_u32("memory", memory_value);
                }

                if let Some(cpus_value) = config.cpus {
                    sandbox_mapping.insert_u32("cpus", cpus_value);
                }

                // 添加 shell（如果未提供则使用默认值）
                if let Some(shell_value) = &config.shell {
                    sandbox_mapping.insert_str("shell", shell_value);
                } else if sandbox_mapping.get_mut("shell").is_none() {
                    sandbox_mapping.insert_str("shell", DEFAULT_SHELL);
                }

                // 添加 volumes（如果有）
                if !config.volumes.is_empty() {
                    let mut volumes_sequence = sandbox_mapping
                        .insert("volumes", yaml::Separator::Auto)
                        .make_sequence();

                    for volume in &config.volumes {
                        volumes_sequence.push_string(volume);
                    }
                }

                // 添加 ports（如果有）
                if !config.ports.is_empty() {
                    let mut ports_sequence = sandbox_mapping
                        .insert("ports", yaml::Separator::Auto)
                        .make_sequence();

                    for port in &config.ports {
                        ports_sequence.push_string(port);
                    }
                }

                // 添加 env vars（如果有）
                if !config.envs.is_empty() {
                    let mut envs_sequence = sandbox_mapping
                        .insert("envs", yaml::Separator::Auto)
                        .make_sequence();

                    for env in &config.envs {
                        envs_sequence.push_string(env);
                    }
                }

                // 如果提供了则添加 env_file
                if let Some(env_file_path) = &config.env_file {
                    sandbox_mapping.insert_str("env_file", env_file_path);
                }

                // 添加 depends_on（如果有）
                if !config.depends_on.is_empty() {
                    let mut depends_on_sequence = sandbox_mapping
                        .insert("depends_on", yaml::Separator::Auto)
                        .make_sequence();

                    for dep in &config.depends_on {
                        depends_on_sequence.push_string(dep);
                    }
                }

                // 如果提供了则添加 workdir
                if let Some(workdir_path) = &config.workdir {
                    sandbox_mapping.insert_str("workdir", workdir_path);
                }

                // 添加 scripts（如果有）
                if !config.scripts.is_empty() {
                    let mut scripts_mapping = sandbox_mapping
                        .insert("scripts", yaml::Separator::Auto)
                        .make_mapping();

                    for (script_name, script_content) in &config.scripts {
                        scripts_mapping.insert_str(script_name, script_content);
                    }
                }

                // 添加 imports（如果有）
                if !config.imports.is_empty() {
                    let mut imports_mapping = sandbox_mapping
                        .insert("imports", yaml::Separator::Auto)
                        .make_mapping();

                    for (import_name, import_path) in &config.imports {
                        imports_mapping.insert_str(import_name, import_path);
                    }
                }

                // 添加 exports（如果有）
                if !config.exports.is_empty() {
                    let mut exports_mapping = sandbox_mapping
                        .insert("exports", yaml::Separator::Auto)
                        .make_mapping();

                    for (export_name, export_path) in &config.exports {
                        exports_mapping.insert_str(export_name, export_path);
                    }
                }

                // 如果提供了则添加 network scope
                if let Some(scope_value) = &config.scope {
                    let mut network_mapping = sandbox_mapping
                        .insert("network", yaml::Separator::Auto)
                        .make_mapping();

                    network_mapping.insert_str("scope", scope_value);
                }
            }
            Component::Build {} => {}
            Component::Group {} => {}
        }
    }

    // 将修改后的 YAML 写回文件，保留格式
    let modified_content = doc.to_string();

    // TODO: 在写入前验证配置
    fs::write(full_config_path, modified_content).await?;

    Ok(())
}

/// 从 Microsandbox 配置中删除组件
///
/// 此函数通过删除现有组件来修改 Microsandbox 配置文件，
/// 同时保留现有的格式和结构。
///
/// ## 参数
/// * `component_type` - 要从配置中删除的组件类型
/// * `names` - 要删除的组件名称列表
/// * `project_dir` - 可选的项目目录路径
/// * `config_file` - 可选的配置文件路径
///
/// ## 返回值
/// * `Ok(())` - 成功
/// * `Err(MicrosandboxError)` - 文件无法找到/读取/写入，
///   包含无效 YAML，或组件不存在
///
/// 注意：此函数目前是占位符，需要完整实现。
pub async fn remove(
    component_type: ComponentType,
    names: &[String],
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<()> {
    let (_, _, full_config_path) = resolve_config_paths(project_dir, config_file).await?;

    // 读取配置文件内容
    let config_contents = fs::read_to_string(&full_config_path).await?;

    let mut doc = yaml::from_slice(config_contents.as_bytes())
        .map_err(|e| MicrosandboxError::ConfigParseError(e.to_string()))?;

    if let ComponentType::Sandbox = component_type {
        let doc_mut = doc.as_mut();
        let mut root_mapping =
            doc_mut
                .into_mapping_mut()
                .ok_or(MicrosandboxError::ConfigParseError(
                    "config is not valid. expected an object".to_string(),
                ))?;

        // 确保 "sandboxes" 键存在于根映射中
        let mut sandboxes_mapping = if let Some(sandboxes_mut) = root_mapping.get_mut("sandboxes") {
            // 获取现有的 sandboxes 映射
            sandboxes_mut
                .into_mapping_mut()
                .ok_or(MicrosandboxError::ConfigParseError(
                    "sandboxes is not a valid mapping".to_string(),
                ))?
        } else {
            // 如果不存在则创建新的 sandboxes 映射
            root_mapping
                .insert("sandboxes", yaml::Separator::Auto)
                .make_mapping()
        };

        for name in names {
            sandboxes_mapping.remove(name);
        }
    }

    // 将修改后的 YAML 写回文件，保留格式
    let modified_content = doc.to_string();

    // TODO: 在写入前验证配置
    fs::write(full_config_path, modified_content).await?;

    Ok(())
}

/// 列出 Microsandbox 配置中的组件
///
/// 检索并显示 Microsandbox 配置中定义的组件信息。
///
/// ## 参数
/// * `component_type` - 要列出的组件类型
/// * `project_dir` - 可选的项目目录路径（默认为当前目录）
/// * `config_file` - 可选的配置文件路径（默认为标准文件名）
///
/// ## 返回值
/// * `Ok(Vec<String>)` - 组件名称列表
/// * `Err(MicrosandboxError)` - 文件无法找到/读取/写入，
///   包含无效 YAML，或组件不存在
pub async fn list(
    component_type: ComponentType,
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<Vec<String>> {
    let (config, _, _) = load_config(project_dir, config_file).await?;

    if let ComponentType::Sandbox = component_type {
        Ok(config.get_sandboxes().keys().cloned().collect())
    } else {
        Ok(vec![])
    }
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// 从文件加载 Microsandbox 配置
///
/// 此函数处理加载 Microsandbox 配置的所有常见步骤，包括：
/// - 解析项目目录和配置文件路径
/// - 验证配置文件路径
/// - 检查配置文件是否存在
/// - 读取和解析配置文件
///
/// ## 参数
/// * `project_dir` - 可选的项目目录路径（默认为当前目录）
/// * `config_file` - 可选的配置文件路径（默认为标准文件名）
///
/// ## 返回值
/// 返回包含以下内容的元组：
/// - 加载的 Microsandbox 配置
/// - 规范化的项目目录路径
/// - 配置文件名称
///
/// 或 `MicrosandboxError` 如果：
/// - 配置文件路径无效
/// - 配置文件不存在
/// - 配置文件无法读取
/// - 配置文件包含无效 YAML
pub async fn load_config(
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<(Microsandbox, PathBuf, String)> {
    // 获取目标路径，如果未指定则默认为当前目录
    let project_dir = project_dir.unwrap_or_else(|| Path::new("."));
    let canonical_project_dir = fs::canonicalize(project_dir).await?;

    // 验证配置文件路径
    let config_file = config_file.unwrap_or(MICROSANDBOX_CONFIG_FILENAME);
    let _ = PathSegment::try_from(config_file)?;
    let full_config_path = canonical_project_dir.join(config_file);

    // 检查配置文件是否存在
    if !full_config_path.exists() {
        return Err(MicrosandboxError::MicrosandboxConfigNotFound(
            project_dir.display().to_string(),
        ));
    }

    // 读取和解析配置文件
    let config_contents = fs::read_to_string(&full_config_path).await?;
    let config: Microsandbox = serde_yaml::from_str(&config_contents)?;

    Ok((config, canonical_project_dir, config_file.to_string()))
}

/// 解析 Microsandbox 配置的路径
///
/// 此函数类似于 `load_config`，但不实际加载文件。
/// 它只解析将使用的路径。
///
/// ## 参数
/// * `project_dir` - 可选的项目目录路径（默认为当前目录）
/// * `config_file` - 可选的配置文件路径（默认为标准文件名）
///
/// ## 返回值
/// 返回包含以下内容的元组：
/// - 规范化的项目目录路径
/// - 配置文件名称
/// - 完整的配置文件路径
pub async fn resolve_config_paths(
    project_dir: Option<&Path>,
    config_file: Option<&str>,
) -> MicrosandboxResult<(PathBuf, String, PathBuf)> {
    // 获取目标路径，如果未指定则默认为当前目录
    let project_dir = project_dir.unwrap_or_else(|| Path::new("."));
    let canonical_project_dir = fs::canonicalize(project_dir).await?;

    // 验证配置文件路径
    let config_file = config_file.unwrap_or(MICROSANDBOX_CONFIG_FILENAME);
    let _ = PathSegment::try_from(config_file)?;
    let full_config_path = canonical_project_dir.join(config_file);

    // 检查配置文件是否存在
    if !full_config_path.exists() {
        return Err(MicrosandboxError::MicrosandboxConfigNotFound(
            project_dir.display().to_string(),
        ));
    }

    Ok((
        canonical_project_dir,
        config_file.to_string(),
        full_config_path,
    ))
}

/// 将 OCI 镜像配置的默认值应用到沙箱配置
///
/// 当沙箱配置中未显式定义时，此函数使用 OCI 镜像配置中的默认值
/// 来增强沙箱配置。
///
/// 应用以下默认值：
/// - **脚本** - 如果缺少脚本，使用镜像的 entrypoint 和 cmd
/// - **环境变量** - 将镜像环境变量与沙箱环境变量合并
/// - **工作目录** - 如果未指定，使用镜像的工作目录
/// - **暴露端口** - 将镜像暴露的端口与沙箱端口合并
///
/// ## 参数
/// * `sandbox_config` - 要增强的沙箱配置的可变引用
/// * `reference` - OCI 镜像引用
/// * `oci_db` - OCI 数据库连接池
///
/// ## 返回值
/// 如果成功应用默认值返回 `Ok(())`，或 `MicrosandboxError` 如果：
/// - 无法检索镜像配置
/// - 任何转换或解析操作失败
pub async fn apply_image_defaults(
    sandbox_config: &mut Sandbox,
    reference: &Reference,
    oci_db: &Pool<Sqlite>,
) -> MicrosandboxResult<()> {
    // 获取镜像配置
    if let Some(config) = db::get_image_config(oci_db, &reference.to_string()).await? {
        tracing::info!("applying defaults from image configuration");

        // 如果沙箱中未设置，应用工作目录
        if sandbox_config.get_workdir().is_none()
            && let Some(workdir) = config.config_working_dir
        {
            tracing::debug!("using image working directory: {}", workdir);
            let workdir_path = Utf8UnixPathBuf::from(workdir);
            sandbox_config.workdir = Some(workdir_path);
        }

        // 合并环境变量
        if let Some(config_env_json) = config.config_env_json
            && let Ok(image_env_vars) = serde_json::from_str::<Vec<String>>(&config_env_json)
        {
            let mut image_env_pairs = Vec::new();
            for env_var in image_env_vars {
                if let Ok(env_pair) = env_var.parse::<EnvPair>() {
                    image_env_pairs.push(env_pair);
                }
            }
            tracing::debug!("image env vars: {:#?}", image_env_pairs);

            // 将镜像环境变量与沙箱环境变量合并（镜像变量在前）
            let mut combined_env = image_env_pairs;
            combined_env.extend_from_slice(sandbox_config.get_envs());
            sandbox_config.envs = combined_env;
        }

        // 如果未定义命令，应用 entrypoint 和 cmd 作为命令
        if sandbox_config.get_command().is_empty() {
            let mut command_vec: Vec<String> = Vec::new();
            let mut has_entrypoint_or_cmd = false;

            // 尝试使用镜像配置中的 entrypoint 和 cmd
            if let Some(entrypoint_json) = &config.config_entrypoint_json
                && let Ok(entrypoint) = serde_json::from_str::<Vec<String>>(entrypoint_json)
                && !entrypoint.is_empty()
            {
                has_entrypoint_or_cmd = true;
                command_vec = entrypoint;

                // 如果存在则添加 CMD 参数
                if let Some(cmd_json) = &config.config_cmd_json
                    && let Ok(cmd) = serde_json::from_str::<Vec<String>>(cmd_json)
                    && !cmd.is_empty()
                {
                    command_vec.extend(cmd);
                }

                tracing::debug!("entrypoint exec content: {:?}", command_vec);
            } else if let Some(cmd_json) = &config.config_cmd_json
                && let Ok(cmd) = serde_json::from_str::<Vec<String>>(cmd_json)
                && !cmd.is_empty()
            {
                has_entrypoint_or_cmd = true;
                command_vec = cmd;
                tracing::debug!("cmd exec content: {:?}", command_vec);
            }

            // 如果找到了 entrypoint 或 cmd，将其设置为命令
            if has_entrypoint_or_cmd {
                tracing::debug!("setting command to: {:?}", command_vec);
                sandbox_config.command = command_vec;
            } else if let Some(shell_value) = &sandbox_config.shell {
                // 如果没有 entrypoint 或 cmd，使用 shell 作为后备命令
                tracing::debug!("using shell as fallback command");
                sandbox_config.command = vec![shell_value.clone()];
            }
        }

        // 合并暴露的端口
        if let Some(exposed_ports_json) = &config.config_exposed_ports_json
            && let Ok(exposed_ports_map) =
                serde_json::from_str::<serde_json::Value>(exposed_ports_json)
            && let Some(exposed_ports_obj) = exposed_ports_map.as_object()
        {
            let mut additional_ports = Vec::new();

            for port_key in exposed_ports_obj.keys() {
                // OCI 格式中的端口键如 "80/tcp"
                if let Some(container_port) = port_key.split('/').next()
                    && let Ok(port_num) = container_port.parse::<u16>()
                {
                    // 创建从主机端口到容器端口的端口映射
                    // 我们使用两侧相同的端口
                    let port_pair = format!("{}:{}", port_num, port_num).parse::<PortPair>();
                    if let Ok(port_pair) = port_pair {
                        // 仅当沙箱配置中未定义时才添加
                        let existing_ports = sandbox_config.get_ports();
                        if !existing_ports
                            .iter()
                            .any(|p| p.get_guest() == port_pair.get_guest())
                        {
                            additional_ports.push(port_pair);
                        }
                    }
                }
            }

            tracing::debug!("additional ports: {:?}", additional_ports);

            // 将新端口添加到现有端口
            let mut combined_ports = sandbox_config.get_ports().to_vec();
            combined_ports.extend(additional_ports);
            sandbox_config.ports = combined_ports;
        }
    }

    Ok(())
}
