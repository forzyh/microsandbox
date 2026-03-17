//! # 环境变量工具模块
//!
//! 本模块提供用于读取和处理 microsandbox 相关环境变量的工具函数。
//!
//! ## 功能概述
//!
//! microsandbox 支持通过环境变量自定义各种配置，本模块提供统一的接口来读取这些变量。
//! 如果环境变量未设置，会自动回退到预定义的默认值。
//!
//! ## 支持的环境变量
//!
//! | 环境变量名 | 用途 | 默认值 |
//! |-----------|------|--------|
//! | `MICROSANDBOX_HOME` | 沙箱主目录路径 | `~/.microsandbox` |
//! | `OCI_REGISTRY_DOMAIN` | OCI 镜像仓库域名 | `docker.io` |
//! | `MSBRUN_EXE` | msbrun 运行时二进制路径 | 自动检测 |
//! | `MSBSERVER_EXE` | msbserver 服务器二进制路径 | 自动检测 |
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::env::{
//!     get_microsandbox_home_path,
//!     get_oci_registry,
//!     MICROSANDBOX_HOME_ENV_VAR,
//! };
//!
//! // 获取沙箱主目录路径
//! // 如果设置了 MICROSANDBOX_HOME=/custom/path，则返回 /custom/path
//! // 否则返回 ~/.microsandbox
//! let home_path = get_microsandbox_home_path();
//!
//! // 获取 OCI 仓库域名
//! // 如果设置了 OCI_REGISTRY_DOMAIN=ghcr.io，则返回 ghcr.io
//! // 否则返回 docker.io
//! let registry = get_oci_registry();
//!
//! // 也可以直接使用常量获取环境变量名
//! println!("沙箱主目录环境变量名：{}", MICROSANDBOX_HOME_ENV_VAR);
//! ```
//!
//! ## 环境变量优先级
//!
//! microsandbox 的配置遵循以下优先级（从高到低）：
//! 1. 命令行参数（如果支持）
//! 2. 环境变量
//! 3. 配置文件
//! 4. 内置默认值
//!
//! 本模块处理的是第 2 级（环境变量）到第 4 级（默认值）的回退逻辑。

use std::path::PathBuf;

use crate::{DEFAULT_MICROSANDBOX_HOME, DEFAULT_OCI_REGISTRY};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// ### 沙箱主目录环境变量名：`MICROSANDBOX_HOME`
///
/// 通过设置此环境变量，用户可以自定义 microsandbox 的全局数据存储目录。
///
/// ## 使用场景
/// - 多用户共享系统：每个用户设置不同的主目录
/// - 测试环境：使用临时目录避免污染正式环境
/// - CI/CD 环境：使用特定目录便于清理
///
/// ## 示例
/// ```bash
/// # Bash/Zsh
/// export MICROSANDBOX_HOME=/tmp/microsandbox-test
/// microsandbox run
///
/// # 或者临时设置
/// MICROSANDBOX_HOME=/custom/path microsandbox run
/// ```
pub const MICROSANDBOX_HOME_ENV_VAR: &str = "MICROSANDBOX_HOME";

/// ### OCI 仓库域名环境变量名：`OCI_REGISTRY_DOMAIN`
///
/// 允许用户自定义默认的 OCI 镜像仓库域名。
///
/// ## 使用场景
/// - 使用私有仓库：设置为公司内部的镜像仓库
/// - 使用镜像加速：设置为国内镜像站（如阿里云、腾讯云）
/// - 使用 GitHub Container Registry：设置为 `ghcr.io`
///
/// ## 示例
/// ```bash
/// # 使用 GitHub Container Registry
/// export OCI_REGISTRY_DOMAIN=ghcr.io
///
/// # 使用阿里云镜像加速
/// export OCI_REGISTRY_DOMAIN=registry.cn-hangzhou.aliyuncs.com
/// ```
pub const OCI_REGISTRY_ENV_VAR: &str = "OCI_REGISTRY_DOMAIN";

/// ### msbrun 二进制路径环境变量名：`MSBRUN_EXE`
///
/// 允许用户显式指定 msbrun 运行时二进制的位置。
///
/// ## 使用场景
/// - 自定义安装位置：当 msbrun 不在默认位置时
/// - 开发调试：使用不同版本的 msbrun 进行测试
/// - 多版本共存：同时安装多个版本并切换使用
///
/// ## 示例
/// ```bash
/// export MSBRUN_EXE=/opt/microsandbox/custom/msbrun
/// ```
pub const MSBRUN_EXE_ENV_VAR: &str = "MSBRUN_EXE";

/// ### msbserver 二进制路径环境变量名：`MSBSERVER_EXE`
///
/// 允许用户显式指定 msbserver 服务器二进制的位置。
///
/// ## 使用场景
/// 与 [`MSBRUN_EXE_ENV_VAR`] 类似，用于自定义服务器组件位置。
pub const MSBSERVER_EXE_ENV_VAR: &str = "MSBSERVER_EXE";

//--------------------------------------------------------------------------------------------------
// 函数定义
//--------------------------------------------------------------------------------------------------

/// ### 获取沙箱主目录路径
///
/// 此函数返回 microsandbox 的全局数据存储目录路径。
///
/// ## 返回值优先级
/// 1. 如果设置了 `MICROSANDBOX_HOME` 环境变量，返回该变量的值
/// 2. 否则，返回默认路径 `~/.microsandbox`
///
/// ## 返回值类型
/// 返回 `PathBuf`，这是一个可拥有的路径类型，可以自由修改。
///
/// ## 实现细节
/// - 使用 `std::env::var()` 读取环境变量
/// - 如果环境变量不存在或不是有效 UTF-8，`var()` 返回 `Err`
/// - 使用 `if let` 模式匹配成功的情况
///
/// ## 示例
///
/// ```rust
/// use microsandbox_utils::get_microsandbox_home_path;
/// use std::path::PathBuf;
///
/// // 正常情况（未设置环境变量）
/// let path = get_microsandbox_home_path();
/// // path 可能是："/home/user/.microsandbox" (Linux)
/// // 或："/Users/user/.microsandbox" (macOS)
///
/// // 设置环境变量后
/// // export MICROSANDBOX_HOME=/custom/sandbox
/// // path 会是："/custom/sandbox"
/// ```
///
/// ## 相关类型
/// - [`PathBuf`](std::path::PathBuf): 可拥有的可变路径
/// - [`Path`](std::path::Path): 不可变的路径视图
pub fn get_microsandbox_home_path() -> PathBuf {
    // 尝试读取 MICROSANDBOX_HOME 环境变量
    // std::env::var() 返回 Result<String, VarError>
    // - Ok(String): 环境变量存在且是有效 UTF-8
    // - Err(VarError): 环境变量不存在或不是有效 UTF-8
    if let Ok(microsandbox_home) = std::env::var(MICROSANDBOX_HOME_ENV_VAR) {
        // 环境变量存在，将其转换为 PathBuf
        PathBuf::from(microsandbox_home)
    } else {
        // 环境变量不存在，使用默认值
        // DEFAULT_MICROSANDBOX_HOME 是 LazyLock<PathBuf>，解引用后得到 &PathBuf
        // 使用 to_owned() 创建一个新的 PathBuf 副本
        DEFAULT_MICROSANDBOX_HOME.to_owned()
    }
}

/// ### 获取 OCI 镜像仓库域名
///
/// 此函数返回用于拉取 Docker/OCI 镜像的仓库域名。
///
/// ## 返回值优先级
/// 1. 如果设置了 `OCI_REGISTRY_DOMAIN` 环境变量，返回该变量的值
/// 2. 否则，返回默认域名 `docker.io`
///
/// ## 返回值类型
/// 返回 `String`，因为域名是纯文本字符串。
///
/// ## OCI 镜像命名规范
/// 完整的 OCI 镜像引用格式为：
/// ```text
/// [registry]/[namespace]/[repository]:[tag]
/// ```
/// 例如：`docker.io/library/nginx:latest`
///
/// 当用户只提供镜像名（如 `nginx`）时，会自动补全为：
/// `docker.io/library/nginx:latest`
///
/// ## 示例
///
/// ```rust
/// use microsandbox_utils::get_oci_registry;
///
/// // 正常情况（未设置环境变量）
/// let registry = get_oci_registry();
/// assert_eq!(registry, "docker.io");
///
/// // 设置环境变量后
/// // export OCI_REGISTRY_DOMAIN=ghcr.io
/// // registry 会是："ghcr.io"
/// ```
///
/// ## 常见 OCI 仓库域名
/// - `docker.io`: Docker Hub（官方公共仓库）
/// - `ghcr.io`: GitHub Container Registry
/// - `quay.io`: Red Hat Quay
/// - `registry.cn-hangzhou.aliyuncs.com`: 阿里云容器镜像服务
pub fn get_oci_registry() -> String {
    // 尝试读取 OCI_REGISTRY_DOMAIN 环境变量
    if let Ok(oci_registry_domain) = std::env::var(OCI_REGISTRY_ENV_VAR) {
        // 环境变量存在，直接返回（已经是 String 类型）
        oci_registry_domain
    } else {
        // 环境变量不存在，使用默认值
        // DEFAULT_OCI_REGISTRY 是 &str，转换为 String
        DEFAULT_OCI_REGISTRY.to_string()
    }
}
