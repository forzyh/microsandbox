//! VM 模块
//!
//! 本模块提供了微虚拟机（MicroVm）的管理和配置功能。
//! 基于 libkrun 库实现，用于创建安全隔离的虚拟机环境。
//!
//! ## 模块组成
//!
//! - **builder** - MicroVm 配置构建器
//! - **ffi** - 与 libkrun 库的 FFI 绑定
//! - **microvm** - MicroVm 核心实现
//! - **rlimit** - Linux 资源限制配置

mod builder;
mod ffi;
mod microvm;
mod rlimit;

//--------------------------------------------------------------------------------------------------
// 导出
//--------------------------------------------------------------------------------------------------

pub use builder::*;
#[allow(unused)]
pub use ffi::*;
pub use microvm::*;
pub use rlimit::*;
