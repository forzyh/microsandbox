//! # microsandbox Portal 主程序
//!
//! 这是 microsandbox portal 服务的主入口点。
//!
//! ## 功能概述
//!
//! 此二进制程序启动一个 JSON-RPC 服务器，可以处理通用的门户操作。
//! 它作为 microsandbox portal 服务的主要入口点。
//!
//! ## 主要组件
//!
//! ### 1. HTTP 服务器
//! 使用 Axum 框架提供的 Web 服务器：
//! - 监听指定端口（默认 4444）
//! - 处理健康检查请求（GET /health）
//! - 处理 JSON-RPC 请求（POST /api/v1/rpc）
//!
//! ### 2. REPL 引擎
//! 支持多种编程语言的代码执行：
//! - Python（通过 `python` 特性启用）
//! - Node.js（通过 `nodejs` 特性启用）
//!
//! ### 3. 信号处理
//! 优雅地处理关闭信号：
//! - Ctrl+C (SIGINT)
//! - SIGTERM (Unix)
//!
//! ## 启动流程
//!
//! ```text
//! 1. 初始化 tracing 日志系统
//!         │
//!         ▼
//! 2. 解析命令行参数
//!         │
//!         ▼
//! 3. 启动 REPL 引擎
//!         │
//!         ▼
//! 4. 创建 Axum 路由器
//!         │
//!         ▼
//! 5. 绑定端口并启动服务器
//!         │
//!         ▼
//! 6. 等待关闭信号
//!         │
//!         ▼
//! 7. 优雅关闭（清理引擎资源）
//! ```
//!
//! ## 命令行参数
//!
//! ```bash
//! portal [OPTIONS]
//!
//! Options:
//!   -p, --port <PORT>  监听端口 [默认：4444]
//!   -h, --help         打印帮助信息
//!   -V, --version      打印版本信息
//! ```
//!
//! ## 使用示例
//!
//! ```bash
//! # 使用默认端口启动
//! cargo run --bin portal
//!
//! # 指定端口启动
//! cargo run --bin portal -- --port 8080
//!
//! # 启用所有语言特性
//! cargo run --bin portal --features "python nodejs"
//! ```
//!
//! ## API 端点
//!
//! | 方法 | 路径 | 说明 |
//! |------|------|------|
//! | GET | /health | 健康检查 |
//! | POST | /api/v1/rpc | JSON-RPC 请求 |
//!
//! ## JSON-RPC 方法
//!
//! | 方法 | 说明 |
//! |------|------|
//! | sandbox.repl.run | 在 REPL 中执行代码 |
//! | sandbox.command.run | 执行系统命令 |

use anyhow::Result;
use clap::Parser;
use microsandbox_utils::DEFAULT_PORTAL_GUEST_PORT;
use std::{
    net::SocketAddr,
    sync::{Arc, atomic::Ordering},
};
use tokio::{net::TcpListener, signal};

use microsandbox_portal::{
    portal::repl::{EngineHandle, start_engines},
    route::create_router,
    state::SharedState,
};

//--------------------------------------------------------------------------------------------------
// 常量 (Constants)
//--------------------------------------------------------------------------------------------------

/// 默认监听地址
///
/// `0.0.0.0` 表示监听所有网络接口：
/// - 可以接受来自任何网络适配器的连接
/// - 包括 localhost 和外部网络
///
/// 如果只想接受本地连接，可以使用 `127.0.0.1`
const DEFAULT_HOST: &str = "0.0.0.0";

//--------------------------------------------------------------------------------------------------
// 类型 (Types)
//--------------------------------------------------------------------------------------------------

/// # Portal 命令行参数
///
/// 使用 Clap crate 定义命令行参数结构。
///
/// ## Clap 派生宏说明
///
/// `#[derive(Parser)]` 自动生成：
/// - 参数解析代码
/// - 帮助信息
/// - 错误处理
///
/// ## 属性说明
///
/// * `#[command(name = "portal")]` - 程序名称
/// * `#[command(author)]` - 从 Cargo.toml 获取作者信息
/// * `#[command(about = "...")]` - 程序描述
///
/// ## 字段属性
///
/// * `#[arg(short, long)]` - 同时支持短选项（-p）和长选项（--port）
///
/// ## 使用示例
///
/// ```bash
/// # 显示帮助
/// portal --help
///
/// # 指定端口
/// portal --port 8080
/// portal -p 8080
/// ```
#[derive(Debug, Parser)]
#[command(name = "portal", author, about = "microsandbox 的 JSON-RPC 门户服务")]
struct PortalArgs {
    /// 监听端口号
    ///
    /// 如果不指定，使用默认端口 4444
    #[arg(short, long)]
    port: Option<u16>,
}

//--------------------------------------------------------------------------------------------------
// 函数 (Functions)
//--------------------------------------------------------------------------------------------------

/// # 关闭信号处理器
///
/// 此异步函数等待并处理系统关闭信号。
///
/// ## 支持的信号
///
/// ### Unix 系统
/// - **SIGINT**: Ctrl+C 产生的中断信号
/// - **SIGTERM**: 终止信号（kill 命令默认发送）
///
/// ### 所有平台
/// - **Ctrl+C**: Windows 和 Unix 都支持
///
/// ## 参数说明
///
/// * `engine_handle` - 可选的引擎句柄，用于关闭前清理 REPL 引擎
///
/// ## 关闭流程
///
/// 1. 等待关闭信号（Ctrl+C 或 SIGTERM）
/// 2. 记录日志消息
/// 3. 如果存在引擎句柄，关闭 REPL 引擎
/// 4. 记录关闭完成消息
///
/// ## tokio::select! 说明
///
/// `tokio::select!` 宏等待多个异步操作中任何一个完成：
/// - 这里用于同时等待 Ctrl+C 和 SIGTERM
/// - 哪个信号先到就处理哪个
/// - 未完成的分支被自动取消
///
/// ## 使用示例
///
/// ```rust,no_run
/// let engine_handle = ...;  // 获取引擎句柄
/// shutdown_signal(engine_handle).await;  // 等待关闭信号
/// // 函数返回时表示已收到关闭信号
/// ```
async fn shutdown_signal(engine_handle: Option<EngineHandle>) {
    // ================================================================================
    // 步骤 1: 创建 Ctrl+C 信号监听器
    // ================================================================================
    // signal::ctrl_c() 返回一个 Future，在收到 Ctrl+C 时完成
    // expect() 在安装处理器失败时 panic（通常不会发生）
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    // ================================================================================
    // 步骤 2: 创建 SIGTERM 信号监听器（仅 Unix）
    // ================================================================================
    // #[cfg(unix)] 条件编译，只在 Unix 系统上包含此代码
    #[cfg(unix)]
    let terminate = async {
        // signal::unix::signal() 创建 Unix 信号处理器
        // SignalKind::terminate() 对应 SIGTERM 信号
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")  // 安装失败时 panic
            .recv()  // 等待信号
            .await;  // 异步等待信号到达
    };

    // 非 Unix 系统（如 Windows），使用永不会完成的 future
    // 这样 tokio::select! 只会等待 ctrl_c
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // ================================================================================
    // 步骤 3: 等待任一信号
    // ================================================================================
    // tokio::select! 等待多个 future 中第一个完成的
    // 这里等待 Ctrl+C 或 SIGTERM，哪个先到就执行哪个分支
    tokio::select! {
        _ = ctrl_c => {},      // Ctrl+C 到达
        _ = terminate => {},   // SIGTERM 到达
    }

    // ================================================================================
    // 步骤 4: 记录日志并开始清理
    // ================================================================================
    tracing::info!("Shutdown signal received, cleaning up...");

    // ================================================================================
    // 步骤 5: 关闭引擎（如果存在）
    // ================================================================================
    // 引擎句柄可能为 None（如果引擎启动失败）
    if let Some(handle) = engine_handle {
        // handle.shutdown() 异步关闭所有引擎
        // 发送关闭命令到引擎反应器线程
        if let Err(e) = handle.shutdown().await {
            // 关闭失败，记录错误日志
            tracing::error!("Error shutting down engines: {}", e);
        } else {
            // 关闭成功，记录信息日志
            tracing::info!("Engines shutdown successfully");
        }
    }

    // ================================================================================
    // 步骤 6: 记录关闭完成消息
    // ================================================================================
    tracing::info!("Server shutdown complete");
}

/// # 主函数
///
/// 这是 portal 服务的入口点。
///
/// ## 执行流程
///
/// 1. 初始化 tracing 日志系统
/// 2. 解析命令行参数
/// 3. 确定监听地址
/// 4. 初始化共享状态
/// 5. 启动 REPL 引擎
/// 6. 创建 Axum 路由器
/// 7. 绑定端口并启动服务器
/// 8. 等待关闭信号
/// 9. 优雅关闭
///
/// ## 返回值
///
/// * `Ok(())` - 服务器正常关闭
/// * `Err(anyhow::Error)` - 启动或运行期间发生错误
///
/// ## #[tokio::main] 说明
///
/// 此宏将 main 函数转换为异步运行时：
/// - 创建 tokio 运行时
/// - 运行异步 main 函数
/// - 处理返回值
///
/// ## 错误处理
///
/// 使用 `anyhow::Result` 简化错误处理：
/// - `?` 操作符自动传播错误
/// - 错误被转换为 anyhow::Error
#[tokio::main]
async fn main() -> Result<()> {
    // ================================================================================
    // 步骤 1: 初始化 tracing 日志系统
    // ================================================================================
    // tracing_subscriber::fmt::init() 初始化基础的格式化日志订阅器
    // 日志输出到 stdout，包含时间戳、日志级别、消息
    tracing_subscriber::fmt::init();

    // ================================================================================
    // 步骤 2: 解析命令行参数
    // ================================================================================
    // PortalArgs::parse() 使用 Clap 解析命令行参数
    // 支持 --port/-p 选项
    let args = PortalArgs::parse();

    // ================================================================================
    // 步骤 3: 确定监听地址
    // ================================================================================
    // 使用用户指定的端口或默认端口
    let port = args.port.unwrap_or(DEFAULT_PORTAL_GUEST_PORT);
    // 构建完整地址，如 "0.0.0.0:4444"
    let addr = format!("{}:{}", DEFAULT_HOST, port)
        .parse::<SocketAddr>()  // 解析为 SocketAddr 类型
        .unwrap();  // 格式错误时 panic（通常不会发生，因为我们是自己构建的字符串）

    // ================================================================================
    // 步骤 4: 初始化共享状态
    // ================================================================================
    // SharedState 包含服务器的全局状态：
    // - ready: 原子布尔值，表示服务器是否就绪
    // - engine_handle: REPL 引擎句柄
    // - command_handle: 命令执行器句柄
    let state = SharedState::default();
    // 克隆 engine_handle 的 Arc，用于后续关闭
    let engine_handle_for_shutdown = Arc::clone(&state.engine_handle);

    // ================================================================================
    // 步骤 5: 启动 REPL 引擎
    // ================================================================================
    // start_engines() 异步初始化所有启用的语言引擎
    match start_engines().await {
        // 引擎启动成功
        Ok(engine_handle) => {
            tracing::info!("REPL engines started successfully");
            // 将引擎句柄存储到共享状态中
            // *lock 解引用 MutexGuard 以修改内部值
            *engine_handle_for_shutdown.lock().await = Some(engine_handle.clone());
            *state.engine_handle.lock().await = Some(engine_handle);
            // 设置就绪标志，使用 Release 顺序确保之前的所有操作对其他线程可见
            state.ready.store(true, Ordering::Release);
        }
        // 引擎启动失败（可能因为 Python/Node.js 未安装）
        Err(e) => {
            tracing::warn!("Failed to start REPL engines: {}", e);
            // 继续运行，但 REPL 功能将不可用
            // 命令执行功能仍然可用
        }
    }

    // ================================================================================
    // 步骤 6: 记录启动日志
    // ================================================================================
    tracing::info!("Starting microsandbox portal server on {}", addr);

    // ================================================================================
    // 步骤 7: 创建 Axum 路由器
    // ================================================================================
    // create_router() 配置所有 HTTP 端点：
    // - GET /health
    // - POST /api/v1/rpc
    let app = create_router(state);

    // ================================================================================
    // 步骤 8: 准备关闭处理
    // ================================================================================
    // 克隆引擎句柄用于关闭信号处理
    // .lock().await 异步获取互斥锁
    let engine_handle_clone = engine_handle_for_shutdown.lock().await.clone();

    // ================================================================================
    // 步骤 9: 绑定端口并启动服务器
    // ================================================================================
    // TcpListener::bind() 绑定到指定地址
    let listener = TcpListener::bind(addr).await?;
    // axum::serve() 启动 HTTP 服务器
    // .with_graceful_shutdown() 设置优雅关闭处理器
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(engine_handle_clone))
        .await?;  // 等待服务器关闭

    Ok(())
}
