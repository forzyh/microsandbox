//! # 配置模块 - 服务器配置管理
//!
//! 本模块负责管理微沙箱服务器的所有配置项，包括：
//!
//! ## 配置项说明
//!
//! | 配置项 | 类型 | 说明 | 是否必需 |
//! |--------|------|------|----------|
//! | `key` | `Option<String>` | JWT 密钥，用于签名和验证令牌 | 非开发模式下必需 |
//! | `project_dir` | `PathBuf` | 项目目录，存储沙箱配置和状态文件 | 否（有默认值） |
//! | `dev_mode` | `bool` | 开发模式开关，跳过认证验证 | 否（默认 false） |
//! | `host` | `IpAddr` | 服务器监听的网络地址 | 否（默认 127.0.0.1） |
//! | `port` | `u16` | 服务器监听的端口号 | 否（默认值由 CLI 定义） |
//! | `addr` | `SocketAddr` | 完整的监听地址（host:port） | 内部计算生成 |
//!
//! ## 目录结构
//!
//! 服务器会在项目目录下创建以下文件：
//!
//! ```text
//! ~/.microsandbox/projects/           # 默认项目目录
//! ├── microsandbox.yaml               # 沙箱配置文件
//! ├── server.pid                      # 服务器进程 ID 文件
//! ├── server.key                      # JWT 密钥文件（非开发模式）
//! └── portal_ports.json               # 端口分配记录
//! ```
//!
//! ## 安全模式 vs 开发模式
//!
//! ### 安全模式（默认）
//! - 必须提供 JWT 密钥（通过 `--key` 参数或自动生成）
//! - 密钥会保存到 `~/.microsandbox/server.key`
//! - 所有 API 请求必须携带有效的 API 密钥
//! - API 密钥格式：`msb_<jwt_token>`
//!
//! ### 开发模式（`--dev`）
//! - 不需要 JWT 密钥
//! - 跳过所有认证检查
//! - 适用于本地开发和调试
//! - **注意：不要在生产环境使用**
//!
//! ## 示例用法
//!
//! ```rust,no_run
//! use microsandbox_server::Config;
//! use std::path::PathBuf;
//!
//! // 创建开发模式配置（无需密钥）
//! let dev_config = Config::new(
//!     None,           // 无密钥
//!     "127.0.0.1".to_string(),
//!     8080,
//!     None,           // 使用默认项目目录
//!     true,           // 开发模式
//! ).unwrap();
//!
//! // 创建生产模式配置（需要密钥）
//! let prod_config = Config::new(
//!     Some("my-secret-key".to_string()),
//!     "0.0.0.0".to_string(),  // 监听所有网络接口
//!     8080,
//!     Some(PathBuf::from("/var/microsandbox")),
//!     false,          // 生产模式
//! ).unwrap();
//! ```

use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

// getset 是一个 Rust 宏库，可以自动生成 getter 方法
// #[getset(get = "pub with_prefix")] 会为每个字段生成 pub fn get_<field_name>() 方法
use getset::Getters;
use microsandbox_utils::{PROJECTS_SUBDIR, env};
use serde::Deserialize;

use crate::{MicrosandboxServerError, MicrosandboxServerResult};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// 代理认证请求头的名称
///
/// 客户端需要在 HTTP 请求头中携带此字段进行认证：
/// ```http
/// Proxy-Authorization: Bearer msb_<jwt_token>
/// ```
///
/// 也可以使用标准的 `Authorization` 请求头：
/// ```http
/// Authorization: Bearer msb_<jwt_token>
/// ```
pub const PROXY_AUTH_HEADER: &str = "Proxy-Authorization";

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// # 服务器配置结构体
///
/// 此结构体存储服务器运行所需的所有配置信息。
///
/// ## 派生 trait 说明
///
/// - `Debug`: 实现调试格式化输出，便于日志打印
/// - `Deserialize`: 支持从配置文件中反序列化（虽然当前主要从环境变量加载）
/// - `Getters`: 自动生成 getter 方法，避免手动编写样板代码
///
/// ## 字段详细说明
///
/// ### `key: Option<String>`
/// JWT 签名密钥，用于：
/// - 签名生成的 API 令牌（使用 HMAC-SHA256 算法）
/// - 验证客户端提交的令牌有效性
///
/// 在非开发模式下，此字段必须为 `Some(value)`。
/// 密钥应该是一个足够随机的字符串，建议使用至少 32 字节的随机数据。
///
/// ### `project_dir: PathBuf`
/// 项目目录路径，用于存储：
/// - `microsandbox.yaml` - 沙箱配置文件
/// - `server.pid` - 服务器进程 ID
/// - `server.key` - JWT 密钥文件
/// - `portal_ports.json` - 端口分配记录
///
/// 默认值为 `~/.microsandbox/projects/`
///
/// ### `dev_mode: bool`
/// 开发模式开关：
/// - `true`: 跳过认证，无需密钥，适用于本地开发
/// - `false`: 启用认证，需要有效密钥，适用于生产环境
///
/// **安全警告**：不要在生产环境启用开发模式！
///
/// ### `host: IpAddr`
/// 服务器监听的网络地址：
/// - `127.0.0.1` - 仅本地访问（默认，最安全）
/// - `0.0.0.0` - 所有网络接口（可被远程访问）
/// - 其他 IP - 特定网络接口
///
/// ### `port: u16`
/// 服务器监听的端口号，范围 1-65535。
/// 建议使用 1024 以上的端口以避免需要 root 权限。
///
/// ### `addr: SocketAddr`
/// 由 `host` 和 `port` 组合而成的完整地址，内部使用。
#[derive(Debug, Deserialize, Getters)]
#[getset(get = "pub with_prefix")]
pub struct Config {
    /// JWT 密钥，用于令牌签名和验证
    ///
    /// 在非开发模式下必须提供。密钥应该：
    /// - 至少 16 字节长（推荐 32 字节或更长）
    /// - 使用密码学安全的随机数生成器生成
    /// - 安全存储，不要泄露给未授权方
    /// - 定期轮换以提高安全性
    key: Option<String>,

    /// 项目目录路径
    ///
    /// 此目录用于存储服务器的所有持久化数据：
    /// - 沙箱配置文件 (microsandbox.yaml)
    /// - 服务器状态文件 (server.pid)
    /// - JWT 密钥文件 (server.key)
    /// - 端口分配记录 (portal_ports.json)
    ///
    /// 如果未指定，默认使用 `~/.microsandbox/projects/`
    project_dir: PathBuf,

    /// 开发模式开关
    ///
    /// 开发模式的特点：
    /// - 不需要 JWT 密钥
    /// - 跳过所有认证检查
    /// - 适用于本地开发和调试
    ///
    /// **警告**: 在生产环境中切勿启用此模式！
    dev_mode: bool,

    /// 服务器监听的网络地址
    ///
    /// 常见设置：
    /// - `127.0.0.1` - 仅本地访问（推荐，最安全）
    /// - `0.0.0.0` - 接受所有网络接口的连接
    host: IpAddr,

    /// 服务器监听的端口号
    ///
    /// 端口范围说明：
    /// - 0-1023: 系统端口（需要 root/admin 权限）
    /// - 1024-49151: 用户端口（推荐范围）
    /// - 49152-65535: 动态/私有端口
    port: u16,

    /// 完整的监听地址（由 host 和 port 组合而成）
    ///
    /// 此字段在创建 Config 时自动计算，格式为 `<host>:<port>`
    /// 例如：`127.0.0.1:8080` 或 `0.0.0.0:3000`
    addr: SocketAddr,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl Config {
    /// # 创建新的配置实例
    ///
    /// 此函数用于创建服务器配置，会根据开发模式进行不同的验证。
    ///
    /// ## 参数说明
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `key` | `Option<String>` | JWT 密钥，开发模式下可省略 |
    /// | `host` | `String` | 监听地址，如 `"127.0.0.1"` 或 `"0.0.0.0"` |
    /// | `port` | `u16` | 监听端口号，如 `8080` |
    /// | `project_dir` | `Option<PathBuf>` | 项目目录，None 时使用默认值 |
    /// | `dev_mode` | `bool` | 是否启用开发模式 |
    ///
    /// ## 返回值
    ///
    /// - `Ok(Config)`: 配置创建成功
    /// - `Err(MicrosandboxServerError::ConfigError)`: 配置验证失败
    ///
    /// ## 错误情况
    ///
    /// 1. **非开发模式下缺少密钥**
    ///    ```rust,ignore
    ///    Config::new(None, "127.0.0.1".to_string(), 8080, None, false)
    ///    // 返回 Err: "No key provided. A key is required when not in dev mode"
    ///    ```
    ///
    /// 2. **无效的 host 地址**
    ///    ```rust,ignore
    ///    Config::new(Some("key".to_string()), "invalid-ip".to_string(), 8080, None, false)
    ///    // 返回 Err: "Invalid host address: invalid-ip"
    ///    ```
    ///
    /// ## 实现细节
    ///
    /// 1. **密钥验证**：检查非开发模式下是否提供了密钥
    /// 2. **地址解析**：将 `host` 字符串解析为 `IpAddr` 类型
    /// 3. **目录设置**：如果未指定 `project_dir`，使用默认路径
    /// 4. **地址组合**：将 `host` 和 `port` 组合成 `SocketAddr`
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_server::Config;
    /// // 开发模式配置
    /// let dev_config = Config::new(
    ///     None,
    ///     "127.0.0.1".to_string(),
    ///     8080,
    ///     None,
    ///     true,
    /// ).expect("Failed to create dev config");
    ///
    /// // 生产模式配置
    /// let prod_config = Config::new(
    ///     Some("my-super-secret-key-32bytes!".to_string()),
    ///     "127.0.0.1".to_string(),
    ///     8080,
    ///     None,
    ///     false,
    /// ).expect("Failed to create prod config");
    /// ```
    pub fn new(
        key: Option<String>,
        host: String,
        port: u16,
        project_dir: Option<PathBuf>,
        dev_mode: bool,
    ) -> MicrosandboxServerResult<Self> {
        // 根据开发模式检查密钥要求
        // match 表达式用于模式匹配，处理 key 的三种情况：
        // 1. Some(k): 提供了密钥，直接使用
        // 2. None + dev_mode: 开发模式下允许无密钥
        // 3. None + !dev_mode: 非开发模式下无密钥则报错
        let key = match key {
            Some(k) => Some(k),
            None if dev_mode => None,
            None => {
                return Err(MicrosandboxServerError::ConfigError(
                    "No key provided. A key is required when not in dev mode".to_string(),
                ));
            }
        };

        // 将 host 字符串解析为 IpAddr 类型
        // parse() 方法会将字符串解析为对应的类型
        // map_err() 将解析错误转换为我们的自定义错误类型
        let host_ip: IpAddr = host.parse().map_err(|_| {
            MicrosandboxServerError::ConfigError(format!("Invalid host address: {}", host))
        })?;

        // 创建 SocketAddr，这是 Rust 标准库中的类型，表示 IP 地址和端口的组合
        let addr = SocketAddr::new(host_ip, port);

        // 设置项目目录：如果未指定，使用微沙箱的默认项目目录
        // env::get_microsandbox_home_path() 返回 ~/.microsandbox
        // PROJECTS_SUBDIR 是 "projects" 子目录
        let project_dir =
            project_dir.unwrap_or_else(|| env::get_microsandbox_home_path().join(PROJECTS_SUBDIR));

        // 返回创建好的配置结构体
        // Rust 的结构体更新语法允许我们简洁地初始化所有字段
        Ok(Self {
            key,
            project_dir,
            dev_mode,
            host: host_ip,
            port,
            addr,
        })
    }
}
