//! # REPL 代码评估引擎模块 (Code Evaluation Engines)
//!
//! 本模块为多种编程语言提供统一的代码评估系统。
//! 它支持在沙箱环境中交互式地执行代码，并捕获输出结果。
//!
//! ## 架构概览
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      EngineHandle                           │
//! │                    (统一接口层)                              │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!              ┌───────────────┼───────────────┐
//!              ▼               ▼               ▼
//!     ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
//!     │   Python    │ │   Node.js   │ │    Rust     │
//!     │   Engine    │ │   Engine    │ │   Engine    │
//!     │  (python.rs)│ │ (nodejs.rs) │ │  (待实现)    │
//!     └─────────────┘ └─────────────┘ └─────────────┘
//! ```
//!
//! ## 主要组件
//!
//! ### EngineHandle (引擎句柄)
//! 统一的客户端接口，用于：
//! - 评估代码 (`eval` 方法)
//! - 关闭引擎 (`shutdown` 方法)
//! - 与后端引擎通信（通过消息传递）
//!
//! ### Engine Trait (引擎特质)
//! 每个语言引擎必须实现的接口：
//! - `initialize()` - 初始化引擎
//! - `eval()` - 评估代码
//! - `shutdown()` - 关闭引擎
//!
//! ### 类型定义 (types.rs)
//! - `Language` - 支持的语言枚举
//! - `Line` - 输出行结构
//! - `Stream` - 输出流类型（stdout/stderr）
//! - `EngineError` - 引擎错误类型
//!
//! ## 特性标志
//!
//! 本模块使用特性标志控制包含哪些语言引擎：
//!
//! | 特性 | 启用 | 说明 |
//! |------|------|------|
//! | `rust` | 可选 | 通过 evcxr 启用 Rust 代码评估 |
//! | `python` | 可选 | 启用 Python 代码评估 |
//! | `javascript` / `nodejs` | 可选 | 启用 JavaScript/Node.js 代码评估 |
//!
//! ## Reactor 模式
//!
//! 本系统采用 Reactor（反应器）模式：
//!
//! 1. **命令通道**: 客户端通过 channel 发送命令
//! 2. **反应器线程**: 中央线程接收并分发命令
//! 3. **语言引擎**: 各自处理特定语言的代码
//! 4. **响应通道**: 结果通过 channel 返回给客户端
//!
//! ## 使用示例
//!
//! ### 基本使用
//!
//! ```rust,no_run
//! use microsandbox_portal::repl::{start_engines, Language};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 启动所有启用的引擎
//!     let handle = start_engines().await?;
//!
//!     // 在不同语言中评估代码
//!     #[cfg(feature = "python")]
//!     let python_result = handle.eval(
//!         "print('Hello from Python')",
//!         Language::Python,
//!         "exec-001",
//!         Some(30)
//!     ).await?;
//!
//!     #[cfg(feature = "nodejs")]
//!     let js_result = handle.eval(
//!         "console.log('Hello from JavaScript')",
//!         Language::Node,
//!         "exec-002",
//!         Some(30)
//!     ).await?;
//!
//!     // 完成后关闭引擎
//!     handle.shutdown().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### 输出处理
//!
//! 每次评估返回 `Vec<Line>`，包含所有输出行：
//!
//! ```rust
//! use microsandbox_portal::repl::{Line, Stream};
//!
//! fn process_output(lines: Vec<Line>) {
//!     for line in lines {
//!         match line.stream {
//!             Stream::Stdout => println!("输出：{}", line.text),
//!             Stream::Stderr => eprintln!("错误：{}", line.text),
//!         }
//!     }
//! }
//! ```
//!
//! ## 线程安全设计
//!
//! 所有组件都设计为线程安全的：
//! - 使用消息传递（channel）进行线程间通信
//! - 共享状态使用 Arc<Mutex<T>> 保护
//! - 无共享可变状态，避免数据竞争

//--------------------------------------------------------------------------------------------------
// 模块导出 (Exports)
//--------------------------------------------------------------------------------------------------

/// Python 引擎实现 - 仅当启用 python 特性时
#[cfg(feature = "python")]
pub mod python;

/// Node.js 引擎实现 - 仅当启用 nodejs 特性时
#[cfg(feature = "nodejs")]
pub mod nodejs;

/// 引擎管理核心逻辑 - 始终包含
pub mod engine;

/// 类型定义 - 始终包含
pub mod types;

// 公开 engine 和 types 模块的所有公共内容
// 这样调用者可以直接使用 microsandbox_portal::repl::Language 等
pub use engine::*;
pub use types::*;
