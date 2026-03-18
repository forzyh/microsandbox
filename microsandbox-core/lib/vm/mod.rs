//! MicroVM（微虚拟机）模块
//!
//! 本模块提供了微虚拟机（MicroVm）的管理和配置功能。
//! 基于 libkrun 库实现，用于创建安全隔离的虚拟机环境。
//!
//! ## 什么是 MicroVM？
//!
//! MicroVM 是一种轻量级虚拟机，具有：
//! - **快速启动**: 毫秒级启动时间
//! - **低开销**: 最小的内存和 CPU 开销
//! - **高安全性**: 基于 KVM 的硬件隔离
//! - **容器兼容**: 支持 OCI 镜像和容器运行时
//!
//! ## 架构概览
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Microsandbox                         │
//! ├─────────────────────────────────────────────────────────┤
//! │  用户接口层：                                           │
//! │  • MicroVmBuilder - 流式 API 构建虚拟机配置              │
//! │  • MicroVmConfigBuilder - 底层配置构建器               │
//! │                                                         │
//! │  核心抽象层：                                           │
//! │  • MicroVm - 虚拟机实例封装                            │
//! │  • Rootfs - 根文件系统（Native/Overlayfs）              │
//! │  • LinuxRlimit - Linux 资源限制                        │
//! │                                                         │
//! │  系统接口层：                                           │
//! │  • FFI - libkrun 库的 Rust 绑定                         │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 模块组成
//!
//! | 子模块 | 说明 |
//! |--------|------|
//! | `builder` | MicroVm 配置构建器，提供流式 API |
//! | `ffi` | 与 libkrun 库的 FFI（外部函数接口）绑定 |
//! | `microvm` | MicroVm 核心实现，封装虚拟机实例 |
//! | `rlimit` | Linux 资源限制（CPU、内存、文件描述符等） |
//! | `errors` | MicroVM 配置错误类型定义 |
//!
//! ## Builder 模式（构建器模式）
//!
//! 本模块使用 Builder 模式来构建虚拟机配置：
//!
//! ```rust,no_run
//! use microsandbox_core::vm::{MicroVmBuilder, LogLevel, Rootfs};
//! use microsandbox_core::config::NetworkScope;
//! use std::path::PathBuf;
//!
//! # fn main() -> anyhow::Result<()> {
//! let vm = MicroVmBuilder::default()
//!     .log_level(LogLevel::Debug)
//!     .rootfs(Rootfs::Native(PathBuf::from("/tmp/rootfs")))
//!     .num_vcpus(2)
//!     .memory_mib(1024)
//!     .exec_path("/bin/sh")
//!     .args(["-c", "echo Hello"])
//!     .build()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## 类型状态模式（Type State Pattern）
//!
//! Builder 使用类型状态模式确保配置的正确性：
//! - `MicroVmConfigBuilder<R, E>`: R 和 E 是类型状态参数
//! - 编译时检查必需字段（rootfs, exec_path）是否已设置
//! - 防止构建不完整的配置

mod builder;
mod ffi;
mod microvm;
mod rlimit;
pub mod errors;

//--------------------------------------------------------------------------------------------------
// 导出
//--------------------------------------------------------------------------------------------------

pub use builder::*;
#[allow(unused)]
pub use ffi::*;
pub use microvm::*;
pub use rlimit::*;
pub use errors::InvalidMicroVMConfigError;
