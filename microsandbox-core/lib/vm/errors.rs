//! MicroVM 配置错误类型
//!
//! 本模块定义了 MicroVM 配置无效时可能发生的错误类型。

use std::path::PathBuf;
use thiserror::Error;
use typed_path::Utf8UnixPathBuf;

/// MicroVM 配置无效时的详细错误
///
/// 此枚举提供了更具体的 MicroVM 配置错误原因，
/// 用于诊断和修复配置问题。
///
/// ## 变体说明
/// * `RootPathDoesNotExist` - 根路径不存在
/// * `MemoryIsZero` - 分配的内存为零
/// * `ExecutablePathDoesNotExist` - 可执行文件路径不存在
/// * `InvalidCommandLineString` - 命令行包含无效字符
/// * `ConflictingGuestPaths` - 访客路径冲突
#[derive(Debug, Error)]
pub enum InvalidMicroVMConfigError {
    /// MicroVm 的根路径不存在
    ///
    /// 创建 VM 时指定的根文件系统路径在主机上不存在
    #[error("根路径 {0} 不存在")]
    RootPathDoesNotExist(PathBuf),

    /// 指定的内存为零
    ///
    /// VM 必须分配至少 1 MiB 的内存
    #[error("指定的内存为零")]
    MemoryIsZero,

    /// 指定的可执行文件路径不存在
    ///
    /// VM 内要运行的可执行文件路径在根文件系统中不存在
    #[error("可执行文件路径 {0} 不存在")]
    ExecutablePathDoesNotExist(Utf8UnixPathBuf),

    /// 命令行字符串包含无效字符
    ///
    /// 命令行（可执行文件路径和参数）只能包含可打印 ASCII 字符
    /// （空格 0x20 到波浪线 0x7E 之间）
    #[error("命令行字符串 '{0}' 包含无效字符")]
    InvalidCommandLineString(String),

    /// 访客路径冲突
    ///
    /// 当两个挂载点的路径重叠时发生，例如：
    /// - /app 和 /app/data（子集关系）
    /// - /var 和 /var/log（父子关系）
    #[error("访客路径 {0} 和 {1} 冲突")]
    ConflictingGuestPaths(String, String),
}
