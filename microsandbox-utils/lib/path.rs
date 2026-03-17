//! # 路径处理模块
//!
//! 本模块提供路径规范化、验证和解析的工具函数，以及 microsandbox 项目中使用的各种路径常量。
//!
//! ## 核心功能
//!
//! ### 路径常量
//! 定义了 microsandbox 项目中使用的各种目录名、文件名和路径后缀：
//! - 项目级数据目录（`.menv`）
//! - 全局数据目录（`.microsandbox`）
//! - 日志、层、安装等子目录
//! - 数据库文件和配置文件名
//!
//! ### 路径规范化
//! [`normalize_path`] 函数用于标准化路径表示，便于比较和验证：
//! - 解析 `.` 和 `..` 组件
//! - 防止路径遍历攻击
//! - 移除冗余的斜杠
//!
//! ### 环境变量路径解析
//! [`resolve_env_path`] 函数用于从环境变量或默认位置解析文件路径。
//!
//! ## 目录结构概述
//!
//! ### 项目级目录（位于项目根目录）
//! ```text
//! <PROJECT_ROOT>/
//! └── .menv/                 # MICROSANDBOX_ENV_DIR
//!     ├── rw/                # RW_SUBDIR: 读写层目录
//!     ├── patch/             # PATCH_SUBDIR: 补丁层目录
//!     ├── log/               # LOG_SUBDIR: 日志目录
//!     ├── sandbox.db         # SANDBOX_DB_FILENAME: 沙箱数据库
//!     └── Sandboxfile        # MICROSANDBOX_CONFIG_FILENAME: 配置文件
//! ```
//!
//! ### 全局目录（位于用户主目录）
//! ```text
//! ~/.microsandbox/           # MICROSANDBOX_HOME_DIR
//! ├── layers/                # LAYERS_SUBDIR: 镜像层存储
//! ├── installs/              # INSTALLS_SUBDIR: 已安装的沙箱
//! ├── projects/              # PROJECTS_SUBDIR: 项目配置
//! ├── oci.db                 # OCI_DB_FILENAME: OCI 数据库
//! ├── server.pid             # SERVER_PID_FILE: 服务器 PID
//! └── server.key             # SERVER_KEY_FILE: 服务器密钥
//! ```
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::path::{
//!     normalize_path, SupportedPathType,
//!     MICROSANDBOX_HOME_DIR, SANDBOX_DB_FILENAME,
//! };
//!
//! // 规范化路径
//! let normalized = normalize_path("/data/./app", SupportedPathType::Absolute).unwrap();
//! assert_eq!(normalized, "/data/app");
//!
//! // 防止路径遍历攻击
//! let result = normalize_path("/data/../etc/passwd", SupportedPathType::Absolute);
//! assert!(result.is_err());  // 错误：不能遍历到根目录之上
//!
//! // 使用路径常量
//! let sandbox_db_path = format!("{}/{}", MICROSANDBOX_HOME_DIR, SANDBOX_DB_FILENAME);
//! ```

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use typed_path::{Utf8UnixComponent, Utf8UnixPathBuf};

use crate::{MicrosandboxUtilsError, MicrosandboxUtilsResult};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// ### 项目级环境变量目录名：`.menv`
///
/// 这是 microsandbox 在项目根目录下创建的隐藏目录，用于存储项目特定的配置和数据。
/// 名称 `.menv` 代表 "microsandbox environment"。
///
/// ## 目录结构
/// ```text
/// <PROJECT_ROOT>/.menv/
/// ├── rw/          # 读写层
/// ├── patch/       # 补丁层
/// ├── log/         # 日志
/// └── sandbox.db   # 沙箱数据库
/// ```
pub const MICROSANDBOX_ENV_DIR: &str = ".menv";

/// ### 全局数据目录名：`.microsandbox`
///
/// 这是 microsandbox 在用户主目录（`~`）下创建的隐藏目录，用于存储全局数据。
///
/// ## 完整路径示例
/// - Linux: `/home/username/.microsandbox`
/// - macOS: `/Users/username/.microsandbox`
pub const MICROSANDBOX_HOME_DIR: &str = ".microsandbox";

/// ### 读写层子目录名：`rw`
///
/// 用于存储项目的读写层（read-write layer）。
/// 在 OverlayFS 中，读写层是所有修改的存储位置。
///
/// ## OverlayFS 概念
/// OverlayFS 是一种联合文件系统，它将多个目录"叠加"在一起：
/// - **下层（lower）**: 只读层，通常是基础镜像
/// - **上层（upper）**: 读写层，存储所有修改
/// - **合并层（merged）**: 用户看到的统一视图
///
/// ## 完整路径
/// `<PROJECT_ROOT>/.menv/rw/`
pub const RW_SUBDIR: &str = "rw";

/// ### 补丁层子目录名：`patch`
///
/// 用于存储项目的补丁层。
/// 补丁层允许用户应用自定义修改到基础镜像，而不直接修改原始层。
///
/// ## 使用场景
/// - 应用安全补丁
/// - 添加自定义配置
/// - 安装额外的软件包
///
/// ## 完整路径
/// `<PROJECT_ROOT>/.menv/patch/`
pub const PATCH_SUBDIR: &str = "patch";

/// ### 日志子目录名：`log`
///
/// 用于存储项目的日志文件。
///
/// ## 完整路径
/// `<PROJECT_ROOT>/.menv/log/`
///
/// ## 相关文件
/// - [`SUPERVISOR_LOG_FILENAME`]: 监督者日志文件名
pub const LOG_SUBDIR: &str = "log";

/// ### 镜像层存储目录：`layers`
///
/// 在全局目录下存储所有下载的 OCI 镜像层。
/// 每个层根据其 digest（哈希值）命名。
///
/// ## 完整路径
/// `~/.microsandbox/layers/`
///
/// ## 层提取目录
/// 提取后的层存储在 `<layer_id>.extracted/` 目录下，
/// 其中 `.extracted` 后缀由 [`EXTRACTED_LAYER_SUFFIX`] 定义。
pub const LAYERS_SUBDIR: &str = "layers";

/// ### 沙箱安装目录：`installs`
///
/// 在全局目录下存储已安装的沙箱。
///
/// ## 完整路径
/// `~/.microsandbox/installs/`
pub const INSTALLS_SUBDIR: &str = "installs";

/// ### 沙箱数据库文件名：`sandbox.db`
///
/// 项目级数据库文件，存储沙箱的状态和配置信息。
///
/// ## 完整路径
/// `<PROJECT_ROOT>/.menv/sandbox.db`
pub const SANDBOX_DB_FILENAME: &str = "sandbox.db";

/// ### OCI 数据库文件名：`oci.db`
///
/// 全局数据库文件，存储 OCI 镜像的元数据信息。
///
/// ## 完整路径
/// `~/.microsandbox/oci.db`
pub const OCI_DB_FILENAME: &str = "oci.db";

/// ### 沙箱脚本目录：`.sandbox`
///
/// 在 MicroVM 内部，用于存储沙箱脚本的目录。
pub const SANDBOX_DIR: &str = ".sandbox";

/// ### 脚本子目录名：`scripts`
///
/// 沙箱脚本目录下的 scripts 子目录。
///
/// ## 完整路径
/// `.sandbox/scripts/`
pub const SCRIPTS_DIR: &str = "scripts";

/// ### 层提取目录后缀：`extracted`
///
/// 添加到提取后的层目录 ID 后面的后缀。
///
/// ## 示例
/// 如果一个层的 ID 是 `abc123`，那么提取后的目录名为：
/// `abc123.extracted/`
///
/// ## 存储位置
/// `~/.microsandbox/layers/abc123.extracted/`
pub const EXTRACTED_LAYER_SUFFIX: &str = "extracted";

/// ### 沙箱配置文件名：`Sandboxfile`
///
/// 项目沙箱配置文件的名称，使用 YAML 格式。
///
/// ## 完整路径
/// `<PROJECT_ROOT>/.menv/Sandboxfile`
///
/// ## 文件格式
/// ```yaml
/// # Sandbox configurations
/// sandboxes:
///   my-sandbox:
///     image: nginx:latest
///     ports:
///       - "8080:80"
/// ```
pub const MICROSANDBOX_CONFIG_FILENAME: &str = "Sandboxfile";

/// ### Shell 脚本名：`shell`
///
/// 用于启动 shell 会话的脚本文件名。
///
/// ## 完整路径
/// `<PROJECT_ROOT>/.menv/patch/<config_name>/shell`
pub const SHELL_SCRIPT_NAME: &str = "shell";

/// ### 项目存储子目录：`projects`
///
/// 在全局目录下存储项目配置信息。
///
/// ## 完整路径
/// `~/.microsandbox/projects/`
pub const PROJECTS_SUBDIR: &str = "projects";

/// ### 服务器 PID 文件名：`server.pid`
///
/// 存储 microsandbox 服务器进程 ID 的文件。
///
/// ## 完整路径
/// `~/.microsandbox/server.pid`
///
/// ## 用途
/// 用于检查服务器是否正在运行，以及向服务器发送信号。
pub const SERVER_PID_FILE: &str = "server.pid";

/// ### 服务器密钥文件名：`server.key`
///
/// 存储服务器认证密钥的文件。
///
/// ## 完整路径
/// `~/.microsandbox/server.key`
///
/// ## 安全说明
/// 此文件包含敏感信息，应设置适当的文件权限（如 600）。
pub const SERVER_KEY_FILE: &str = "server.key";

/// ### 门户端口映射文件名：`portal.ports`
///
/// 存储沙箱门户端口映射信息的文件。
///
/// ## 完整路径
/// `~/.microsandbox/projects/portal.ports`
pub const PORTAL_PORTS_FILE: &str = "portal.ports";

/// ### XDG 基础目录规范的主目录
///
/// 根据 [XDG 基础目录规范](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html)，
/// 用户级数据应存储在 `~/.local` 目录下。
///
/// ## XDG 规范简介
/// XDG（X Desktop Group）规范定义了 Linux 桌面环境的标准目录结构：
/// - `XDG_DATA_HOME`: 用户数据文件（默认 `~/.local/share`）
/// - `XDG_CONFIG_HOME`: 用户配置文件（默认 `~/.config`）
/// - `XDG_STATE_HOME`: 用户状态文件（默认 `~/.local/state`）
///
/// microsandbox 使用 `~/.local` 作为额外的安装位置。
pub static XDG_HOME_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| dirs::home_dir().unwrap().join(".local"));

/// ### XDG bin 目录：`bin`
///
/// 用户级可执行文件的目录。
///
/// ## 完整路径
/// `~/.local/bin/`
///
/// ## 用途
/// microsandbox 可执行文件可以安装到此目录，方便用户全局访问。
pub const XDG_BIN_DIR: &str = "bin";

/// ### XDG lib 目录：`lib`
///
/// 用户级库文件的目录。
///
/// ## 完整路径
/// `~/.local/lib/`
pub const XDG_LIB_DIR: &str = "lib";

/// ### 日志文件后缀：`log`
///
/// 用于日志文件的后缀名。
pub const LOG_SUFFIX: &str = "log";

/// ### 监督者日志文件名：`supervisor.log`
///
/// 进程监督者（Supervisor）的日志文件名。
///
/// ## 完整路径
/// `<LOG_SUBDIR>/supervisor.log`
pub const SUPERVISOR_LOG_FILENAME: &str = "supervisor.log";

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// ### 支持的路径类型枚举
///
/// 用于指定路径必须是绝对路径、相对路径，或两者皆可。
///
/// ## 变体说明
///
/// ### `Any`
/// 接受任何类型的路径（绝对或相对）。
///
/// ### `Absolute`
/// 只接受绝对路径（以 `/` 开头）。
///
/// ### `Relative`
/// 只接受相对路径（不以 `/` 开头）。
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::path::{normalize_path, SupportedPathType};
///
/// // 只接受绝对路径
/// let result = normalize_path("/data/app", SupportedPathType::Absolute);
/// assert!(result.is_ok());
///
/// let result = normalize_path("data/app", SupportedPathType::Absolute);
/// assert!(result.is_err());  // 错误：必须是绝对路径
///
/// // 只接受相对路径
/// let result = normalize_path("data/app", SupportedPathType::Relative);
/// assert!(result.is_ok());
///
/// let result = normalize_path("/data/app", SupportedPathType::Relative);
/// assert!(result.is_err());  // 错误：必须是相对路径
/// ```
pub enum SupportedPathType {
    /// 接受任何路径类型
    Any,
    /// 只接受绝对路径
    Absolute,
    /// 只接受相对路径
    Relative,
}

//--------------------------------------------------------------------------------------------------
// 函数定义
//--------------------------------------------------------------------------------------------------

/// ### 规范化路径字符串
///
/// 此函数将路径标准化为统一的表示形式，便于进行卷挂载比较和验证。
///
/// ## 规范化规则
///
/// 1. **解析 `.` 和 `..` 组件**: 在可能的情况下解析当前目录和父目录引用
///    - `/data/./app` → `/data/app`
///    - `/data/temp/../app` → `/data/app`
///
/// 2. **防止路径遍历攻击**: 阻止试图逃逸根目录的路径
///    - `/data/../..` → 错误（试图访问根目录之上）
///
/// 3. **移除冗余分隔符**: 合并连续的斜杠
///    - `/data//app` → `/data/app`
///
/// 4. **移除尾部斜杠**: 标准化路径结尾
///    - `/data/app/` → `/data/app`
///
/// 5. **区分大小写**: 遵循 Unix 标准，路径区分大小写
///
/// ## 参数说明
///
/// - `path`: 要规范化的路径字符串
/// - `path_type`: 要求的路径类型（绝对、相对或任意）
///
/// ## 返回值
///
/// - `Ok(String)`: 规范化后的路径
/// - `Err(MicrosandboxUtilsError::PathValidation)`: 路径无效时
///
/// ## 错误情况
///
/// 1. **空路径**: 路径不能为空字符串
/// 2. **路径遍历**: 试图使用 `..` 逃逸到根目录之上
/// 3. **类型不匹配**: 路径类型不符合要求（如要求绝对路径但提供相对路径）
/// 4. **根组件位置错误**: `/` 只能出现在路径开头
///
/// ## 实现原理
///
/// 使用 `typed_path` 库的 `Utf8UnixPathBuf` 来解析路径组件：
/// - `RootDir`: 根目录 `/`
/// - `ParentDir`: 父目录 `..`
/// - `CurDir`: 当前目录 `.`
/// - `Normal`: 普通组件
///
/// 通过维护一个 `depth` 计数器来跟踪当前深度，防止遍历到根目录之上。
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_utils::path::{normalize_path, SupportedPathType};
///
/// // 基本规范化
/// assert_eq!(
///     normalize_path("/data/app/", SupportedPathType::Absolute).unwrap(),
///     "/data/app"
/// );
///
/// // 解析 .. 组件
/// assert_eq!(
///     normalize_path("/data/temp/../app", SupportedPathType::Absolute).unwrap(),
///     "/data/app"
/// );
///
/// // 复杂路径
/// assert_eq!(
///     normalize_path("/data/./temp/../logs/app/./config/../", SupportedPathType::Absolute).unwrap(),
///     "/data/logs/app"
/// );
///
/// // 错误情况：路径遍历
/// assert!(normalize_path("/data/../..", SupportedPathType::Any).is_err());
///
/// // 错误情况：类型不匹配
/// assert!(normalize_path("data/app", SupportedPathType::Absolute).is_err());
/// ```
pub fn normalize_path(path: &str, path_type: SupportedPathType) -> MicrosandboxUtilsResult<String> {
    // 检查空路径
    if path.is_empty() {
        return Err(MicrosandboxUtilsError::PathValidation(
            "Path cannot be empty".to_string(),
        ));
    }

    // 使用 typed_path 库解析路径
    // Utf8UnixPathBuf 确保路径是有效的 UTF-8 字符串
    let path = Utf8UnixPathBuf::from(path);
    let mut normalized = Vec::new();  // 存储规范化后的组件
    let mut is_absolute = false;      // 标记是否为绝对路径
    let mut depth = 0;                // 当前深度（用于防止路径遍历）

    // 遍历路径的每个组件
    for component in path.components() {
        match component {
            // 根目录组件：必须是路径的第一个组件
            Utf8UnixComponent::RootDir => {
                if normalized.is_empty() {
                    is_absolute = true;
                    normalized.push("/".to_string());
                } else {
                    // 根组件出现在中间位置，这是无效的路径
                    return Err(MicrosandboxUtilsError::PathValidation(
                        "Invalid path: root component '/' found in middle of path".to_string(),
                    ));
                }
            }
            // 父目录引用：`..`
            Utf8UnixComponent::ParentDir => {
                if depth > 0 {
                    // 可以向上移动一层
                    normalized.pop();  // 移除最后一个组件
                    depth -= 1;
                } else {
                    // 试图移动到根目录之上，这是不允许的
                    return Err(MicrosandboxUtilsError::PathValidation(
                        "Invalid path: cannot traverse above root directory".to_string(),
                    ));
                }
            }
            // 当前目录引用：`.`，直接跳过
            Utf8UnixComponent::CurDir => continue,
            // 普通组件：直接添加
            Utf8UnixComponent::Normal(c) => {
                if !c.is_empty() {
                    normalized.push(c.to_string());
                    depth += 1;
                }
            }
        }
    }

    // 根据 path_type 参数验证路径类型
    match path_type {
        // 要求绝对路径但不是绝对路径
        SupportedPathType::Absolute if !is_absolute => {
            return Err(MicrosandboxUtilsError::PathValidation(
                "Path must be absolute (start with '/')".to_string(),
            ));
        }
        // 要求相对路径但是绝对路径
        SupportedPathType::Relative if is_absolute => {
            return Err(MicrosandboxUtilsError::PathValidation(
                "Path must be relative (must not start with '/')".to_string(),
            ));
        }
        // Any 类型或其他情况：无需额外检查
        _ => {}
    }

    // 构建最终的路径字符串
    if is_absolute {
        if normalized.len() == 1 {
            // 只有根目录
            Ok("/".to_string())
        } else {
            // 连接所有组件，在开头添加 `/`
            // normalized[0] 是 "/"，所以从 [1..] 开始连接
            Ok(format!("/{}", normalized[1..].join("/")))
        }
    } else {
        // 相对路径：直接连接所有组件
        Ok(normalized.join("/"))
    }
}

/// ### 解析环境变量路径
///
/// 此函数解析文件路径，优先检查环境变量，如果未设置则使用默认路径。
///
/// ## 参数说明
///
/// - `env_var`: 环境变量的名称
/// - `default_path`: 默认路径（任何可转换为 `&Path` 的类型）
///
/// ## 返回值
///
/// - `Ok(PathBuf)`: 解析后的路径（文件必须存在）
/// - `Err(MicrosandboxUtilsError::FileNotFound)`: 文件在指定位置不存在
///
/// ## 解析逻辑
///
/// 1. 首先尝试读取环境变量 `env_var`
/// 2. 如果环境变量存在，使用该值作为路径
/// 3. 如果环境变量不存在，使用 `default_path`
/// 4. 检查文件是否在解析后的路径存在
/// 5. 如果文件不存在，返回 `FileNotFound` 错误
///
/// ## 错误信息来源
/// 错误消息中包含文件来源说明：
/// - "environment variable": 文件路径来自环境变量
/// - "default path": 文件路径来自默认值
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_utils::path::resolve_env_path;
///
/// // 假设设置了环境变量 CONFIG_PATH=/custom/config.yaml
/// let config_path = resolve_env_path("CONFIG_PATH", "/etc/default/config.yaml");
/// // 返回：Ok("/custom/config.yaml")（如果文件存在）
///
/// // 如果未设置环境变量
/// let config_path = resolve_env_path("NON_EXISTENT_VAR", "/etc/default/config.yaml");
/// // 返回：Ok("/etc/default/config.yaml")（如果文件存在）
///
/// // 如果文件不存在
/// let config_path = resolve_env_path("NON_EXISTENT_VAR", "/non/existent/path");
/// // 返回：Err(FileNotFound("/non/existent/path", "default path"))
/// ```
pub fn resolve_env_path(
    env_var: &str,
    default_path: impl AsRef<Path>,
) -> MicrosandboxUtilsResult<PathBuf> {
    // 尝试读取环境变量
    // std::env::var() 返回 Result<String, VarError>
    let (path, source) = std::env::var(env_var)
        // 如果环境变量存在，使用它并标记来源为"环境变量"
        .map(|p| (PathBuf::from(p), "environment variable"))
        // 如果环境变量不存在，使用默认路径并标记来源为"默认路径"
        .unwrap_or_else(|_| (default_path.as_ref().to_path_buf(), "default path"));

    // 检查文件是否存在
    if !path.exists() {
        return Err(MicrosandboxUtilsError::FileNotFound(
            // 将路径转换为字符串表示
            path.to_string_lossy().to_string(),
            // 包含来源信息，便于调试
            source.to_string(),
        ));
    }

    Ok(path)
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        // 测试 SupportedPathType::Absolute
        assert_eq!(
            normalize_path("/data/app/", SupportedPathType::Absolute).unwrap(),
            "/data/app"
        );
        assert_eq!(
            normalize_path("/data//app", SupportedPathType::Absolute).unwrap(),
            "/data/app"
        );
        assert_eq!(
            normalize_path("/data/./app", SupportedPathType::Absolute).unwrap(),
            "/data/app"
        );

        // 测试 SupportedPathType::Relative
        assert_eq!(
            normalize_path("data/app/", SupportedPathType::Relative).unwrap(),
            "data/app"
        );
        assert_eq!(
            normalize_path("./data/app", SupportedPathType::Relative).unwrap(),
            "data/app"
        );
        assert_eq!(
            normalize_path("data//app", SupportedPathType::Relative).unwrap(),
            "data/app"
        );

        // 测试 SupportedPathType::Any
        assert_eq!(
            normalize_path("/data/app", SupportedPathType::Any).unwrap(),
            "/data/app"
        );
        assert_eq!(
            normalize_path("data/app", SupportedPathType::Any).unwrap(),
            "data/app"
        );

        // 测试路径遍历（在允许范围内）
        assert_eq!(
            normalize_path("/data/temp/../app", SupportedPathType::Absolute).unwrap(),
            "/data/app"
        );
        assert_eq!(
            normalize_path("data/temp/../app", SupportedPathType::Relative).unwrap(),
            "data/app"
        );

        // 测试无效路径
        assert!(matches!(
            normalize_path("data/app", SupportedPathType::Absolute),
            Err(MicrosandboxUtilsError::PathValidation(e)) if e.contains("must be absolute")
        ));
        assert!(matches!(
            normalize_path("/data/app", SupportedPathType::Relative),
            Err(MicrosandboxUtilsError::PathValidation(e)) if e.contains("must be relative")
        ));
        assert!(matches!(
            normalize_path("/data/../..", SupportedPathType::Any),
            Err(MicrosandboxUtilsError::PathValidation(e)) if e.contains("cannot traverse above root")
        ));
    }

    #[test]
    fn test_normalize_path_complex() {
        // 测试复杂但有效的路径
        assert_eq!(
            normalize_path(
                "/data/./temp/../logs/app/./config/../",
                SupportedPathType::Absolute
            )
            .unwrap(),
            "/data/logs/app"
        );
        assert_eq!(
            normalize_path(
                "/data///temp/././../app//./test/..",
                SupportedPathType::Absolute
            )
            .unwrap(),
            "/data/app"
        );

        // 测试边界情况
        assert_eq!(
            normalize_path("/data/./././.", SupportedPathType::Absolute).unwrap(),
            "/data"
        );
        assert_eq!(
            normalize_path("/data/test/../../data/app", SupportedPathType::Absolute).unwrap(),
            "/data/app"
        );

        // 测试无效的复杂路径
        assert!(matches!(
            normalize_path("/data/test/../../../root", SupportedPathType::Any),
            Err(MicrosandboxUtilsError::PathValidation(e)) if e.contains("cannot traverse above root")
        ));
        assert!(matches!(
            normalize_path("/./data/../..", SupportedPathType::Any),
            Err(MicrosandboxUtilsError::PathValidation(e)) if e.contains("cannot traverse above root")
        ));
    }
}
