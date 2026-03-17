//! # microsandbox-cli 库 crate
//!
//! 本 crate 是 microsandbox 项目的命令行接口库。
//! 它提供了参数解析、错误处理、终端样式等功能。
//!
//! ## 模块结构
//!
//! ```text
//! lib/
//! ├── lib.rs      # 本文件：库入口，导出子模块
//! ├── args/       # 命令行参数定义
//! │   ├── mod.rs
//! │   ├── msb.rs      # msb 命令参数
//! │   ├── msbrun.rs   # msbrun 命令参数
//! │   └── msbserver.rs # msbserver 命令参数
//! ├── error.rs    # 错误类型定义
//! └── styles.rs   # ANSI 终端样式
//! ```
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! use microsandbox_cli::{MicosandboxArgs, MicrosandboxCliResult};
//!
//! // 解析命令行参数
//! let args = MicrosandboxArgs::parse();
//!
//! // 处理命令
//! match args.subcommand {
//!     Some(cmd) => handle_command(cmd),
//!     None => print_help(),
//! }
//! ```

//--------------------------------------------------------------------------------------------------
// 模块声明
//--------------------------------------------------------------------------------------------------

mod args;
mod error;
mod styles;

//--------------------------------------------------------------------------------------------------
// Exports - 导出
//--------------------------------------------------------------------------------------------------

// 将所有子模块的类型导出，使得外部代码可以直接使用：
// use microsandbox_cli::MicosandboxArgs;
// 而不是：
// use microsandbox_cli::args::MicosandboxArgs;
pub use args::*;
pub use error::*;
pub use styles::*;
