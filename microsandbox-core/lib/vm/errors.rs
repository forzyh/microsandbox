//! MicroVM 配置错误类型
//!
//! 本模块定义了 MicroVM 配置无效时可能发生的错误类型。
//!
//! ## 错误类型设计
//!
//! 使用 `thiserror` 库定义错误枚举，提供：
//! 1. **友好的错误消息**: 使用 `#[error(...)]` 宏格式化输出
//! 2. **类型安全**: 每个错误变体携带相关的错误数据
//! 3. **自动 trait 实现**: 自动实现 `std::error::Error` trait
//!
//! ## 错误处理流程
//!
//! ```text
//! 配置验证 ──> 发现错误 ──> 返回 InvalidMicroVMConfigError
//!              │
//!              ├─> 根路径不存在 ──> RootPathDoesNotExist
//!              ├─> 内存为零 ──> MemoryIsZero
//!              ├─> 可执行文件不存在 ──> ExecutablePathDoesNotExist
//!              ├─> 命令行包含无效字符 ──> InvalidCommandLineString
//!              └─> 访客路径冲突 ──> ConflictingGuestPaths
//! ```

use std::path::PathBuf;
use thiserror::Error;
use typed_path::Utf8UnixPathBuf;

//--------------------------------------------------------------------------------------------------
// InvalidMicroVMConfigError - MicroVM 配置错误枚举
//--------------------------------------------------------------------------------------------------

/// MicroVM 配置无效时的详细错误
///
/// 此枚举提供了更具体的 MicroVM 配置错误原因，
/// 用于诊断和修复配置问题。
///
/// ## 错误变体
///
/// | 变体 | 触发条件 | 错误消息 |
/// |------|----------|----------|
/// | `RootPathDoesNotExist` | 根路径在主机上不存在 | "根路径 {path} 不存在" |
/// | `MemoryIsZero` | 分配的内存为 0 MiB | "指定的内存为零" |
/// | `ExecutablePathDoesNotExist` | 可执行文件在根文件系统中不存在 | "可执行文件路径 {path} 不存在" |
/// | `InvalidCommandLineString` | 命令行包含非 ASCII 可打印字符 | "命令行字符串 '{cmd}' 包含无效字符" |
/// | `ConflictingGuestPaths` | 两个挂载点路径重叠 | "访客路径 {a} 和 {b} 冲突" |
///
/// ## thiserror 宏
///
/// `#[derive(Error)]` 自动生成：
/// - `Display` trait 实现（使用 `#[error(...)]` 格式）
/// - `std::error::Error` trait 实现
/// - `Send + Sync` trait 实现（用于跨线程错误处理）
#[derive(Debug, Error)]
pub enum InvalidMicroVMConfigError {
    /// MicroVm 的根路径不存在
    ///
    /// 创建 VM 时指定的根文件系统路径在主机上不存在。
    /// 此错误通常在配置验证阶段被检测到。
    ///
    /// ## 可能原因
    ///
    /// - 路径拼写错误
    /// - 路径是相对路径但当前工作目录不正确
    /// - 根文件系统镜像尚未创建或提取
    #[error("根路径 {0} 不存在")]
    RootPathDoesNotExist(PathBuf),

    /// 指定的内存为零
    ///
    /// VM 必须分配至少 1 MiB 的内存才能运行。
    /// 零内存配置在物理上是不可能的。
    ///
    /// ## 默认值
    ///
    /// 如果未指定内存大小，通常使用默认值（如 256 MiB）。
    #[error("指定的内存为零")]
    MemoryIsZero,

    /// 指定的可执行文件路径不存在
    ///
    /// VM 内要运行的可执行文件路径在根文件系统中不存在。
    /// 注意：这里检查的是访客文件系统（容器内）的路径。
    ///
    /// ## 可能原因
    ///
    /// - 可执行文件路径拼写错误
    /// - 使用了绝对路径但相对于错误的根目录
    /// - 可执行文件尚未复制到根文件系统中
    #[error("可执行文件路径 {0} 不存在")]
    ExecutablePathDoesNotExist(Utf8UnixPathBuf),

    /// 命令行字符串包含无效字符
    ///
    /// 命令行（可执行文件路径和参数）只能包含可打印 ASCII 字符
    ///（空格 0x20 到波浪线 0x7E 之间）。
    ///
    /// ## 为什么限制字符范围？
    ///
    /// - **安全性**: 防止注入控制字符或特殊序列
    /// - **兼容性**: 确保在所有终端和日志系统中正确显示
    /// - **简化解析**: 避免处理 Unicode 和转义序列的复杂性
    ///
    /// ## 无效字符示例
    ///
    /// - 控制字符（0x00-0x1F）：换行、制表符等
    /// - DEL 字符（0x7F）
    /// - 非 ASCII 字符（0x80 及以上）
    #[error("命令行字符串 '{0}' 包含无效字符")]
    InvalidCommandLineString(String),

    /// 访客路径冲突
    ///
    /// 当两个挂载点的路径重叠时发生冲突。
    /// 这会导致文件系统挂载顺序和可见性的问题。
    ///
    /// ## 冲突场景
    ///
    /// ### 父子关系
    ///
    /// ```text
    /// 挂载 1: /var
    /// 挂载 2: /var/log
    /// 结果：/var/log 会被挂载两次，后者覆盖前者
    /// ```
    ///
    /// ### 子集关系
    ///
    /// ```text
    /// 挂载 1: /app/data
    /// 挂载 2: /app/data/logs
    /// 结果：/app/data/logs 被挂载两次，产生冲突
    /// ```
    ///
    /// ## 解决方案
    ///
    /// - 移除冗余的挂载点
    /// - 使用单个挂载点覆盖整个目录树
    /// - 选择不重叠的路径
    #[error("访客路径 {0} 和 {1} 冲突")]
    ConflictingGuestPaths(String, String),
}
