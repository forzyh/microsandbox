//! # 应用状态模块 - 全局状态管理
//!
//! 本模块负责管理微沙箱服务器的全局应用状态，提供线程安全的状态共享机制。
//!
//! ## 为什么需要 AppState？
//!
//! 在 Web 服务器中，多个请求会并发处理。为了在请求之间共享数据（如配置、数据库连接、
//! 端口管理器等），需要一个线程安全的状态容器。Axum 框架通过 `State` 提取器提供对此
//! 状态的访问。
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      AppState                               │
//! │  ┌─────────────────────────────────────────────────────┐    │
//! │  │                   Arc<T>                             │    │
//! │  │  (Atomic Reference Counting - 原子引用计数)          │    │
//! │  │  • 线程安全的共享所有权                              │    │
//! │  │  • 克隆时只增加计数，不复制数据                       │    │
//! │  │  • 最后一个引用释放时自动删除数据                     │    │
//! │  └─────────────────────────────────────────────────────┘    │
//! │                           │                                   │
//! │           ┌───────────────┴───────────────┐                  │
//! │           │                               │                   │
//! │           ▼                               ▼                   │
//! │  ┌─────────────────┐            ┌─────────────────┐          │
//! │  │     Config      │            │   PortManager   │          │
//! │  │  (只读配置)      │            │  (可写状态)      │          │
//! │  │  Arc<Config>    │            │ Arc<RwLock<...>>│          │
//! │  └─────────────────┘            └─────────────────┘          │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 并发控制机制
//!
//! ### Arc (原子引用计数)
//! - **用途**: 实现多线程共享所有权
//! - **特点**: 克隆成本低（只增加计数），线程安全
//! - **限制**: 只能获得不可变引用
//!
//! ### RwLock (读写锁)
//! - **用途**: 允许多个读取者或一个写入者访问数据
//! - **特点**: 读多写少场景下性能优于 Mutex
//! - **使用**: `.read()` 获得不可变引用，`.write()` 获得可变引用
//!
//! ## Axum State 模式
//!
//! Axum 使用提取器模式访问应用状态：
//!
//! ```rust,no_run
//! use axum::{extract::State, routing::get, Router};
//! use microsandbox_server::AppState;
//!
//! // 处理器函数通过 State 提取器访问共享状态
//! async fn handler(State(state): State<AppState>) -> impl axum::response::IntoResponse {
//!     // 可以访问 state.config 和 state.port_manager
//!     let config = state.get_config();
//!     // ...
//! }
//!
//! // 路由配置时注入状态
//! let app = Router::new()
//!     .route("/", get(handler))
//!     .with_state(app_state);
//! ```

use std::sync::Arc;
use tokio::sync::RwLock;

// getset 宏库用于自动生成 getter 方法
// #[getset(get = "pub with_prefix")] 会为每个字段生成 pub fn get_<field_name>() 方法
use getset::Getters;

use crate::{
    ServerError, ServerResult,
    config::Config,
    port::{LOCALHOST_IP, PortManager},
};

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// # 应用状态结构体
///
/// 这是微沙箱服务器的核心状态容器，包含所有请求处理器需要访问的共享数据。
///
/// ## 字段详解
///
/// ### `config: Arc<Config>`
/// 服务器配置，使用 `Arc` 包装以实现线程安全的共享读取。
/// - **为什么用 Arc?** 配置在运行时通常不会改变，多个线程可以同时读取
/// - **访问方式**: `state.get_config()` 返回 `&Arc<Config>`
///
/// ### `port_manager: Arc<RwLock<PortManager>>`
/// 端口管理器，使用 `Arc<RwLock<T>>` 包装以支持并发读写。
/// - **为什么用 RwLock?** 端口分配需要修改状态（写），获取端口只需读取（读）
/// - **访问方式**:
///   - 读取：`state.get_port_manager().read().await`
///   - 写入：`state.get_port_manager().write().await`
///
/// ## 派生 trait 说明
///
/// ### `Clone`
/// `AppState` 实现了 `Clone`，但克隆成本很低：
/// - 只增加 `Arc` 的引用计数
/// - 不复制实际数据
/// - 克隆后的实例指向同一份数据
///
/// ```rust,ignore
/// // 克隆 AppState 是安全的，且成本低
/// let state1 = AppState::new(config, port_manager);
/// let state2 = state1.clone();  // 只增加 Arc 引用计数
/// ```
///
/// ### `Getters`
/// 自动生成以下 getter 方法：
/// - `get_config(&self) -> &Arc<Config>`
/// - `get_port_manager(&self) -> &Arc<RwLock<PortManager>>`
///
/// 使用 `with_prefix` 选项，方法名会带有 `get_` 前缀。
#[derive(Clone, Getters)]
#[getset(get = "pub with_prefix")]
pub struct AppState {
    /// 服务器配置
    ///
    /// 包含所有服务器运行时配置：
    /// - JWT 密钥
    /// - 监听地址
    /// - 项目目录
    /// - 开发模式开关
    ///
    /// 使用 `Arc` 包装，因为：
    /// 1. 配置在运行时通常是只读的
    /// 2. 多个请求需要同时访问配置
    /// 3. Arc 提供线程安全的共享所有权
    config: Arc<Config>,

    /// 端口管理器
    ///
    /// 负责为沙箱分配和管理网络端口。
    ///
    /// 使用 `Arc<RwLock<PortManager>>` 的原因：
    /// 1. `Arc`: 多个请求需要共享访问
    /// 2. `RwLock`: 支持并发读取（获取端口信息）和独占写入（分配/释放端口）
    /// 3. 异步安全：使用 `tokio::sync::RwLock` 而非 `std::sync::RwLock`
    port_manager: Arc<RwLock<PortManager>>,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl AppState {
    /// # 创建新的应用状态实例
    ///
    /// 此函数用于初始化服务器的全局状态。
    ///
    /// ## 参数说明
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `config` | `Arc<Config>` | 服务器配置（已用 Arc 包装） |
    /// | `port_manager` | `Arc<RwLock<PortManager>>` | 端口管理器（已用 Arc<RwLock> 包装） |
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    /// use tokio::sync::RwLock;
    /// use microsandbox_server::{AppState, Config, PortManager};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // 创建配置
    /// let config = Arc::new(Config::new(
    ///     Some("my-key".to_string()),
    ///     "127.0.0.1".to_string(),
    ///     8080,
    ///     None,
    ///     true,
    /// )?);
    ///
    /// // 创建端口管理器
    /// let port_manager = Arc::new(RwLock::new(
    ///     PortManager::new(config.get_project_dir()).await?
    /// ));
    ///
    /// // 创建应用状态
    /// let state = AppState::new(config, port_manager);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(config: Arc<Config>, port_manager: Arc<RwLock<PortManager>>) -> Self {
        Self {
            config,
            port_manager,
        }
    }

    /// # 获取沙箱的 Portal URL
    ///
    /// 此函数返回指定沙箱的 Portal 服务 URL。
    /// Portal 是运行在沙箱内部的子服务，提供代码执行等功能。
    ///
    /// ## 参数说明
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `sandbox_name` | `&str` | 沙箱名称 |
    ///
    /// ## 返回值
    ///
    /// - `Ok(String)`: 格式为 `http://127.0.0.1:<port>` 的 URL
    /// - `Err(ServerError::InternalError)`: 沙箱未分配端口
    ///
    /// ## 实现细节
    ///
    /// 1. **获取读锁**: 使用 `.read().await` 获取端口管理器的只读访问权
    /// 2. **查询端口**: 通过沙箱名称查找已分配的端口
    /// 3. **构建 URL**: 使用 `LOCALHOST_IP`（127.0.0.1）和端口构建完整 URL
    /// 4. **错误处理**: 如果未找到端口，返回内部错误
    ///
    /// ## Portal 架构说明
    ///
    /// ```text
    /// ┌─────────────────────────────────────────────────────────┐
    /// │                    外部请求                              │
    /// └─────────────────────────────────────────────────────────┘
    ///                          │
    ///                          ▼
    /// ┌─────────────────────────────────────────────────────────┐
    /// │              microsandbox-server (端口 8080)             │
    /// │  ┌─────────────────────────────────────────────────┐    │
    ///  │ │              PortManager                         │    │
    ///  │ │  sandboxes: {                                    │    │
    ///  │ │    "my-python-sandbox": 54321,                  │    │
    ///  │ │    "my-node-sandbox": 54322                      │    │
    ///  │ │  }                                               │    │
    ///  │ └─────────────────────────────────────────────────┘    │
    /// └─────────────────────────────────────────────────────────┘
    ///                          │
    ///         ┌────────────────┼────────────────┐
    ///         ▼                ▼                ▼
    /// ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
    /// │Sandbox A     │ │Sandbox B     │ │Sandbox C     │
    /// │127.0.0.1:54321│ │127.0.0.1:54322│ │127.0.0.1:54323│
    /// │ ┌──────────┐ │ │ ┌──────────┐ │ │ ┌──────────┐ │
    /// │ │  Portal  │ │ │ │  Portal  │ │ │ │  Portal  │ │
    /// │ │ :8080    │ │ │ │ :8080    │ │ │ │ :8080    │ │
    /// │ └──────────┘ │ │ └──────────┘ │ │ └──────────┘ │
    /// └──────────────┘ └──────────────┘ └──────────────┘
    /// ```
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// use microsandbox_server::AppState;
    ///
    /// # async fn example(state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    /// // 获取沙箱的 Portal URL
    /// let portal_url = state.get_portal_url_for_sandbox("my-sandbox").await?;
    /// println!("Portal URL: {}", portal_url);
    /// // 输出：Portal URL: http://127.0.0.1:54321
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_portal_url_for_sandbox(&self, sandbox_name: &str) -> ServerResult<String> {
        // 获取端口管理器的读锁
        // .read().await 是异步操作，会等待锁可用
        // 读锁允许多个读取者同时访问，但不允许写入
        let port_manager = self.port_manager.read().await;

        // 尝试获取沙箱的端口
        // get_port() 返回 Option<u16>，Some(port) 表示已分配端口
        if let Some(port) = port_manager.get_port(sandbox_name) {
            // 构建完整的 URL
            // LOCALHOST_IP 是 127.0.0.1
            // format! 宏用于字符串格式化，{} 是占位符
            Ok(format!("http://{}:{}", LOCALHOST_IP, port))
        } else {
            // 未找到端口分配，返回内部错误
            // 这通常表示沙箱尚未启动或已被停止
            Err(ServerError::InternalError(format!(
                "No portal port assigned for sandbox {}",
                sandbox_name
            )))
        }
    }
}
