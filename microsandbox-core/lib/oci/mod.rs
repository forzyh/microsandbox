//! OCI（Open Container Initiative）模块
//!
//! 本模块提供了与容器注册表交互的功能，用于处理容器镜像。
//!
//! ## 主要功能
//!
//! - **镜像拉取** - 从 OCI 兼容的注册表拉取容器镜像
//! - **镜像引用解析** - 解析和验证镜像引用（标签和 digest）
//! - **清单和配置管理** - 处理镜像清单、配置和层
//! - **层处理** - 解压和管理镜像层
//!
//! ## OCI 标准简介
//!
//! OCI（Open Container Initiative）是容器行业的开放标准组织，定义了：
//! - **镜像格式** - 容器镜像的标准格式
//! - **清单（Manifest）** - 描述镜像元数据的 JSON 文件
//! - **层（Layer）** - 镜像的压缩文件系统快照
//! - **配置（Config）** - 镜像的运行配置（环境变量、命令等）
//!
//! ## 模块组成
//!
//! - **image** - 镜像处理核心逻辑
//! - **reference** - 镜像引用解析
//! - **registry** - 注册表交互
//! - **layer** - 层处理和解包
//! - **global_cache** - 全局镜像缓存管理

mod global_cache;
mod image;
mod layer;
#[cfg(test)]
pub(crate) mod mocks;
mod reference;
mod registry;
#[cfg(test)]
mod tests;

//--------------------------------------------------------------------------------------------------
// 导出
//--------------------------------------------------------------------------------------------------

pub(crate) use global_cache::*;
pub use image::*;
pub(crate) use layer::*;
pub use reference::*;
pub(crate) use registry::*;
