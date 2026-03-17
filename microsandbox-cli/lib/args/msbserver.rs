//! # msbserver 命令参数定义
//!
//! 本文件定义了 `msbserver` 命令的命令行参数。
//! `msbserver` 是微沙箱的服务器组件，提供 API 接口和 MCP（Model Context Protocol）服务。

use std::path::PathBuf;

use clap::Parser;
use microsandbox_utils::{DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT};

use crate::styles;

//--------------------------------------------------------------------------------------------------
// Types - 类型定义
//--------------------------------------------------------------------------------------------------

/// ## msbserver 命令参数结构体
///
/// 这个结构体定义了 msbserver 命令的所有命令行参数。
/// 使用 `#[derive(Parser)]` 宏，clap 会自动生成参数解析代码。
///
/// ### 派生宏说明
///
/// **`#[derive(Parser)]`**
/// - 自动生成 `parse()` 方法，从 `std::env::args()` 解析参数
/// - 自动生成帮助信息（`--help`）
///
/// **`#[command(...)]` 属性**
/// - 设置命令的元数据（名称、作者、样式等）
/// - `styles=styles::styles()`: 使用自定义的 ANSI 样式
///
/// ### 字段属性说明
///
/// | 属性 | 说明 |
/// |------|------|
/// | `#[arg(short = 'k')]` | 短选项形式，如 `-k` |
/// | `#[arg(long = "key")]` | 长选项形式，如 `--key` |
/// | `#[arg(default_value = "...")]` | 默认值 |
/// | `#[arg(default_value_t = false)]` | 布尔默认值（t 表示 true/false） |
#[derive(Debug, Parser)]
#[command(name = "msbserver", author, styles=styles::styles())]
pub struct MsbserverArgs {
    /// ### JWT 密钥
    ///
    /// 用于生成和验证 JWT（JSON Web Token）的密钥。
    /// JWT 是一种安全的令牌格式，用于 API 认证。
    ///
    /// ### 使用方式
    /// - 短选项：`-k mysecretkey`
    /// - 长选项：`--key mysecretkey`
    /// - 如不指定，服务器会自动生成一个随机密钥
    #[arg(short = 'k', long = "key")]
    pub key: Option<String>,

    /// ### 服务器监听地址
    ///
    /// 服务器绑定监听的网络地址。
    ///
    /// ### 常见值
    /// - `127.0.0.1`: 仅本地访问（默认）
    /// - `0.0.0.0`: 所有网络接口
    /// - `::1`: IPv6 本地回环
    ///
    /// ### 默认值
    /// 使用 `DEFAULT_SERVER_HOST` 常量（通常为 "127.0.0.1"）
    #[arg(long, default_value = DEFAULT_SERVER_HOST)]
    pub host: String,

    /// ### 服务器监听端口
    ///
    /// 服务器绑定监听的 TCP 端口号。
    ///
    /// ### 默认值
    /// 使用 `DEFAULT_SERVER_PORT` 常量
    ///
    /// ### `default_value_t` 说明
    /// - `_t` 后缀表示从字段的默认值推断
    /// - 与 `default_value`（字符串）不同，这是类型化的默认值
    #[arg(long, default_value_t = DEFAULT_SERVER_PORT)]
    pub port: u16,

    /// ### 项目目录
    ///
    /// 用于存储沙箱配置和状态数据的目录路径。
    ///
    /// ### 使用方式
    /// - 短选项：`-p /path/to/project`
    /// - 长选项：`--path /path/to/project`
    ///
    /// ### PathBuf 类型
    /// - Rust 标准库的路径类型
    /// - 可拥有所有权的平台无关路径表示
    #[arg(short = 'p', long = "path")]
    pub project_dir: Option<PathBuf>,

    /// ### 开发模式标志
    ///
    /// 启用开发模式，提供更详细的日志和调试信息。
    ///
    /// ### 布尔标志说明
    /// - 不需要值，出现即表示 true
    /// - 如：`--dev` 启用开发模式
    /// - 默认值为 `false`
    #[arg(long = "dev", default_value_t = false)]
    pub dev_mode: bool,
}
