//! # msbserver 可执行文件入口
//!
//! `msbserver` 是微沙箱的服务器组件，提供：
//!
//! 1. **REST API 接口**: 用于远程管理沙箱
//! 2. **MCP 服务**: Model Context Protocol，用于与 AI 模型集成
//! 3. **WebSocket 通信**: 实时推送沙箱状态更新
//!
//! ## 架构概览
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      msbserver                               │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐    │
//! │  │  REST API    │   │   MCP 服务    │   │  WebSocket   │    │
//! │  │  (HTTP)      │   │  (JSON-RPC)  │   │  (实时推送)   │    │
//! │  └──────┬───────┘   └──────┬───────┘   └──────┬───────┘    │
//! │         │                  │                  │            │
//! │         └──────────────────┼──────────────────┘            │
//! │                            │                                │
//! │                   ┌────────▼────────┐                       │
//! │                   │   AppState      │                       │
//! │                   │  (共享状态)      │                       │
//! │                   └────────┬────────┘                       │
//! │                            │                                │
//! │         ┌──────────────────┼──────────────────┐             │
//! │         │                  │                  │             │
//! │  ┌──────▼───────┐   ┌──────▼───────┐   ┌──────▼───────┐    │
//! │  │   Config     │   │ PortManager  │   │   Routes     │    │
//! │  │  (配置)       │   │ (端口管理)    │   │  (路由)      │    │
//! │  └──────────────┘   └──────────────┘   └──────────────┘    │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 核心组件说明
//!
//! ### Config（配置）
//! - 服务器地址和端口
//! - JWT 密钥（用于 API 认证）
//! - 项目目录路径
//! - 开发模式标志
//!
//! ### PortManager（端口管理器）
//! - 跟踪已分配的端口
//! - 防止端口冲突
//! - 持久化端口分配状态
//!
//! ### AppState（应用状态）
//! - 在多个请求之间共享的状态
//! - 使用 Arc（原子引用计数）实现线程安全共享
//! - 使用 RwLock（读写锁）支持并发读写
//!
//! ## 使用示例
//!
//! ```bash
//! # 启动服务器（默认配置）
//! msbserver
//!
//! # 指定端口和主机
//! msbserver --host 0.0.0.0 --port 8080
//!
//! # 开发模式
//! msbserver --dev
//!
//! # 指定项目目录
//! msbserver --path /path/to/project
//!
//! # 设置 JWT 密钥
//! msbserver --key mysecretkey
//! ```
//!
//! ## 安全考虑
//!
//! ### JWT 认证
//! - 所有 API 请求需要携带 JWT 令牌
//! - 令牌通过 `Authorization: Bearer <token>` 头传递
//! - 密钥应该妥善保管，不要提交到版本控制
//!
//! ### CORS（跨域资源共享）
//! - 默认允许所有来源（开发模式）
//! - 生产环境应该限制允许的源

use std::sync::Arc;
use tokio::sync::RwLock;

use axum::http::{
    Method,
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
};
use clap::Parser;
use microsandbox_cli::{MicrosandboxCliResult, MsbserverArgs};
use microsandbox_server::{Config, port::PortManager, route, state::AppState};
use microsandbox_utils::CHECKMARK;
use tower_http::cors::{Any, CorsLayer};

//--------------------------------------------------------------------------------------------------
// Functions: Main - 主函数
//--------------------------------------------------------------------------------------------------

/// ## msbserver 主入口函数
///
/// 这是 msbserver 命令的异步主函数。
/// 使用 `axum` Web 框架提供 HTTP 服务。
///
/// ### Axum 框架简介
/// Axum 是 Tokio 团队开发的现代 Rust Web 框架：
/// - 基于 `hyper` 底层 HTTP 库
/// - 使用 `tower` 中间件生态
/// - 类型安全的路由和处理
/// - 与 Tokio 生态无缝集成
///
/// ### 架构流程
/// ```text
/// 1. 解析命令行参数
/// 2. 创建配置对象
/// 3. 初始化端口管理器
/// 4. 创建应用状态
/// 5. 配置 CORS 中间件
/// 6. 设置路由
/// 7. 启动 HTTP 服务器
/// 8. 等待并处理请求
/// ```
#[tokio::main]
pub async fn main() -> MicrosandboxCliResult<()> {
    // ======================================================================
    // 步骤 1: 初始化日志系统
    // ======================================================================
    // tracing_subscriber 是 Rust 的结构化日志库
    // fmt::init() 使用人类可读的格式输出日志
    tracing_subscriber::fmt::init();

    // ======================================================================
    // 步骤 2: 解析命令行参数
    // ======================================================================
    let args = MsbserverArgs::parse();

    // 如果是开发模式，输出提示信息
    if args.dev_mode {
        tracing::info!("Development mode: {}", args.dev_mode);
        println!(
            "{} Running in {} mode",
            &*CHECKMARK,
            console::style("development").yellow()
        );
    }

    // ======================================================================
    // 步骤 3: 创建配置对象
    // ======================================================================
    // Config 封装了服务器的所有配置项
    // Arc::new() 创建原子引用计数包装，允许多线程安全共享
    let config = Arc::new(Config::new(
        args.key,              // JWT 密钥（可选）
        args.host,             // 监听主机
        args.port,             // 监听端口
        args.project_dir.clone(), // 项目目录
        args.dev_mode,         // 开发模式标志
    )?);

    // 从配置中获取项目目录
    let project_dir = config.get_project_dir().clone();

    // ======================================================================
    // 步骤 4: 初始化端口管理器
    // ======================================================================
    // PortManager 负责：
    // 1. 跟踪沙箱使用的端口
    // 2. 自动分配可用端口
    // 3. 防止端口冲突
    //
    // 错误处理：如果初始化失败，打印错误消息并返回
    let port_manager = PortManager::new(project_dir).await.map_err(|e| {
        eprintln!("Error initializing port manager: {}", e);
        e
    })?;

    // 使用 Arc 和 RwLock 包装端口管理器
    // - Arc: 允许多个所有者共享数据
    // - RwLock: 读写锁，支持多读单写
    let port_manager = Arc::new(RwLock::new(port_manager));

    // ======================================================================
    // 步骤 5: 创建应用状态
    // ======================================================================
    // AppState 是 Axum 的"共享状态"机制
    // 所有请求处理函数都可以访问这个状态
    let state = AppState::new(config.clone(), port_manager);

    // ======================================================================
    // 步骤 6: 配置 CORS（跨域资源共享）
    // ======================================================================
    // CORS 是一种安全机制，控制网页是否可以跨域访问 API
    //
    // 配置的允许项：
    // - 方法：GET, POST, PUT, DELETE
    // - 头部：Authorization（认证）, Accept, Content-Type
    // - 源：Any（允许所有来源，生产环境应该限制）
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE])
        .allow_origin(Any);

    // ======================================================================
    // 步骤 7: 构建 Axum 应用
    // ======================================================================
    // create_router 设置所有 API 路由
    // .layer(cors) 添加 CORS 中间件
    let app = route::create_router(state).layer(cors);

    // ======================================================================
    // 步骤 8: 启动 HTTP 服务器
    // ======================================================================
    tracing::info!("Starting server on {}", config.get_addr());
    println!(
        "{} Server listening on {}",
        &*CHECKMARK,
        console::style(config.get_addr()).yellow()
    );

    // 绑定 TCP 监听器
    let listener = tokio::net::TcpListener::bind(config.get_addr()).await?;

    // 启动服务器，开始处理请求
    // axum::serve 会一直运行，直到被信号中断
    axum::serve(listener, app).await?;

    Ok(())
}
