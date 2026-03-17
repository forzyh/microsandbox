//! # 服务器管理模块 - 微沙箱服务器生命周期管理
//!
//! 本模块提供微沙箱服务器的完整生命周期管理功能，包括：
//!
//! ## 核心功能
//!
//! ### 1. 服务器启动 (`start()`)
//! - 创建必要的目录结构
//! - 生成或加载 JWT 密钥
//! - 启动服务器进程
//! - 创建 PID 文件进行进程跟踪
//! - 设置信号处理（SIGTERM/SIGINT）
//! - 支持后台运行（detach 模式）
//!
//! ### 2. 服务器停止 (`stop()`)
//! - 读取 PID 文件获取进程 ID
//! - 发送 SIGTERM 信号优雅关闭
//! - 清理 PID 文件
//!
//! ### 3. API 密钥生成 (`keygen()`)
//! - 读取服务器密钥
//! - 生成 JWT 令牌
//! - 设置过期时间
//! - 转换为自定义 API 密钥格式
//!
//! ## 安全机制
//!
//! ### JWT 密钥管理
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                       密钥生成流程                               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  非开发模式                                                       │
//! │  ┌─────────────┐                                                 │
//! │  │ 检查密钥来源 │                                                 │
//! │  └──────┬──────┘                                                 │
//! │         │                                                        │
//! │    ┌────┴────┬─────────────┬─────────────┐                       │
//! │    │         │             │             │                       │
//! │    ▼         ▼             ▼             ▼                       │
//! │  --key   从文件读取    生成新密钥    reset_key                   │
//! │  参数     现有密钥       (随机 32 字节)  重置                       │
//! │    │         │             │             │                       │
//! │    └────┬────┴─────────────┴─────────────┘                       │
//! │         │                                                        │
//! │         ▼                                                        │
//! │  保存到 ~/.microsandbox/server.key                                │
//! │         │                                                        │
//! │         ▼                                                        │
//! │  用于 JWT 令牌签名和验证                                           │
//! │                                                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ### API 密钥格式
//!
//! ```text
//! API 密钥：msb_eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjE3MT...
//!            └──┘└───────────────────────────────────────────┘
//!           前缀                JWT 令牌 (header.payload.signature)
//!
//! JWT 结构:
//! - header: {"alg": "HS256", "typ": "JWT"}
//! - payload: {"exp": 过期时间，"iat": 签发时间}
//! - signature: HMAC-SHA256(header + "." + payload, server_key)
//! ```
//!
//! ## 进程管理
//!
//! ### PID 文件机制
//!
//! ```text
//! 启动流程:
//! 1. 检查 ~/.microsandbox/server.pid 是否存在
//! 2. 如果存在，读取 PID 并检查进程是否运行
//! 3. 如果进程运行中，拒绝重复启动
//! 4. 如果进程不存在，清理陈旧 PID 文件
//! 5. 启动新进程，写入新 PID
//!
//! 停止流程:
//! 1. 读取 ~/.microsandbox/server.pid
//! 2. 发送 SIGTERM 信号
//! 3. 删除 PID 文件
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_server::management;
//! use chrono::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 启动服务器（开发模式）
//! management::start(
//!     None,           // 无密钥（开发模式）
//!     Some("127.0.0.1".to_string()),
//!     Some(8080),
//!     None,           // 默认项目目录
//!     true,           // 开发模式
//!     false,          // 不后台运行
//!     false,          // 不重置密钥
//! ).await?;
//!
//! // 生成 API 密钥（24 小时过期）
//! let api_key = management::keygen(Some(Duration::hours(24))).await?;
//! println!("API Key: {}", api_key);
//!
//! // 停止服务器
//! management::stop().await?;
//! # Ok(())
//! # }
//! ```

use std::{path::PathBuf, process::Stdio};

use chrono::{Duration, Utc};
use jsonwebtoken::{EncodingKey, Header};
#[cfg(feature = "cli")]
use microsandbox_utils::term;
use microsandbox_utils::{
    DEFAULT_MSBSERVER_EXE_PATH, MSBSERVER_EXE_ENV_VAR, PROJECTS_SUBDIR, SERVER_KEY_FILE,
    SERVER_PID_FILE, env,
};
use rand::{Rng, distr::Alphanumeric};
use serde::{Deserialize, Serialize};
use tokio::{fs, process::Command};

use crate::{MicrosandboxServerError, MicrosandboxServerResult};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// API 密钥前缀
///
/// 所有生成的 API 密钥都以此前缀开头，便于：
/// - 识别密钥类型（微沙箱服务器密钥）
/// - 与其他系统的密钥区分
/// - 客户端验证密钥格式
pub const API_KEY_PREFIX: &str = "msb_";

/// 服务器密钥长度（字节）
///
/// 32 字节（256 位）的随机密钥足够安全：
/// - 穷举攻击不可行（2^256 种可能）
/// - 适合 HMAC-SHA256 算法
const SERVER_KEY_LENGTH: usize = 32;

#[cfg(feature = "cli")]
const START_SERVER_MSG: &str = "启动沙箱服务器";

#[cfg(feature = "cli")]
const STOP_SERVER_MSG: &str = "停止沙箱服务器";

#[cfg(feature = "cli")]
const KEYGEN_MSG: &str = "生成新 API 密钥";

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// # JWT 令牌声明结构
///
/// 此结构定义了 JWT 令牌中包含的信息。
///
/// ## 字段说明
///
/// ### `exp: u64`
/// 过期时间（Expiration time），Unix 时间戳（秒）。
/// 令牌在此时间之后失效。
///
/// ### `iat: u64`
/// 签发时间（Issued at），Unix 时间戳（秒）。
/// 记录令牌的创建时间。
///
/// ## 安全考虑
///
/// - 默认过期时间：24 小时
/// - 过期后需要重新生成令牌
/// - 不包含用户身份信息（服务器级别的认证）
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// 过期时间（Unix 时间戳）
    pub exp: u64,

    /// 签发时间（Unix 时间戳）
    pub iat: u64,
}

//--------------------------------------------------------------------------------------------------
// 函数定义
//--------------------------------------------------------------------------------------------------

/// # 启动沙箱服务器
///
/// 此函数负责启动微沙箱服务器进程，包括所有必要的初始化和配置。
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `key` | `Option<String>` | JWT 密钥（非开发模式下必需） |
/// | `host` | `Option<String>` | 监听地址（默认由服务器决定） |
/// | `port` | `Option<u16>` | 监听端口（默认由服务器决定） |
/// | `project_dir` | `Option<PathBuf>` | 项目目录（默认 ~/.microsandbox/projects） |
/// | `dev_mode` | `bool` | 是否启用开发模式（跳过认证） |
/// | `detach` | `bool` | 是否后台运行 |
/// | `reset_key` | `bool` | 是否重置现有密钥 |
///
/// ## 返回值
///
/// - `Ok(())`: 服务器已启动（或已运行）
/// - `Err(MicrosandboxServerError)`: 启动失败
///
/// ## 启动流程详解
///
/// ### 1. 目录准备
/// ```rust,ignore
/// // 确保 ~/.microsandbox 存在
/// fs::create_dir_all(&microsandbox_home_path).await?;
///
/// // 确保项目目录存在
/// fs::create_dir_all(&project_path).await?;
/// ```
///
/// ### 2. PID 文件检查
/// 检查是否有服务器已经在运行：
/// - 读取 server.pid 文件
/// - 检查进程是否存在（使用 kill(pid, 0)）
/// - 如果运行中：拒绝重复启动
/// - 如果不存在：清理陈旧 PID 文件
///
/// ### 3. 密钥处理（非开发模式）
/// ```text
/// 是否提供了 --key 参数？
/// ├─ 是 → 使用提供的密钥
/// │
/// └─ 否 → server.key 文件是否存在且 reset_key=false?
///         ├─ 是 → 读取现有密钥
///         │
///         └─ 否 → 生成新的随机密钥
///                 保存到 server.key
/// ```
///
/// ### 4. 进程启动
/// - 解析 msbserver 可执行文件路径
/// - 构建命令行参数
/// - 生成子进程
/// - 获取 PID
///
/// ### 5. PID 文件创建
/// - 将 PID 写入 ~/.microsandbox/server.pid
/// - 用于后续的 stop 命令和重复启动检测
///
/// ### 6. 信号处理（非 detach 模式）
/// - 注册 SIGTERM 处理器
/// - 注册 SIGINT 处理器（Ctrl+C）
/// - 等待信号或子进程退出
/// - 优雅关闭：清理 PID 文件
///
/// ## 运行模式
///
/// ### 前台模式（detach = false）
/// 服务器在前台运行，阻塞当前终端：
/// - 可以看到服务器日志输出
/// - Ctrl+C 可以停止服务器
/// - 适合开发和调试
///
/// ### 后台模式（detach = true）
/// 服务器在后台运行，立即返回：
/// - 使用 setsid() 创建新会话
/// - 标准输入输出重定向到 /dev/null
/// - 适合生产部署
///
/// ## 开发模式 vs 生产模式
///
/// | 特性 | 开发模式 | 生产模式 |
/// |------|----------|----------|
/// | JWT 密钥 | 不需要 | 必需 |
/// | 认证检查 | 跳过 | 启用 |
/// | 日志级别 | DEBUG | INFO |
/// | 使用场景 | 本地开发 | 生产部署 |
///
/// ## 安全警告
///
/// ⚠️ **不要在生产环境使用开发模式！**
///
/// 开发模式跳过所有认证检查，任何连接到服务器的人都可以：
/// - 启动和停止沙箱
/// - 执行任意代码
/// - 访问敏感数据
///
/// ## 示例
///
/// ```rust,no_run
/// use microsandbox_server::management;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // 开发模式启动（前台）
/// management::start(
///     None, None, None, None, true, false, false
/// ).await?;
///
/// // 生产模式启动（后台，带密钥）
/// management::start(
///     Some("my-super-secret-key-32bytes!".to_string()),
///     Some("127.0.0.1".to_string()),
///     Some(8080),
///     None,
///     false,  // 生产模式
///     true,   // 后台运行
///     false,  // 不重置密钥
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn start(
    key: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    project_dir: Option<PathBuf>,
    dev_mode: bool,
    detach: bool,
    reset_key: bool,
) -> MicrosandboxServerResult<()> {
    // ==================== 1. 目录准备 ====================
    // 确保微沙箱主目录存在（~/.microsandbox）
    let microsandbox_home_path = env::get_microsandbox_home_path();
    fs::create_dir_all(&microsandbox_home_path).await?;

    // 确保项目目录存在（~/.microsandbox/projects）
    let project_path = microsandbox_home_path.join(PROJECTS_SUBDIR);
    fs::create_dir_all(&project_path).await?;

    #[cfg(feature = "cli")]
    let start_server_sp = term::create_spinner(START_SERVER_MSG.to_string(), None, None);

    // ==================== 2. PID 文件检查 ====================
    // 检查 PID 文件路径
    let pid_file_path = microsandbox_home_path.join(SERVER_PID_FILE);

    if pid_file_path.exists() {
        // 读取 PID 文件
        let pid_str = fs::read_to_string(&pid_file_path).await?;
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            // 检查进程是否实际运行
            // kill(pid, 0) 不发送信号，只检查进程是否存在
            let process_running = unsafe { libc::kill(pid, 0) == 0 };

            if process_running {
                // 进程正在运行，拒绝重复启动
                #[cfg(feature = "cli")]
                term::finish_with_error(&start_server_sp);

                #[cfg(feature = "cli")]
                println!(
                    "沙箱服务器已经在运行 (PID: {}) - 使用 {} 停止它",
                    pid,
                    console::style("msb server stop").yellow()
                );

                tracing::info!(
                    "沙箱服务器已经在运行 (PID: {})。使用 'msb server stop' 停止它",
                    pid
                );

                return Ok(());
            } else {
                // 进程不存在，清理陈旧的 PID 文件
                tracing::warn!("发现陈旧的 PID 文件 (进程 {} 已不存在)。清理中。", pid);
                clean(&pid_file_path).await?;
            }
        } else {
            // PID 文件格式无效，清理
            tracing::warn!("发现无效的 PID 文件。清理中。");
            clean(&pid_file_path).await?;
        }
    }

    // ==================== 3. 准备服务器命令 ====================
    // 获取 msbserver 可执行文件路径
    let msbserver_path = microsandbox_utils::path::resolve_env_path(
        MSBSERVER_EXE_ENV_VAR,
        &*DEFAULT_MSBSERVER_EXE_PATH,
    )
    .inspect_err(|_e| {
        #[cfg(feature = "cli")]
        term::finish_with_error(&start_server_sp);
    })?;

    let mut command = Command::new(msbserver_path);

    // 添加命令行参数
    if dev_mode {
        command.arg("--dev");
    }

    if let Some(host) = host {
        command.arg("--host").arg(host);
    }

    if let Some(port) = port {
        command.arg("--port").arg(port.to_string());
    }

    if let Some(project_dir) = project_dir {
        command.arg("--path").arg(project_dir);
    }

    // ==================== 4. 密钥处理（非开发模式） ====================
    if !dev_mode {
        // 创建密钥文件路径
        let key_file_path = microsandbox_home_path.join(SERVER_KEY_FILE);

        // 记录是否提供了密钥（在消费 key 选项之前）
        let key_provided = key.is_some();

        // 确定使用哪个密钥
        let server_key = if let Some(key) = key {
            // 使用提供的密钥
            command.arg("--key").arg(&key);
            key
        } else if key_file_path.exists() && !reset_key {
            // 使用现有的密钥文件（如果存在且不重置）
            let existing_key = fs::read_to_string(&key_file_path).await.map_err(|e| {
                #[cfg(feature = "cli")]
                term::finish_with_error(&start_server_sp);

                MicrosandboxServerError::StartError(format!(
                    "无法读取现有密钥文件 {}: {}",
                    key_file_path.display(),
                    e
                ))
            })?;
            command.arg("--key").arg(&existing_key);
            existing_key
        } else {
            // 生成新的随机密钥
            let generated_key = generate_random_key();
            command.arg("--key").arg(&generated_key);
            generated_key
        };

        // 写入密钥文件（如果是新密钥或重置）
        if !key_file_path.exists() || key_provided || reset_key {
            fs::write(&key_file_path, &server_key).await.map_err(|e| {
                #[cfg(feature = "cli")]
                term::finish_with_error(&start_server_sp);

                MicrosandboxServerError::StartError(format!(
                    "无法写入密钥文件 {}: {}",
                    key_file_path.display(),
                    e
                ))
            })?;

            tracing::info!("在 {} 创建了服务器密钥文件", key_file_path.display());
        }
    }

    // ==================== 5. 后台运行配置 ====================
    if detach {
        // 创建新会话，使进程独立于控制终端
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        // 重定向标准输入输出到 /dev/null
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());
        command.stdin(Stdio::null());
    }

    // ==================== 6. 环境变量传递 ====================
    // 只在已设置时传递 RUST_LOG
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        tracing::debug!("使用现有的 RUST_LOG: {:?}", rust_log);
        command.env("RUST_LOG", rust_log);
    }

    // ==================== 7. 启动子进程 ====================
    let mut child = command.spawn().map_err(|e| {
        #[cfg(feature = "cli")]
        term::finish_with_error(&start_server_sp);

        MicrosandboxServerError::StartError(format!("无法生成服务器进程：{}", e))
    })?;

    // 获取子进程 PID
    let pid = child.id().unwrap_or(0);
    tracing::info!("启动了沙箱服务器进程，PID: {}", pid);

    // ==================== 8. 创建 PID 文件 ====================
    // 写入 PID 文件
    fs::write(&pid_file_path, pid.to_string())
        .await
        .map_err(|e| {
            #[cfg(feature = "cli")]
            term::finish_with_error(&start_server_sp);

            MicrosandboxServerError::StartError(format!(
                "无法写入 PID 文件 {}: {}",
                pid_file_path.display(),
                e
            ))
        })?;

    #[cfg(feature = "cli")]
    start_server_sp.finish();

    // 如果是后台模式，立即返回
    if detach {
        return Ok(());
    }

    // ==================== 9. 设置信号处理器 ====================
    // SIGTERM: 优雅终止信号（kill 命令默认发送）
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| {
            #[cfg(feature = "cli")]
            term::finish_with_error(&start_server_sp);

            MicrosandboxServerError::StartError(format!("无法设置信号处理器：{}", e))
        })?;

    // SIGINT: 中断信号（Ctrl+C）
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| {
            #[cfg(feature = "cli")]
            term::finish_with_error(&start_server_sp);

            MicrosandboxServerError::StartError(format!("无法设置信号处理器：{}", e))
        })?;

    // ==================== 10. 等待事件 ====================
    // 使用 tokio::select! 等待多个异步事件中的任意一个
    tokio::select! {
        // 子进程退出
        status = child.wait() => {
            if !status.as_ref().is_ok_and(|s| s.success()) {
                tracing::error!(
                    "子进程 — 沙箱服务器 — 退出状态：{:?}",
                    status
                );

                // 清理 PID 文件
                clean(&pid_file_path).await?;

                #[cfg(feature = "cli")]
                term::finish_with_error(&start_server_sp);

                return Err(MicrosandboxServerError::StartError(format!(
                    "子进程 — 沙箱服务器 — 退出状态：{:?}",
                    status
                )));
            }

            // 正常退出，清理 PID 文件
            clean(&pid_file_path).await?;
        }
        // 收到 SIGTERM 信号
        _ = sigterm.recv() => {
            tracing::info!("收到 SIGTERM 信号");

            // 发送 SIGTERM 给子进程
            if let Err(e) = child.kill().await {
                tracing::error!("无法发送 SIGTERM 给子进程：{}", e);
            }

            // 等待子进程退出
            if let Err(e) = child.wait().await {
                tracing::error!("SIGTERM 后等待子进程出错：{}", e);
            }

            // 清理 PID 文件
            clean(&pid_file_path).await?;

            tracing::info!("服务器被 SIGTERM 信号终止");
        }
        // 收到 SIGINT 信号
        _ = sigint.recv() => {
            tracing::info!("收到 SIGINT 信号");

            // 发送 SIGTERM 给子进程
            if let Err(e) = child.kill().await {
                tracing::error!("无法发送 SIGTERM 给子进程：{}", e);
            }

            // 等待子进程退出
            if let Err(e) = child.wait().await {
                tracing::error!("SIGINT 后等待子进程出错：{}", e);
            }

            // 清理 PID 文件
            clean(&pid_file_path).await?;

            tracing::info!("服务器被 SIGINT 信号终止");
        }
    }

    Ok(())
}

/// # 停止沙箱服务器
///
/// 此函数优雅地停止正在运行的沙箱服务器。
///
/// ## 返回值
///
/// - `Ok(())`: 服务器已停止
/// - `Err(MicrosandboxServerError)`: 停止失败
///
/// ## 停止流程
///
/// 1. **检查 PID 文件**: 确认服务器是否在运行
/// 2. **读取 PID**: 从文件获取进程 ID
/// 3. **发送 SIGTERM**: 请求进程优雅关闭
/// 4. **清理 PID 文件**: 删除 server.pid
///
/// ## 错误情况
///
/// | 错误 | 原因 |
/// |------|------|
/// | "PID file not found" | 服务器未运行 |
/// | "invalid PID" | PID 文件格式错误 |
/// | "process not found" | 进程已不存在（陈旧 PID） |
/// | "failed to stop" | 发送信号失败 |
///
/// ## 示例
///
/// ```rust,no_run
/// use microsandbox_server::management;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// management::stop().await?;
/// println!("服务器已停止");
/// # Ok(())
/// # }
/// ```
pub async fn stop() -> MicrosandboxServerResult<()> {
    let microsandbox_home_path = env::get_microsandbox_home_path();
    let pid_file_path = microsandbox_home_path.join(SERVER_PID_FILE);

    #[cfg(feature = "cli")]
    let stop_server_sp = term::create_spinner(STOP_SERVER_MSG.to_string(), None, None);

    // ==================== 1. 检查 PID 文件 ====================
    if !pid_file_path.exists() {
        #[cfg(feature = "cli")]
        term::finish_with_error(&stop_server_sp);

        return Err(MicrosandboxServerError::StopError(
            "服务器未运行（未找到 PID 文件）".to_string(),
        ));
    }

    // ==================== 2. 读取 PID ====================
    let pid_str = fs::read_to_string(&pid_file_path).await?;
    let pid = pid_str.trim().parse::<i32>().map_err(|_| {
        MicrosandboxServerError::StopError("PID 文件格式无效".to_string())
    })?;

    // ==================== 3. 发送 SIGTERM ====================
    unsafe {
        // 发送 SIGTERM 信号
        if libc::kill(pid, libc::SIGTERM) != 0 {
            // 检查错误原因
            if std::io::Error::last_os_error().raw_os_error().unwrap() == libc::ESRCH {
                // ESRCH = No such process
                // 进程不存在，清理陈旧 PID 文件
                clean(&pid_file_path).await?;

                #[cfg(feature = "cli")]
                term::finish_with_error(&stop_server_sp);

                return Err(MicrosandboxServerError::StopError(
                    "服务器进程不存在（已清理陈旧 PID 文件）".to_string(),
                ));
            }

            #[cfg(feature = "cli")]
            term::finish_with_error(&stop_server_sp);

            return Err(MicrosandboxServerError::StopError(format!(
                "无法停止服务器进程 (PID: {})",
                pid
            )));
        }
    }

    // ==================== 4. 清理 PID 文件 ====================
    clean(&pid_file_path).await?;

    #[cfg(feature = "cli")]
    stop_server_sp.finish();

    tracing::info!("已停止沙箱服务器进程 (PID: {})", pid);

    Ok(())
}

/// # 生成新的 API 密钥
///
/// 此函数生成一个带有过期时间的 JWT API 密钥。
///
/// ## 参数说明
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `expire` | `Option<Duration>` | 令牌过期时间，None 表示默认 24 小时 |
///
/// ## 返回值
///
/// - `Ok(String)`: API 密钥（格式：`msb_<jwt_token>`）
/// - `Err(MicrosandboxServerError)`: 生成失败
///
/// ## 生成流程
///
/// 1. **检查密钥文件**: 确认服务器密钥存在
/// 2. **读取密钥**: 从 server.key 读取
/// 3. **创建声明**: 设置 exp 和 iat
/// 4. **签名 JWT**: 使用 HS256 算法
/// 5. **添加前缀**: 转换为 `msb_<jwt>` 格式
///
/// ## JWT 结构
///
/// ```json
/// // Header
/// {
///     "alg": "HS256",
///     "typ": "JWT"
/// }
///
/// // Payload (Claims)
/// {
///     "exp": 1710864000,  // 过期时间（Unix 时间戳）
///     "iat": 1710777600   // 签发时间（Unix 时间戳）
/// }
///
/// // Signature
/// HMAC-SHA256(
///     base64(header) + "." + base64(payload),
///     server_key
/// )
/// ```
///
/// ## 示例
///
/// ```rust,no_run
/// use microsandbox_server::management;
/// use chrono::Duration;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // 生成 24 小时有效的密钥
/// let api_key = management::keygen(Some(Duration::hours(24))).await?;
/// println!("API Key: {}", api_key);
///
/// // 生成 7 天有效的密钥
/// let week_key = management::keygen(Some(Duration::days(7))).await?;
/// # Ok(())
/// # }
/// ```
pub async fn keygen(expire: Option<Duration>) -> MicrosandboxServerResult<String> {
    let microsandbox_home_path = env::get_microsandbox_home_path();
    let key_file_path = microsandbox_home_path.join(SERVER_KEY_FILE);

    #[cfg(feature = "cli")]
    let keygen_sp = term::create_spinner(KEYGEN_MSG.to_string(), None, None);

    // ==================== 1. 检查密钥文件 ====================
    if !key_file_path.exists() {
        #[cfg(feature = "cli")]
        term::finish_with_error(&keygen_sp);

        return Err(MicrosandboxServerError::KeyGenError(
            "未找到服务器密钥文件。请确保服务器以安全模式运行。".to_string(),
        ));
    }

    // ==================== 2. 读取服务器密钥 ====================
    let server_key = fs::read_to_string(&key_file_path).await.map_err(|e| {
        #[cfg(feature = "cli")]
        term::finish_with_error(&keygen_sp);

        MicrosandboxServerError::KeyGenError(format!(
            "无法读取服务器密钥文件 {}: {}",
            key_file_path.display(),
            e
        ))
    })?;

    // ==================== 3. 确定过期时间 ====================
    // 默认 24 小时
    let expire = expire.unwrap_or(Duration::hours(24));

    // ==================== 4. 创建 JWT 声明 ====================
    let now = Utc::now();
    let expiry = now + expire;

    let claims = Claims {
        exp: expiry.timestamp() as u64,
        iat: now.timestamp() as u64,
    };

    // ==================== 5. 签名 JWT ====================
    let jwt_token = jsonwebtoken::encode(
        &Header::default(),  // 默认 header: {"alg": "HS256", "typ": "JWT"}
        &claims,
        &EncodingKey::from_secret(server_key.as_bytes()),
    )
    .map_err(|e| {
        #[cfg(feature = "cli")]
        term::finish_with_error(&keygen_sp);

        MicrosandboxServerError::KeyGenError(format!("无法生成令牌：{}", e))
    })?;

    // ==================== 6. 转换为 API 密钥格式 ====================
    let custom_token = convert_jwt_to_api_key(&jwt_token)?;

    // 存储用于输出的信息
    let token_str = custom_token.clone();
    let expiry_str = expiry.to_rfc3339();

    #[cfg(feature = "cli")]
    keygen_sp.finish();

    tracing::info!("生成了 API 令牌，过期时间 {}", expiry_str);

    #[cfg(feature = "cli")]
    {
        println!("令牌：{}", console::style(&token_str).cyan());
        println!("令牌过期时间：{}", console::style(&expiry_str).cyan());
    }

    Ok(token_str)
}

/// # 清理 PID 文件
///
/// 删除服务器 PID 文件，用于正常关闭或清理陈旧文件。
///
/// ## 参数
///
/// | 参数 | 类型 | 说明 |
/// |------|------|------|
/// | `pid_file_path` | `&PathBuf` | PID 文件路径 |
pub async fn clean(pid_file_path: &PathBuf) -> MicrosandboxServerResult<()> {
    // 清理 PID 文件
    if pid_file_path.exists() {
        fs::remove_file(pid_file_path).await?;
        tracing::info!("已删除服务器 PID 文件 {}", pid_file_path.display());
    }

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// # 生成随机服务器密钥
///
/// 生成一个指定长度的随机字符串，用于 JWT 签名。
///
/// ## 实现细节
///
/// - 使用 `rand::rng()` 获取密码学安全的随机数生成器
/// - 从字母数字字符集（A-Z, a-z, 0-9）中采样
/// - 生成指定长度的字符串
///
/// ## 安全性
///
/// 32 字节（256 位）的随机密钥：
/// - 穷举空间：62^32 ≈ 2^191，足够安全
/// - 适合 HMAC-SHA256 算法
fn generate_random_key() -> String {
    rand::rng()
        .sample_iter(&Alphanumeric)
        .take(SERVER_KEY_LENGTH)
        .map(char::from)
        .collect()
}

/// # 将 JWT 令牌转换为 API 密钥格式
///
/// 将标准 JWT 令牌转换为自定义的 API 密钥格式。
///
/// ## 格式说明
///
/// ```text
/// JWT 令牌：eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjE3MT...
/// API 密钥：msb_eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjE3MT...
///           └──┘
///          前缀
/// ```
///
/// ## 设计原因
///
/// 1. **标识**: 前缀 `msb_` 标识这是微沙箱的密钥
/// 2. **兼容**: 保持与标准 JWT 的兼容性
/// 3. **验证**: 便于客户端验证密钥格式
pub fn convert_jwt_to_api_key(jwt_token: &str) -> MicrosandboxServerResult<String> {
    Ok(format!("{}{}", API_KEY_PREFIX, jwt_token))
}
