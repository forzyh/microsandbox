//! # microsandbox-utils 工具库
//!
//! `microsandbox-utils` 是 microsandbox 项目的通用工具库，提供了各种辅助功能和工具函数。
//!
//! ## 模块概述
//!
//! 本库包含以下核心模块：
//!
//! - **defaults**: 定义整个项目使用的默认配置值和常量
//! - **env**: 环境变量读取和处理的工具函数
//! - **error**: 统一的错误类型定义和错误处理机制
//! - **log**: 日志轮转等日志相关功能
//! - **path**: 路径规范化、验证和解析工具
//! - **runtime**: 运行时工具，包括进程监控和管理
//! - **seekable**: 可异步定位 (seek) 的读写 trait 和工具
//! - **term**: 终端检测、进度条显示等终端交互功能
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::defaults::{DEFAULT_NUM_VCPUS, DEFAULT_MEMORY_MIB};
//! use microsandbox_utils::path::normalize_path;
//!
//! // 使用默认配置
//! let vcpus = DEFAULT_NUM_VCPUS;  // 默认 1 个 vCPU
//! let memory = DEFAULT_MEMORY_MIB; // 默认 1024 MiB 内存
//!
//! // 规范化路径
//! let normalized = normalize_path("/data/./app", path::SupportedPathType::Absolute).unwrap();
//! assert_eq!(normalized, "/data/app");
//! ```
//!
//! ## 错误处理
//!
//! 本库使用自定义的 [`MicrosandboxUtilsError`](error/enum.MicrosandboxUtilsError.html) 类型，
//! 支持从标准库 IO 错误、nix 错误等多种错误类型自动转换。
//!
//! ```rust
//! use microsandbox_utils::{MicrosandboxUtilsResult, MicrosandboxUtilsError};
//!
//! fn my_function() -> MicrosandboxUtilsResult<()> {
//!     // 可以直接使用 `?` 操作符，IO 错误会自动转换
//!     std::fs::read_to_string("file.txt")?;
//!     Ok(())
//! }
//! ```

#![warn(missing_docs)]
#![allow(clippy::module_inception)]

// 声明各个子模块
pub mod defaults;  // 默认配置常量
pub mod env;       // 环境变量工具
pub mod error;     // 错误类型定义
pub mod log;       // 日志轮转功能
pub mod path;      // 路径处理工具
pub mod runtime;   // 运行时进程管理
pub mod seekable;  // 可定位读写 trait
pub mod term;      // 终端交互工具

//--------------------------------------------------------------------------------------------------
// 模块导出
//--------------------------------------------------------------------------------------------------
// 使用 `pub use` 将所有子模块的内容导出到库的根级别，
// 这样使用者可以直接通过 `microsandbox_utils::XXX` 访问，而不需要写完整路径

pub use defaults::*;  // 导出所有默认配置常量
pub use env::*;       // 导出所有环境变量工具函数
pub use error::*;     // 导出错误类型和结果类型
pub use log::*;       // 导出日志相关类型
pub use path::*;      // 导出路径处理函数和常量
pub use runtime::*;   // 导出运行时管理类型
pub use seekable::*;  // 导出可定位读写 trait
pub use term::*;      // 导出终端工具函数和常量
