//! # microsandbox-portal 库入口
//!
//! 本库实现了 microsandbox 沙箱系统的门户（portal）功能，提供 JSON-RPC 接口
//! 用于在沙箱环境中执行代码和系统命令。
//!
//! ## 主要功能
//!
//! ### 1. REPL 代码执行
//! 支持多种编程语言的交互式代码执行：
//! - **Python**: 通过 Python 3 解释器执行
//! - **Node.js**: 通过 Node.js 运行时执行 JavaScript
//!
//! ### 2. 系统命令执行
//! 在受控环境中执行系统命令，支持：
//! - 命令参数传递
//! - 超时控制
//! - 标准输出/错误流捕获
//!
//! ### 3. JSON-RPC 接口
//! 提供标准的 JSON-RPC 2.0 接口：
//! - `sandbox.repl.run` - 执行代码
//! - `sandbox.command.run` - 执行命令
//!
//! ## 模块结构
//!
//! ```text
//! microsandbox_portal
//! ├── error       # 错误类型定义
//! ├── handler     # 请求处理函数
//! ├── payload     # JSON-RPC 数据结构
//! ├── portal      # 核心门户功能
//! │   ├── command # 命令执行引擎
//! │   ├── fs      # 文件系统操作（待实现）
//! │   └── repl    # REPL 引擎
//! │       ├── engine.rs  # 引擎管理
//! │       ├── types.rs   # 类型定义
//! │       ├── python.rs  # Python 引擎
//! │       └── nodejs.rs  # Node.js 引擎
//! ├── route       # 路由配置
//! └── state       # 共享状态管理
//! ```
//!
//! ## 特性标志 (Feature Flags)
//!
//! 本库支持以下可选特性：
//!
//! | 特性 | 说明 | 依赖 |
//! |------|------|------|
//! | `python` | 启用 Python REPL 支持 | python3 解释器 |
//! | `nodejs` | 启用 Node.js REPL 支持 | node 运行时 |
//!
//! ## 使用示例
//!
//! ### 启动门户服务器
//!
//! ```rust,no_run
//! use microsandbox_portal::{SharedState, create_router};
//!
//! #[tokio::main]
//! async fn main() {
//!     let state = SharedState::default();
//!     let app = create_router(state);
//!     // 使用 axum 启动服务器...
//! }
//! ```
//!
//! ### 执行 REPL 代码
//!
//! ```rust,no_run
//! use microsandbox_portal::portal::repl::{start_engines, Language};
//!
//! #[tokio::main]
//! async fn main() {
//!     let handle = start_engines().await.unwrap();
//!     let result = handle.eval("print('hello')", Language::Python, "id1", Some(30)).await;
//! }
//! ```

#![warn(missing_docs)]

//--------------------------------------------------------------------------------------------------
// 模块声明 (Modules)
//--------------------------------------------------------------------------------------------------

/// 错误处理模块 - 定义 PortalError 类型和 IntoResponse 实现
pub mod error;

/// 请求处理模块 - 实现 JSON-RPC 请求处理函数
pub mod handler;

/// 载荷模块 - 定义 JSON-RPC 请求/响应结构
pub mod payload;

/// 门户模块 - 核心门户功能（REPL、命令执行等）
pub mod portal;

/// 路由模块 - Axum 路由器配置
pub mod route;

/// 状态模块 - 共享状态管理
pub mod state;

//--------------------------------------------------------------------------------------------------
// 公开导出 (Exports)
//--------------------------------------------------------------------------------------------------

// 公开所有子模块的内容，使调用者可以直接使用
// 例如：use microsandbox_portal::PortalError;

/// 公开错误类型
pub use error::*;

/// 公开处理函数
pub use handler::*;

/// 公开载荷类型
pub use payload::*;

/// 公开门户功能
pub use portal::*;

/// 公开路由函数
pub use route::*;

/// 公开状态类型
pub use state::*;
