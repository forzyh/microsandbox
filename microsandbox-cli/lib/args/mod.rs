//! # 命令行参数定义模块
//!
//! 本模块包含 microsandbox-cli 的所有命令行参数定义。
//! 使用 `clap` crate（Command Line Argument Parser）来解析命令行参数。
//!
//! ## 模块结构
//!
//! ```text
//! args/
//! ├── mod.rs      # 本文件：模块入口，导出子模块
//! ├── msb.rs      # 主命令 msb 的参数定义
//! ├── msbrun.rs   # msbrun 命令的参数定义
//! └── msbserver.rs # msbserver 命令的参数定义
//! ```
//!
//! ## Clap 库基础
//!
//! `clap` 是 Rust 生态系统中最流行的命令行解析库。
//! 它支持两种定义参数的方式：
//!
//! 1. **派生宏方式**（本代码使用）：通过 `#[derive(Parser)]` 从结构体自动生成解析器
//! 2. **Builder 方式**：手动构建参数定义（更灵活但更繁琐）
//!
//! ### 核心概念
//!
//! - `Parser`: 派生宏，为结构体生成 `parse()` 方法
//! - `Subcommand`: 派生宏，用于定义子命令（如 `git add`, `git commit`）
//! - `#[arg(...)]`: 字段属性，定义参数的行为

mod msb;
mod msbrun;
mod msbserver;

//--------------------------------------------------------------------------------------------------
// Exports - 导出
//--------------------------------------------------------------------------------------------------

// 将所有子模块的类型导出，使得外部代码可以通过 `use microsandbox_cli::args::Xxx` 访问
pub use msb::*;
pub use msbrun::*;
pub use msbserver::*;
