//! 配置模块
//!
//! 本模块定义了 microsandbox 的配置类型和辅助函数。
//! 包含环境变量、路径映射、端口映射、镜像引用等配置结构。

mod env_pair;
mod microsandbox;
mod path_pair;
mod path_segment;
mod port_pair;
mod reference_path;

//--------------------------------------------------------------------------------------------------
// 导出
//--------------------------------------------------------------------------------------------------

pub use env_pair::*;
pub use microsandbox::*;
pub use path_pair::*;
pub use path_segment::*;
pub use port_pair::*;
pub use reference_path::*;
