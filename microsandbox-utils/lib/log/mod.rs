//! # 日志（log）模块
//!
//! 本模块包含 microsandbox 项目的日志相关工具。
//!
//! ## 子模块
//!
//! ### rotating
//! [`rotating`](self/rotating/) 子模块提供了日志轮转功能的实现。
//!
//! ## 什么是日志轮转（Log Rotation）？
//!
//! 日志轮转是一种自动管理日志文件大小的技术：
//! 1. 当日志文件达到预设的最大大小时，触发轮转
//! 2. 将当前日志文件重命名为 `.old` 后缀（备份）
//! 3. 创建新的空日志文件继续写入
//!
//! ## 为什么需要日志轮转？
//!
//! - **防止磁盘占用过大**: 无限增长的日志会占用大量磁盘空间
//! - **便于日志管理**: 分块的日志文件更容易查看、备份和删除
//! - **保留历史记录**: `.old` 文件保留了上一轮的日志，便于问题排查
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::log::RotatingLog;
//! use tokio::io::AsyncWriteExt;
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     // 创建轮转日志，使用默认最大大小（10MB）
//!     let mut log = RotatingLog::new("app.log").await?;
//!
//!     // 写入日志
//!     log.write_all(b"这是一条日志\n").await?;
//!
//!     // 或者获取同步写入器
//!     let mut sync_writer = log.get_sync_writer();
//!     use std::io::Write;
//!     sync_writer.write_all(b"同步写入的日志\n")?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 模块导出
//!
//! 本模块将 `rotating` 子模块的所有公共内容导出到父模块，
//! 使得使用者可以直接通过 `microsandbox_utils::log::RotatingLog` 访问。

mod rotating;

//--------------------------------------------------------------------------------------------------
// 模块导出
//--------------------------------------------------------------------------------------------------

// 导出 rotating 模块的所有公共类型和函数
// 这样使用者可以直接通过 microsandbox_utils::log::XXX 访问，而不需要写 microsandbox_utils::log::rotating::XXX
pub use rotating::*;
