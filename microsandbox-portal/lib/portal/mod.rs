//! # 门户核心模块 (Portal Core)
//!
//! 本模块提供了 microsandbox 环境与外部系统之间的接口。
//! 它处理 REPL 环境中的代码执行、命令执行和文件系统操作，
//! 所有操作都在受控的沙箱环境中进行。
//!
//! ## 核心组件
//!
//! 门户由几个子模块组成：
//!
//! ### `repl` - REPL 引擎模块
//! 提供多语言的交互式代码执行环境：
//! - **Python 引擎**: 通过 subprocess 运行 python3 -i
//! - **Node.js 引擎**: 通过 subprocess 运行 node REPL
//! - 支持状态保持（多次执行间共享变量）
//! - 支持超时控制
//!
//! ### `command` - 命令执行模块
//! 处理系统命令的沙箱化执行：
//! - 使用 tokio::process::Command 派生子进程
//! - 捕获 stdout 和 stderr 输出
//! - 支持命令超时和终止
//!
//! ### `fs` - 文件系统模块
//! （待实现）计划提供安全的文件系统操作：
//! - 文件读取/写入
//! - 目录遍历
//! - 权限控制
//!
//! ## 架构设计
//!
//! 门户系统采用模块化架构，每个子模块处理沙箱环境的特定方面。
//! 所有操作都以安全性为首要考虑。
//!
//! ## 特性标志
//!
//! 可以使用以下特性标志自定义门户功能：
//!
//! - `python`: 启用 Python REPL 支持
//! - `javascript` / `nodejs`: 启用 JavaScript/Node.js REPL 支持
//! - `rust`: 启用 Rust 代码执行支持
//!
//! ## 使用示例
//!
//! ### REPL 代码执行
//!
//! ```rust,no_run
//! use microsandbox_portal::repl::{start_engines, Language};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 初始化 REPL 引擎
//!     let engines = start_engines().await?;
//!
//!     // 执行 Python 代码
//!     #[cfg(feature = "python")]
//!     let result = engines.eval("print('Hello from microsandbox!')", Language::Python)?;
//!
//!     engines.shutdown().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### 命令执行
//!
//! ```rust,no_run
//! use microsandbox_portal::command::create_command_executor;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 创建命令执行器
//!     let cmd_handle = create_command_executor();
//!
//!     // 执行系统命令（命令，参数，超时）
//!     let (exit_code, output) = cmd_handle.execute("ls", vec!["-la".to_string()], None).await?;
//!
//!     // 处理输出
//!     for line in output {
//!         println!("[{}] {}",
//!                  if line.stream == microsandbox_portal::repl::Stream::Stdout { "stdout" } else { "stderr" },
//!                  line.text);
//!     }
//!
//!     Ok(())
//! }
//! ```

//--------------------------------------------------------------------------------------------------
// 模块导出 (Exports)
//--------------------------------------------------------------------------------------------------

/// 命令执行模块 - 系统命令的沙箱化执行
pub mod command;

/// 文件系统模块 - 安全的文件系统操作（待实现）
pub mod fs;

/// REPL 模块 - 多语言代码执行引擎
pub mod repl;
