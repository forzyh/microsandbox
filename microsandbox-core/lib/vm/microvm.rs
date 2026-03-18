//! 轻量级 Linux 虚拟机实现
//!
//! 本模块提供了 MicroVm（微虚拟机）的实现，用于创建和管理安全隔离的
//! Linux 虚拟机环境。MicroVm 基于 libkrun 库，可以提供 VM 级别的隔离，
//! 同时保持快速的启动速度。
//!
//! ## 架构概述
//!
//! MicroVm 模块是整个微沙箱系统的核心组件之一，它提供了一个轻量级的
//! 虚拟机实现，用于在隔离的环境中安全地运行用户代码。
//!
//! ### libkrun 集成
//!
//! 本模块通过 FFI（外部函数接口）与 libkrun 库进行交互。libkrun 是一个
//! 轻量级的虚拟机管理库，支持快速启动和 VM 级别的隔离。所有与 libkrun
//! 的交互都通过 `ffi` 模块中的函数进行。
//!
//! ### VM 生命周期
//!
//! 1. **创建上下文** - 调用 `krun_create_ctx()` 创建 VM 上下文，获取上下文 ID
//! 2. **应用配置** - 通过 `krun_*` 系列函数配置 VM 的各项参数
//! 3. **启动执行** - 调用 `krun_start_enter()` 启动 VM 并阻塞直到执行完成
//! 4. **清理资源** - 通过 Drop trait 自动调用 `krun_free_ctx()` 释放资源
//!
//! ### 隔离机制
//!
//! MicroVm 提供多层隔离：
//! - **文件系统隔离** - 通过独立的根文件系统和 virtio-fs 挂载实现
//! - **网络隔离** - 通过 TSI（TCP 堆栈隔离）和网络范围限制实现
//! - **资源限制** - 通过 rlimits 限制 CPU、内存等资源使用
//! - **进程隔离** - VM 内的进程与主机完全隔离
//!
//! ## 核心组件
//!
//! - **MicroVm** - 虚拟机实例，负责 VM 的生命周期管理
//! - **MicroVmConfig** - VM 配置结构，包含所有 VM 设置
//! - **Rootfs** - 根文件系统类型，支持 native 和 overlayfs
//! - **LogLevel** - 日志级别控制
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_core::vm::{MicroVm, Rootfs};
//! use tempfile::TempDir;
//!
//! # fn main() -> anyhow::Result<()> {
//! let temp_dir = TempDir::new()?;
//! let vm = MicroVm::builder()
//!     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
//!     .memory_mib(1024)
//!     .exec_path("/bin/echo")
//!     .args(["Hello, World!"])
//!     .build()?;
//!
//! // 启动 VM（会阻塞直到 VM 退出）
//! let status = vm.start()?;
//! # Ok(())
//! # }
//! ```

use std::{ffi::CString, net::Ipv4Addr, path::PathBuf, ptr};

use getset::Getters;
use ipnetwork::Ipv4Network;
use microsandbox_utils::SupportedPathType;
use typed_path::Utf8UnixPathBuf;

use crate::{
    config::{EnvPair, NetworkScope, PathPair, PortPair},
    utils, InvalidMicroVMConfigError, MicrosandboxError, MicrosandboxResult,
};

use super::{ffi, LinuxRlimit, MicroVmBuilder, MicroVmConfigBuilder};

//--------------------------------------------------------------------------------------------------
// 常量
//--------------------------------------------------------------------------------------------------

/// virtio-fs 挂载标签的前缀
///
/// 当挂载共享目录时，每个目录都会被分配一个标签，格式为 "virtiofs_N"
/// 其中 N 是目录的索引号（从 0 开始）。
///
/// ## 用途说明
///
/// virtio-fs 是一种虚拟化的文件系统协议，用于在虚拟机和主机之间
/// 共享目录。libkrun 要求每个 virtio-fs 挂载点都有一个唯一的标签，
/// 此标签在 VM 内部用于识别和挂载对应的共享目录。
///
/// ## 格式示例
/// - `virtiofs_0` - 第一个挂载的共享目录
/// - `virtiofs_1` - 第二个挂载的共享目录
/// - `virtiofs_2` - 第三个挂载的共享目录
///
/// 这个前缀常量用于生成这些标签。
pub const VIRTIOFS_TAG_PREFIX: &str = "virtiofs";

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// 轻量级 Linux 虚拟机
///
/// MicroVm 提供了一个安全、隔离的环境来运行应用程序，具有独立的
/// 文件系统、网络和资源限制。
///
/// ## 核心特性
/// * 真正的 VM 级别隔离，比容器更安全
/// * 支持自定义 vCPU 和内存配置
/// * 支持通过 virtio-fs 共享主机目录
/// * 支持端口转发
/// * 支持资源限制（rlimits）
/// * 基于 libkrun 实现
///
/// ## 内部结构
///
/// `MicroVm` 结构体内部维护两个核心字段：
/// - `ctx_id`: libkrun 库分配的上下文标识符，用于引用和管理 VM 实例
/// - `config`: VM 的完整配置，包含所有运行时参数
///
/// ## ctx_id 的作用
///
/// `ctx_id` 是一个 `u32` 类型的整数，由 libkrun 库在创建上下文时分配。
/// 它是 VM 实例在 libkrun 内部的唯一标识符，所有后续的 libkrun API 调用
/// （如配置 VM、启动 VM、释放资源等）都需要传入这个 ID。
///
/// ## 使用示例
///
/// ```no_run
/// use microsandbox_core::vm::{MicroVm, Rootfs};
/// use tempfile::TempDir;
///
/// # fn main() -> anyhow::Result<()> {
/// let temp_dir = TempDir::new()?;
/// let vm = MicroVm::builder()
///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
///     .memory_mib(1024)
///     .exec_path("/bin/echo")
///     .args(["Hello, World!"])
///     .build()?;
///
/// // 启动 MicroVm
/// vm.start()?;  // 这会实际运行 VM
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Getters)]
pub struct MicroVm {
    /// MicroVm 配置的上下文 ID
    ///
    /// 这是 libkrun 库内部使用的标识符，用于引用和管理 VM 实例。
    /// 所有对 libkrun API 的调用（如 `krun_set_vm_config`、`krun_start_enter`
    /// 等）都需要传入这个 ID 来指定目标 VM。
    ///
    /// 此字段在 VM 创建时通过 `krun_create_ctx()` 分配，在 VM 销毁时
    /// 通过 `krun_free_ctx()` 释放。
    ctx_id: u32,

    /// MicroVm 的配置
    ///
    /// 包含 VM 的所有配置参数，如 CPU、内存、挂载点、网络设置等。
    /// 此配置在 VM 创建时应用，之后不可修改。
    #[get = "pub with_prefix"]
    config: MicroVmConfig,
}

/// MicroVm 使用的根文件系统类型
///
/// 此枚举定义了可用于 MicroVm 的不同类型的根文件系统。
///
/// ## 变体说明
///
/// ### Native(PathBuf)
/// 使用单个路径的原生根文件系统。直接使用指定目录作为 VM 的根文件系统（/）。
/// 适用于简单的场景，如直接使用一个完整的 rootfs 目录。
///
/// ### Overlayfs(Vec<PathBuf>)
/// 使用多个路径的 overlayfs 根文件系统。将多个目录（层）合并成一个
/// 统一的文件系统视图。第一个路径是最底层，后续路径逐层叠加。
///
/// ## OverlayFS 说明
///
/// OverlayFS 是一种联合文件系统，可以将多个目录合并成一个
/// 统一的文件系统视图。在 OCI 镜像中，每个层都是一个目录，
/// 通过 OverlayFS 合并成最终的根文件系统。
///
/// ### OverlayFS 工作原理
///
/// OverlayFS 将多个目录（称为"层"）堆叠在一起：
/// - **最底层**：通常是基础镜像的只读层
/// - **中间层**：额外的只读层，如应用层、配置层
/// - **最顶层**：可写的顶层，所有修改都写到这里
///
/// 在 VM 内部，用户看到的是一个统一的文件系统视图，无法感知
/// 底层的分层结构。
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_core::vm::Rootfs;
/// use std::path::PathBuf;
///
/// // 原生根文件系统
/// let native_root = Rootfs::Native(PathBuf::from("/path/to/root"));
///
/// // OverlayFS 根文件系统（多个层）
/// // 第一个路径是最底层，后续路径逐层叠加
/// let overlayfs_root = Rootfs::Overlayfs(vec![
///     PathBuf::from("/path/to/base_layer"),      // 最底层（只读）
///     PathBuf::from("/path/to/app_layer"),       // 应用层（只读）
///     PathBuf::from("/path/to/writable_layer"),  // 可写层（顶层）
/// ]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rootfs {
    /// 使用底层原生文件系统的根文件系统
    ///
    /// 直接使用单个目录作为 VM 的根文件系统。
    /// 该目录应该包含完整的 Linux 根文件系统结构，
    /// 如 /bin, /etc, /lib, /usr 等目录。
    Native(PathBuf),

    /// 使用 overlayfs 的根文件系统
    ///
    /// 将多个目录（层）合并成一个统一的文件系统视图。
    /// 第一个路径是最底层，后续路径逐层叠加。
    ///
    /// ## 典型用途
    /// - OCI 容器镜像的解压层
    /// - 多层文件系统组合
    /// - 需要可写层的场景
    Overlayfs(Vec<PathBuf>),
}

/// MicroVm 实例的配置结构
///
/// 此结构包含创建和运行 MicroVm 所需的所有设置，
/// 包括系统资源、文件系统配置、网络设置和进程执行详情。
///
/// ## 构建方式
///
/// 建议使用 `MicroVmConfigBuilder` 或 `MicroVmBuilder` 来
/// 构建配置，而不是直接创建此结构。Builder 模式可以确保
/// 所有必需字段都被正确设置，并提供类型安全的配置接口。
///
/// ## 字段说明
///
/// ### 基本配置
/// * `log_level` - 日志级别，控制 libkrun 库的日志输出详细程度
/// * `rootfs` - 根文件系统类型，决定 VM 的文件系统结构
/// * `num_vcpus` - vCPU 数量，虚拟机使用的虚拟 CPU 核心数
/// * `memory_mib` - 内存大小（MiB），虚拟机分配的内存容量
///
/// ### 文件系统配置
/// * `mapped_dirs` - 通过 virtio-fs 挂载的目录列表，用于主机与 VM 之间的文件共享
///
/// ### 网络配置
/// * `port_map` - 端口转发映射，定义 VM 端口与主机端口的映射关系
/// * `scope` - 网络范围，定义 VM 可以访问的网络地址范围
/// * `ip` - IP 地址（可选），如果不指定则自动分配
/// * `subnet` - 子网（可选），如果不指定则使用默认子网
///
/// ### 资源限制
/// * `rlimits` - 资源限制列表，定义 VM 可以使用的各种资源的上限
///
/// ### 进程执行配置
/// * `workdir_path` - 工作目录，VM 中进程启动时的工作目录
/// * `exec_path` - 可执行文件路径，VM 中要运行的程序路径
/// * `args` - 命令行参数，传递给可执行文件的参数列表
/// * `env` - 环境变量，为可执行文件设置的环境变量列表
///
/// ### 其他配置
/// * `console_output` - 控制台输出路径，VM 控制台输出的目标文件路径
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_core::vm::{MicroVm, MicroVmConfig, Rootfs};
/// use tempfile::TempDir;
///
/// # fn main() -> anyhow::Result<()> {
/// let temp_dir = TempDir::new()?;
/// let config = MicroVmConfig::builder()
///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
///     .memory_mib(1024)
///     .exec_path("/bin/echo")
///     .build();
///
/// let vm = MicroVm::from_config(config)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct MicroVmConfig {
    /// MicroVm 使用的日志级别
    ///
    /// 控制 libkrun 库的日志输出级别。日志级别从 0（关闭）到 5（追踪），
    /// 数值越高输出的日志越详细。
    ///
    /// ## 日志级别说明
    /// - `Off (0)` - 关闭所有日志输出
    /// - `Error (1)` - 只输出错误级别的日志
    /// - `Warn (2)` - 输出错误和警告级别的日志
    /// - `Info (3)` - 输出错误、警告和信息级别的日志
    /// - `Debug (4)` - 输出所有级别的日志，包括调试信息
    /// - `Trace (5)` - 最详细的日志级别，包括追踪信息
    pub log_level: LogLevel,

    /// MicroVm 的根文件系统
    ///
    /// 可以是原生文件系统（`Rootfs::Native`）或 overlayfs（`Rootfs::Overlayfs`）。
    /// 根文件系统是 VM 启动时的基础文件系统，VM 内的所有文件操作都基于此。
    pub rootfs: Rootfs,

    /// MicroVm 使用的 vCPU 数量
    ///
    /// 虚拟 CPU 核心数，通常为 1-8 之间。
    /// 必须大于 0，否则配置验证会失败。
    pub num_vcpus: u8,

    /// MicroVm 使用的内存大小（MiB）
    ///
    /// 以 MiB（兆字节）为单位的内存量。
    /// 必须大于 0，否则配置验证会失败。
    ///
    /// ## 推荐配置
    /// - 最小：64 MiB（仅适用于极简单的应用）
    /// - 推荐：512-2048 MiB（适用于大多数应用）
    /// - 最大：取决于主机可用内存
    pub memory_mib: u32,

    /// 通过 virtio-fs 挂载的目录列表
    ///
    /// 每个 `PathPair` 代表一个主机到访客的路径映射。
    /// virtio-fs 是一种高性能的虚拟化文件系统协议，
    /// 允许 VM 直接访问主机上的目录。
    ///
    /// ## 格式说明
    /// 每个 `PathPair` 的格式为 `"主机路径：访客路径"`
    /// 例如：`"/host/data:/app/data"` 表示将主机的 `/host/data`
    /// 挂载到 VM 内的 `/app/data`。
    ///
    /// ## 注意事项
    /// - 主机路径必须存在且可访问
    /// - 访客路径不能重叠（如 `/app` 和 `/app/data`）
    /// - 挂载的目录在 VM 内是可读写的
    pub mapped_dirs: Vec<PathPair>,

    /// MicroVm 使用的端口映射
    ///
    /// 每个 `PortPair` 定义了一个端口转发规则，将 VM 内的端口
    /// 映射到主机的端口，使外部可以访问 VM 内的服务。
    ///
    /// ## 格式说明
    /// 每个 `PortPair` 的格式为 `"主机端口：访客端口"`
    /// 例如：`"8080:80"` 表示将主机的 8080 端口转发到 VM 的 80 端口。
    pub port_map: Vec<PortPair>,

    /// MicroVm 使用的网络范围
    ///
    /// 定义 VM 可以访问的网络地址范围。
    ///
    /// ## 可用范围
    /// - `HostOnly` - 只能访问主机网络
    /// - `Private` - 私有网络，只能与同组 VM 通信
    /// - `Public` - 可以访问公共网络
    pub scope: NetworkScope,

    /// MicroVm 使用的 IP 地址
    ///
    /// 如果为 `None`，则自动分配一个可用的 IP 地址。
    /// 如果指定，则使用该 IP 地址作为 VM 的内部 IP。
    pub ip: Option<Ipv4Addr>,

    /// MicroVm 使用的子网
    ///
    /// 如果为 `None`，则使用默认子网。
    /// 可以指定自定义子网以控制 VM 的网络拓扑。
    pub subnet: Option<Ipv4Network>,

    /// MicroVm 使用的资源限制列表
    ///
    /// 每个 `LinuxRlimit` 定义了一种资源的软/硬限制。
    /// 可以限制的资源包括：
    /// - CPU 时间
    /// - 内存使用
    /// - 打开文件数
    /// - 进程数
    /// - 等等
    ///
    /// ## 软限制 vs 硬限制
    /// - **软限制**：当前生效的限制，可以被进程提高到不超过硬限制
    /// - **硬限制**：软限制的上限，只有特权进程才能提高
    pub rlimits: Vec<LinuxRlimit>,

    /// MicroVm 使用的工作目录路径
    ///
    /// VM 中进程启动时的工作目录。
    /// 如果为 `None`，则使用根目录 `/` 作为工作目录。
    pub workdir_path: Option<Utf8UnixPathBuf>,

    /// MicroVm 使用的可执行文件路径
    ///
    /// VM 中要运行的程序路径。
    /// 必须是 VM 根文件系统内存在的路径。
    pub exec_path: Utf8UnixPathBuf,

    /// 传递给可执行文件的参数列表
    ///
    /// 命令行参数，`args[0]` 通常是程序名。
    /// 参数必须只包含可打印 ASCII 字符（空格到波浪线）。
    pub args: Vec<String>,

    /// 为可执行文件设置的环境变量列表
    ///
    /// 每个 `EnvPair` 格式为 `"NAME=value"`。
    /// 环境变量必须只包含可打印 ASCII 字符。
    pub env: Vec<EnvPair>,

    /// MicroVm 使用的控制台输出路径
    ///
    /// VM 控制台输出的目标文件路径。
    /// 如果为 `None`，则控制台输出到标准输出/错误。
    pub console_output: Option<Utf8UnixPathBuf>,
}

/// MicroVm 使用的日志级别
///
/// 定义 libkrun 库的日志详细程度。日志级别从 0（关闭）到 5（追踪），
/// 数值越高输出的日志越详细。
///
/// ## 日志级别说明
///
/// | 级别 | 值 | 说明 |
/// |------|-----|------|
/// | Off | 0 | 关闭所有日志输出 |
/// | Error | 1 | 只输出错误级别的日志 |
/// | Warn | 2 | 输出错误和警告级别的日志 |
/// | Info | 3 | 输出错误、警告和信息级别的日志 |
/// | Debug | 4 | 输出所有级别的日志，包括调试信息 |
/// | Trace | 5 | 最详细的日志级别，包括追踪信息 |
///
/// ## 使用建议
///
/// - **生产环境**：使用 `Off` 或 `Error`，减少日志输出
/// - **开发调试**：使用 `Debug` 或 `Trace`，获取详细信息
/// - **默认设置**：使用 `Off`（通过 `#[default]` 标记）
///
/// ## 示例
///
/// ```rust
/// use microsandbox_core::vm::LogLevel;
///
/// // 使用枚举值
/// let level = LogLevel::Debug;
///
/// // 从 u8 转换
/// let level = LogLevel::try_from(3).unwrap(); // Info
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum LogLevel {
    /// 关闭日志
    ///
    /// 不输出任何日志消息。
    /// 适用于生产环境，减少日志开销。
    #[default]
    Off = 0,

    /// 错误消息
    ///
    /// 只输出错误级别的日志。
    /// 适用于只需要了解错误的生产环境。
    Error = 1,

    /// 警告消息
    ///
    /// 输出错误和警告级别的日志。
    /// 适用于需要监控潜在问题的环境。
    Warn = 2,

    /// 信息消息
    ///
    /// 输出错误、警告和信息级别的日志。
    /// 适用于需要了解系统运行状态的环境。
    Info = 3,

    /// 调试消息
    ///
    /// 输出所有级别的日志，包括调试信息。
    /// 适用于开发和调试环境。
    Debug = 4,

    /// 追踪消息
    ///
    /// 最详细的日志级别，包括追踪信息。
    /// 适用于深度调试和故障排查。
    Trace = 5,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl MicroVm {
    /// 从给定配置创建新的 MicroVm 实例
    ///
    /// 这是一个底层构造函数 - 建议使用 `MicroVm::builder()`
    /// 来获得更友好的接口。
    ///
    /// ## 参数
    /// * `config` - MicroVm 配置，包含 VM 的所有设置
    ///
    /// ## 返回值
    /// 成功时返回 `Ok(MicroVm)` 实例，失败时返回 `Err(MicrosandboxError)` 错误。
    ///
    /// ## 错误情况
    /// * `InvalidMicroVMConfig` - 配置无效（如路径不存在、vCPU 为 0 等）
    /// * 其他错误 - 系统缺少必需的功能或无法分配所需资源
    ///
    /// ## 内部流程
    /// 1. 调用 `create_ctx()` 创建 VM 上下文，获取上下文 ID
    /// 2. 调用 `config.validate()` 验证配置的有效性
    /// 3. 调用 `apply_ctx()` 将配置应用到 VM 上下文
    /// 4. 返回构造完成的 `MicroVm` 实例
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVm, MicroVmConfig, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let config = MicroVmConfig::builder()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/bin/echo")
    ///     .build();
    ///
    /// let vm = MicroVm::from_config(config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_config(config: MicroVmConfig) -> MicrosandboxResult<Self> {
        // 创建 VM 上下文
        let ctx_id = Self::create_ctx();

        // 验证配置
        config.validate()?;

        // 应用配置到 VM 上下文
        Self::apply_config(ctx_id, &config);

        Ok(Self { ctx_id, config })
    }

    /// 创建用于配置新 MicroVm 实例的构建器
    ///
    /// 这是创建新 MicroVm 的推荐方式。
    /// 构建器模式提供了更友好的接口，确保所有必需字段都被设置。
    ///
    /// ## 返回值
    /// 返回 `MicroVmBuilder<(), ()>` 实例，用于链式配置 VM。
    ///
    /// ## 类型状态参数
    /// 构建器使用类型状态模式确保配置的正确性：
    /// - 第一个参数表示 Rootfs 状态（`()` 表示未设置，`Rootfs` 表示已设置）
    /// - 第二个参数表示 ExecPath 状态（`()` 表示未设置，`Utf8UnixPathBuf` 表示已设置）
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVm, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let vm = MicroVm::builder()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/bin/echo")
    ///     .args(["Hello, World!"])
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> MicroVmBuilder<(), ()> {
        MicroVmBuilder::default()
    }

    /// 启动 MicroVm 并等待其完成
    ///
    /// 此函数会阻塞直到 MicroVm 退出。返回访客进程的退出状态。
    ///
    /// ## 返回值
    /// * `Ok(i32)` - 进程的退出状态码（0 表示成功，非 0 表示失败）
    /// * `Err(MicrosandboxError::StartVmFailed)` - 启动失败时的错误，包含 libkrun 返回的状态码
    ///
    /// ## 内部实现
    /// 调用 libkrun 的 `krun_start_enter()` 函数启动 VM。
    /// 此函数会：
    /// 1. 启动虚拟机
    /// 2. 在 VM 内执行配置的可执行文件
    /// 3. 阻塞直到 VM 内进程退出
    /// 4. 返回进程的退出状态
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// use microsandbox_core::vm::{MicroVm, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let vm = MicroVm::builder()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/usr/bin/python3")
    ///     .args(["-c", "print('Hello from MicroVm!')"])
    ///     .build()?;
    ///
    /// let status = vm.start()?;
    /// assert_eq!(status, 0);  // 进程成功退出
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    /// * 此函数会控制 stdin/stdout
    /// * MicroVm 会在返回后自动清理（通过 Drop trait）
    /// * 非零状态表示访客进程失败
    /// * 此函数是阻塞的，如果需要异步执行，请使用 tokio::task::spawn_blocking
    pub fn start(&self) -> MicrosandboxResult<i32> {
        let ctx_id = self.ctx_id;
        // 调用 libkrun 的 krun_start_enter 函数启动 VM
        let status = unsafe { ffi::krun_start_enter(ctx_id) };
        if status < 0 {
            tracing::error!("failed to start microvm: {}", status);
            return Err(MicrosandboxError::StartVmFailed(status));
        }
        tracing::info!("microvm exited with status: {}", status);
        Ok(status)
    }

    /// 创建新的 MicroVm 上下文
    ///
    /// 调用 libkrun 的 `krun_create_ctx()` 函数创建 VM 上下文。
    /// 这是 VM 生命周期的第一步，返回的上下文 ID 用于后续的所有 libkrun API 调用。
    ///
    /// ## 返回值
    /// 上下文 ID（非负整数），类型为 `u32`。
    ///
    /// ## Panics
    /// 如果创建上下文失败会 panic。
    /// libkrun 返回负值表示错误，此时期望触发 panic 因为创建上下文不应该失败。
    ///
    /// ## 内部实现
    /// 此函数是一个私有辅助函数，仅在 `from_config()` 中被调用。
    /// 它封装了 libkrun 的 FFI 调用，并提供断言确保返回值有效。
    fn create_ctx() -> u32 {
        let ctx_id = unsafe { ffi::krun_create_ctx() };
        assert!(ctx_id >= 0, "failed to create microvm context: {}", ctx_id);
        ctx_id as u32
    }

    /// 将配置应用到 MicroVm 上下文
    ///
    /// 此方法配置 MicroVm 的所有方面，是 VM 初始化的核心逻辑。
    /// 它将 `MicroVmConfig` 中的所有配置项逐一应用到 libkrun 上下文中。
    ///
    /// ## 配置流程
    ///
    /// 此方法按照以下顺序应用配置：
    ///
    /// 1. **日志级别设置** - 调用 `krun_set_log_level()` 设置 libkrun 日志级别
    /// 2. **基本 VM 配置** - 调用 `krun_set_vm_config()` 设置 vCPU 数量和内存大小
    /// 3. **根文件系统设置** - 根据 `Rootfs` 类型调用不同的函数：
    ///    - `Rootfs::Native` - 调用 `krun_set_root()` 设置单个根路径
    ///    - `Rootfs::Overlayfs` - 调用 `krun_set_overlayfs_root()` 设置多层根路径
    /// 4. **virtio-fs 目录映射** - 对每个 `mapped_dirs` 条目调用 `krun_add_virtiofs()`
    /// 5. **端口映射设置** - 调用 `krun_set_port_map()` 设置端口转发规则
    /// 6. **网络范围设置** - 调用 `krun_set_tsi_scope()` 设置网络访问范围
    /// 7. **资源限制设置** - 如果有 rlimits，调用 `krun_set_rlimits()` 设置资源限制
    /// 8. **工作目录设置** - 如果指定了工作目录，调用 `krun_set_workdir()` 设置
    /// 9. **执行配置设置** - 调用 `krun_set_exec()` 设置可执行文件、参数和环境变量
    /// 10. **控制台输出设置** - 如果指定了控制台输出，调用 `krun_set_console_output()` 设置
    ///
    /// ## 参数
    /// * `ctx_id` - 要配置的 MicroVm 上下文 ID，由 `create_ctx()` 返回
    /// * `config` - 要应用的配置，包含所有 VM 设置
    ///
    /// ## Panics
    /// * 任何 libkrun API 调用失败时会 panic（返回负值）
    /// * 无法规范化主机路径时会 panic
    /// * 无法创建 CString 时会 panic（如路径包含空字节）
    ///
    /// ## 注意事项
    /// - 此方法必须在 `start()` 之前调用
    /// - 配置一旦应用，不能在 VM 运行期间修改
    /// - virtio-fs 挂载的主机路径会被规范化（canonicalize）
    fn apply_config(ctx_id: u32, config: &MicroVmConfig) {
        // 设置日志级别
        // 调用 libkrun 的 krun_set_log_level 函数设置日志级别
        // 日志级别影响 libkrun 内部日志输出的详细程度
        unsafe {
            let status = ffi::krun_set_log_level(config.log_level as u32);
            assert!(status >= 0, "failed to set log level: {}", status);
        }

        // 设置基本 VM 配置（vCPU 数量和内存大小）
        // 调用 libkrun 的 krun_set_vm_config 函数设置虚拟机的基本资源
        unsafe {
            let status = ffi::krun_set_vm_config(ctx_id, config.num_vcpus, config.memory_mib);
            assert!(status >= 0, "failed to set VM config: {}", status);
        }

        // 设置根文件系统
        // 根据 Rootfs 类型选择不同的设置方式
        match &config.rootfs {
            Rootfs::Native(path) => {
                // 对于原生根文件系统，直接使用单个路径
                // 调用 krun_set_root 设置 VM 的根文件系统
                let c_path = CString::new(path.to_str().unwrap().as_bytes()).unwrap();
                unsafe {
                    let status = ffi::krun_set_root(ctx_id, c_path.as_ptr());
                    assert!(status >= 0, "failed to set rootfs: {}", status);
                }
            }
            Rootfs::Overlayfs(paths) => {
                // 对于 overlayfs，使用多个路径（层）
                // 调用 krun_set_overlayfs_root 设置多层根文件系统
                // 第一个路径是最底层，后续路径逐层叠加
                tracing::debug!("setting overlayfs rootfs: {:?}", paths);
                let c_paths: Vec<_> = paths
                    .iter()
                    .map(|p| CString::new(p.to_str().unwrap().as_bytes()).unwrap())
                    .collect();
                let c_paths_ptrs = utils::to_null_terminated_c_array(&c_paths);
                unsafe {
                    let status = ffi::krun_set_overlayfs_root(ctx_id, c_paths_ptrs.as_ptr());
                    assert!(status >= 0, "failed to set rootfs: {}", status);
                }
            }
        }

        tracing::debug!("applying config: {:#?}", config);

        // 使用 virtio-fs 添加映射的目录
        // virtio-fs 是一种高性能的虚拟化文件系统协议，用于在 VM 和主机之间共享目录
        let mapped_dirs = &config.mapped_dirs;
        for (idx, dir) in mapped_dirs.iter().enumerate() {
            // 为每个目录生成唯一的 virtio-fs 标签
            // 标签格式为 "virtiofs_N"，其中 N 是目录索引号
            let tag = CString::new(format!("{}_{}", VIRTIOFS_TAG_PREFIX, idx)).unwrap();
            tracing::debug!("adding virtiofs mount for {}", tag.to_string_lossy());

            // 规范化主机路径
            // 使用 canonicalize 确保路径是绝对的且存在
            let host_path_buf = PathBuf::from(dir.get_host().as_str());
            let canonical_host_path = match host_path_buf.canonicalize() {
                Ok(path) => path,
                Err(e) => {
                    tracing::error!("failed to canonicalize host path: {}", e);
                    panic!("failed to canonicalize host path: {}", e);
                }
            };

            let host_path = CString::new(canonical_host_path.to_string_lossy().as_bytes()).unwrap();
            tracing::debug!("canonical host path: {}", host_path.to_string_lossy());

            unsafe {
                // 添加 virtio-fs 挂载点
                // krun_add_virtiofs 将主机目录挂载到 VM 内
                let status = ffi::krun_add_virtiofs(ctx_id, tag.as_ptr(), host_path.as_ptr());
                assert!(status >= 0, "failed to add mapped directory: {}", status);
            }
        }

        // 设置端口映射
        // 端口映射允许外部访问 VM 内的服务
        // 格式为 "主机端口：访客端口"
        let c_port_map: Vec<_> = config
            .port_map
            .iter()
            .map(|p| CString::new(p.to_string()).unwrap())
            .collect();
        let c_port_map_ptrs = utils::to_null_terminated_c_array(&c_port_map);

        unsafe {
            let status = ffi::krun_set_port_map(ctx_id, c_port_map_ptrs.as_ptr());
            assert!(status >= 0, "failed to set port map: {}", status);
        }

        // 设置网络范围
        // 网络范围定义 VM 可以访问的网络地址范围
        // 使用 krun_set_tsi_scope 设置 TCP 堆栈隔离范围
        unsafe {
            let status =
                ffi::krun_set_tsi_scope(ctx_id, ptr::null(), ptr::null(), config.scope as u8);
            assert!(status >= 0, "failed to set network scope: {}", status);
        }

        // 设置资源限制
        // 资源限制控制 VM 可以使用的各种资源的上限
        // 如 CPU 时间、内存、打开文件数等
        if !config.rlimits.is_empty() {
            let c_rlimits: Vec<_> = config
                .rlimits
                .iter()
                .map(|s| CString::new(s.to_string()).unwrap())
                .collect();
            let c_rlimits_ptrs = utils::to_null_terminated_c_array(&c_rlimits);
            unsafe {
                let status = ffi::krun_set_rlimits(ctx_id, c_rlimits_ptrs.as_ptr());
                assert!(status >= 0, "failed to set resource limits: {}", status);
            }
        }

        // 设置工作目录
        // 工作目录是 VM 内进程启动时的当前目录
        if let Some(workdir) = &config.workdir_path {
            let c_workdir = CString::new(workdir.to_string().as_bytes()).unwrap();
            unsafe {
                let status = ffi::krun_set_workdir(ctx_id, c_workdir.as_ptr());
                assert!(status >= 0, "Failed to set working directory: {}", status);
            }
        }

        // 设置可执行文件路径、参数和环境变量
        // 这是 VM 执行的核心配置，决定 VM 启动后运行什么程序
        let c_exec_path = CString::new(config.exec_path.to_string().as_bytes()).unwrap();

        // 准备命令行参数数组（argv）
        // 参数数组必须以 null 结尾，符合 C 语言惯例
        let c_argv: Vec<_> = config
            .args
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap())
            .collect();
        let c_argv_ptrs = utils::to_null_terminated_c_array(&c_argv);

        // 准备环境变量数组（envp）
        // 环境变量格式为 "NAME=value"，也必须以 null 结尾
        let c_env: Vec<_> = config
            .env
            .iter()
            .map(|s| CString::new(s.to_string()).unwrap())
            .collect();
        let c_env_ptrs = utils::to_null_terminated_c_array(&c_env);

        unsafe {
            // 调用 krun_set_exec 设置执行配置
            // 此函数设置 VM 启动后要执行的程序及其参数和环境
            let status = ffi::krun_set_exec(
                ctx_id,
                c_exec_path.as_ptr(),
                c_argv_ptrs.as_ptr(),
                c_env_ptrs.as_ptr(),
            );
            assert!(
                status >= 0,
                "Failed to set executable configuration: {}",
                status
            );
        }

        // 设置控制台输出
        // 如果指定了控制台输出路径，VM 的控制台输出会重定向到该文件
        if let Some(console_output) = &config.console_output {
            let c_console_output = CString::new(console_output.to_string().as_bytes()).unwrap();
            unsafe {
                let status = ffi::krun_set_console_output(ctx_id, c_console_output.as_ptr());
                assert!(status >= 0, "Failed to set console output: {}", status);
            }
        }
    }
}

impl MicroVmConfig {
    /// 创建用于配置新 MicroVm 配置的构建器
    ///
    /// 这是创建 `MicroVmConfig` 实例的推荐方式。构建器模式
    /// 提供了更友好的接口，确保所有必需字段都被设置。
    ///
    /// ## 返回值
    /// 返回 `MicroVmConfigBuilder<(), ()>` 实例，用于链式配置 VM。
    ///
    /// ## 类型状态参数
    /// 构建器使用类型状态模式确保配置的正确性：
    /// - 第一个参数表示 Rootfs 状态（`()` 表示未设置，`Rootfs` 表示已设置）
    /// - 第二个参数表示 ExecPath 状态（`()` 表示未设置，`Utf8UnixPathBuf` 表示已设置）
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVm, MicroVmConfig, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let config = MicroVmConfig::builder()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/bin/echo")
    ///     .build();
    ///
    /// let vm = MicroVm::from_config(config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> MicroVmConfigBuilder<(), ()> {
        MicroVmConfigBuilder::default()
    }

    /// 验证访客路径不是彼此的子集
    ///
    /// 此函数检查所有挂载到 VM 的访客路径（guest paths），确保它们之间
    /// 没有重叠或冲突。路径冲突会导致挂载问题和安全风险。
    ///
    /// ## 冲突场景示例
    ///
    /// 以下路径会产生冲突：
    /// - `/app` 和 `/app/data` - 父子关系（子集）
    /// - `/var/log` 和 `/var` - 父子关系（父集）
    /// - `/data` 和 `/data` - 完全重复
    ///
    /// ## 参数
    /// * `mapped_dirs` - 要验证的映射目录列表，每个目录包含主机路径和访客路径
    ///
    /// ## 返回值
    /// * `Ok(())` - 如果没有路径冲突，所有访客路径都是独立的
    /// * `Err(MicrosandboxError::InvalidMicroVMConfig)` - 包含冲突路径的详细信息
    ///
    /// ## 实现说明
    /// 1. 首先规范化所有路径（移除 `.`、`..`、重复的 `/` 等）
    /// 2. 比较每对路径，检查是否存在前缀关系
    /// 3. 使用 `utils::paths_overlap()` 判断路径是否重叠
    ///
    /// ## 性能考虑
    /// - 时间复杂度：O(n²)，其中 n 是路径数量
    /// - 路径预先规范化，避免重复计算
    fn validate_guest_paths(mapped_dirs: &[PathPair]) -> MicrosandboxResult<()> {
        // 如果有 0 或 1 个路径，不可能有冲突
        if mapped_dirs.len() <= 1 {
            return Ok(());
        }

        // 预先规范化所有路径，避免重复规范化
        // 规范化可以处理如 "/app/./data" 和 "/app/data/" 这样的情况
        let normalized_paths: Vec<_> = mapped_dirs
            .iter()
            .map(|dir| {
                microsandbox_utils::normalize_path(
                    dir.get_guest().as_str(),
                    SupportedPathType::Absolute,
                )
                .map_err(Into::into)
            })
            .collect::<MicrosandboxResult<Vec<_>>>()?;

        // 比较每对路径（只比较一次）
        // 不使用 windows 因为会漏掉一些比较
        // 外层循环遍历每个路径
        for i in 0..normalized_paths.len() {
            let path1 = &normalized_paths[i];

            // 只需要检查后面的路径，前面的已经比较过了
            // 这样可以避免重复比较（如 A 与 B 比较后，不需要再 B 与 A 比较）
            for path2 in &normalized_paths[i + 1..] {
                // 检查两个方向的前缀关系
                // paths_overlap 会检查 path1 是否是 path2 的前缀，或反之
                if utils::paths_overlap(path1, path2) {
                    return Err(MicrosandboxError::InvalidMicroVMConfig(
                        InvalidMicroVMConfigError::ConflictingGuestPaths(
                            path1.to_string(),
                            path2.to_string(),
                        ),
                    ));
                }
            }
        }

        Ok(())
    }

    /// 验证 MicroVm 配置
    ///
    /// 执行一系列检查以确保配置有效和安全。
    /// 此函数在 VM 创建时被调用，如果配置无效会阻止 VM 启动。
    ///
    /// ## 验证项目
    ///
    /// 此函数执行以下验证检查：
    ///
    /// 1. **根路径存在性检查**
    ///    - 验证 `rootfs` 中指定的路径是否存在
    ///    - 对于 `Rootfs::Native`，检查单个路径
    ///    - 对于 `Rootfs::Overlayfs`，检查所有层路径
    ///
    /// 2. **主机路径存在性检查**
    ///    - 验证 `mapped_dirs` 中的所有主机路径是否存在
    ///    - 确保可以挂载这些目录到 VM
    ///
    /// 3. **vCPU 数量检查**
    ///    - 确保 `num_vcpus` 非零
    ///    - vCPU 为 0 会导致 VM 无法启动
    ///
    /// 4. **内存大小检查**
    ///    - 确保 `memory_mib` 非零
    ///    - 内存为 0 会导致 VM 无法启动
    ///
    /// 5. **命令字符串有效性检查**
    ///    - 验证 `exec_path` 只包含可打印 ASCII 字符
    ///    - 验证所有 `args` 只包含可打印 ASCII 字符
    ///    - 防止控制字符和非 ASCII Unicode 字符
    ///
    /// 6. **访客路径冲突检查**
    ///    - 验证 `mapped_dirs` 中的访客路径不重叠或冲突
    ///    - 防止挂载点冲突导致的安全问题
    ///
    /// ## 返回值
    /// * `Ok(())` - 如果配置有效，所有检查都通过
    /// * `Err(MicrosandboxError::InvalidMicroVMConfig)` - 包含失败详情
    ///
    /// ## 错误类型
    /// 可能的错误包括：
    /// - `RootPathDoesNotExist` - 根路径不存在
    /// - `HostPathDoesNotExist` - 主机路径不存在
    /// - `NumVCPUsIsZero` - vCPU 数量为 0
    /// - `MemoryIsZero` - 内存大小为 0
    /// - `InvalidCommandLineString` - 命令行字符串包含无效字符
    /// - `ConflictingGuestPaths` - 访客路径冲突
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmConfig, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let config = MicroVmConfig::builder()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/bin/echo")
    ///     .build();
    ///
    /// assert!(config.validate().is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn validate(&self) -> MicrosandboxResult<()> {
        // 检查 rootfs 中指定的路径是否存在
        // 根文件系统是 VM 运行的基础，必须存在且可访问
        match &self.rootfs {
            Rootfs::Native(path) => {
                // 对于原生根文件系统，检查单个路径是否存在
                if !path.exists() {
                    return Err(MicrosandboxError::InvalidMicroVMConfig(
                        InvalidMicroVMConfigError::RootPathDoesNotExist(
                            path.to_str().unwrap().into(),
                        ),
                    ));
                }
            }
            Rootfs::Overlayfs(paths) => {
                // 对于 overlayfs 根文件系统，检查所有层路径是否存在
                // 任何一个层路径不存在都会导致 overlayfs 无法正确挂载
                for path in paths {
                    if !path.exists() {
                        return Err(MicrosandboxError::InvalidMicroVMConfig(
                            InvalidMicroVMConfigError::RootPathDoesNotExist(
                                path.to_str().unwrap().into(),
                            ),
                        ));
                    }
                }
            }
        }

        // 检查 mapped_dirs 中的所有主机路径是否存在
        // 主机路径必须存在才能成功挂载到 VM 内
        for dir in &self.mapped_dirs {
            let host_path = PathBuf::from(dir.get_host().as_str());
            if !host_path.exists() {
                return Err(MicrosandboxError::InvalidMicroVMConfig(
                    InvalidMicroVMConfigError::HostPathDoesNotExist(
                        host_path.to_str().unwrap().into(),
                    ),
                ));
            }
        }

        // 验证 vCPU 数量非零
        // vCPU 为 0 会导致 VM 无法启动，因为没有任何 CPU 可以执行代码
        if self.num_vcpus == 0 {
            return Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::NumVCPUsIsZero,
            ));
        }

        // 验证内存非零
        // 内存为 0 会导致 VM 无法启动，因为没有内存可以加载内核和程序
        if self.memory_mib == 0 {
            return Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::MemoryIsZero,
            ));
        }

        // 验证命令行字符串
        // 检查可执行文件路径是否只包含可打印 ASCII 字符
        Self::validate_command_line(self.exec_path.as_ref())?;

        // 检查所有参数是否只包含可打印 ASCII 字符
        for arg in &self.args {
            Self::validate_command_line(arg)?;
        }

        // 验证访客路径不重叠或冲突
        // 路径冲突可能导致挂载问题和安全风险
        Self::validate_guest_paths(&self.mapped_dirs)?;

        Ok(())
    }

    /// 验证命令行字符串只包含允许的字符
    ///
    /// 命令行字符串（可执行文件路径和参数）必须只包含可打印 ASCII 字符，
    /// 范围从空格 (0x20) 到波浪线 (0x7E)。排除：
    /// - 控制字符（换行、制表符等）
    /// - 非 ASCII Unicode 字符
    /// - 空字节
    ///
    /// ## 参数
    /// * `s` - 要验证的字符串
    ///
    /// ## 返回值
    /// * `Ok(())` - 如果字符串只包含有效字符
    /// * `Err(MicrosandboxError::InvalidMicroVMConfig)` - 如果发现无效字符
    ///
    /// ## 使用示例
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfig;
    ///
    /// // 有效字符串
    /// assert!(MicroVmConfig::validate_command_line("/bin/echo").is_ok());
    /// assert!(MicroVmConfig::validate_command_line("Hello, World!").is_ok());
    ///
    /// // 无效字符串
    /// assert!(MicroVmConfig::validate_command_line("/bin/echo\n").is_err());  // 换行符
    /// assert!(MicroVmConfig::validate_command_line("hello🌎").is_err());      // emoji
    /// ```
    ///
    /// ## 安全考虑
    /// 此验证是为了防止：
    /// 1. 注入攻击 - 通过换行符或其他控制字符注入恶意参数
    /// 2. 路径遍历 - 某些特殊字符可能被用于绕过路径检查
    /// 3. C FFI 安全问题 - 空字节会破坏 C 字符串的完整性
    pub fn validate_command_line(s: &str) -> MicrosandboxResult<()> {
        // 检查字符是否在有效范围内（空格到波浪线）
        // 使用 matches! 宏和范围模式进行高效的字符匹配
        fn valid_char(c: char) -> bool {
            matches!(c, ' '..='~')
        }

        // 检查字符串中的所有字符是否都有效
        if s.chars().all(valid_char) {
            Ok(())
        } else {
            // 找到无效字符，返回详细的错误信息
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::InvalidCommandLineString(s.to_string()),
            ))
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl Drop for MicroVm {
    /// 清理 MicroVm 资源
    ///
    /// 当 `MicroVm` 实例被销毁时（离开作用域或被显式删除），
    /// 此方法会自动调用，释放 VM 上下文占用的资源。
    ///
    /// ## 清理流程
    /// 调用 libkrun 的 `krun_free_ctx()` 函数释放 VM 上下文。
    /// 这是 VM 生命周期的最后一步，确保没有资源泄漏。
    ///
    /// ## 注意事项
    /// - 此方法 automatically 调用，无需手动干预
    /// - 即使 VM 启动失败，也会正确清理资源
    /// - 不应该在此方法中 panic，因为 Drop 在 panic 展开时可能被调用
    fn drop(&mut self) {
        unsafe { ffi::krun_free_ctx(self.ctx_id) };
    }
}

impl TryFrom<u8> for LogLevel {
    type Error = MicrosandboxError;

    /// 将 `u8` 值转换为 `LogLevel` 枚举
    ///
    /// ## 参数
    /// * `value` - 要转换的 u8 值（0-5 之间）
    ///
    /// ## 返回值
    /// * `Ok(LogLevel)` - 如果值在有效范围内（0-5）
    /// * `Err(MicrosandboxError::InvalidLogLevel)` - 如果值超出范围
    ///
    /// ## 映射关系
    /// - 0 → `LogLevel::Off`
    /// - 1 → `LogLevel::Error`
    /// - 2 → `LogLevel::Warn`
    /// - 3 → `LogLevel::Info`
    /// - 4 → `LogLevel::Debug`
    /// - 5 → `LogLevel::Trace`
    fn try_from(value: u8) -> Result<Self, MicrosandboxError> {
        match value {
            0 => Ok(LogLevel::Off),
            1 => Ok(LogLevel::Error),
            2 => Ok(LogLevel::Warn),
            3 => Ok(LogLevel::Info),
            4 => Ok(LogLevel::Debug),
            5 => Ok(LogLevel::Trace),
            _ => Err(MicrosandboxError::InvalidLogLevel(value)),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use microsandbox_utils::DEFAULT_NUM_VCPUS;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// 测试 MicroVmConfig 构建器的基本功能
    ///
    /// 验证构建器能够正确设置各个配置字段，
    /// 包括日志级别、根文件系统、内存大小和 vCPU 数量。
    #[test]
    fn test_microvm_config_builder() {
        let config = MicroVmConfig::builder()
            .log_level(LogLevel::Info)
            .rootfs(Rootfs::Native(PathBuf::from("/tmp")))
            .memory_mib(512)
            .exec_path("/bin/echo")
            .build();

        assert!(config.log_level == LogLevel::Info);
        assert_eq!(config.rootfs, Rootfs::Native(PathBuf::from("/tmp")));
        assert_eq!(config.memory_mib, 512);
        assert_eq!(config.num_vcpus, DEFAULT_NUM_VCPUS);
    }

    /// 测试配置验证成功的情况
    ///
    /// 使用临时目录创建有效配置，验证所有检查都能通过。
    #[test]
    fn test_microvm_config_validation_success() {
        let temp_dir = TempDir::new().unwrap();
        let config = MicroVmConfig::builder()
            .log_level(LogLevel::Info)
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .exec_path("/bin/echo")
            .build();

        assert!(config.validate().is_ok());
    }

    /// 测试配置验证失败：根路径不存在
    ///
    /// 使用不存在的路径作为根文件系统，
    /// 验证 validate() 返回正确的错误类型。
    #[test]
    fn test_microvm_config_validation_failure_root_path() {
        let config = MicroVmConfig::builder()
            .log_level(LogLevel::Info)
            .rootfs(Rootfs::Native(PathBuf::from("/non/existent/path")))
            .memory_mib(512)
            .exec_path("/bin/echo")
            .build();

        assert!(matches!(
            config.validate(),
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::RootPathDoesNotExist(_)
            ))
        ));
    }

    /// 测试配置验证失败：内存为零
    ///
    /// 设置 memory_mib 为 0，验证 validate() 返回正确的错误类型。
    #[test]
    fn test_microvm_config_validation_failure_zero_ram() {
        let temp_dir = TempDir::new().unwrap();
        let config = MicroVmConfig::builder()
            .log_level(LogLevel::Info)
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .memory_mib(0)
            .exec_path("/bin/echo")
            .build();

        assert!(matches!(
            config.validate(),
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::MemoryIsZero
            ))
        ));
    }

    /// 测试命令行字符串验证：有效字符串
    ///
    /// 验证各种有效的可打印 ASCII 字符能够正确通过验证：
    /// - 基本 ASCII 字符串
    /// - 范围边界字符（空格和波浪号）
    /// - 特殊字符（标点符号等）
    #[test]
    fn test_validate_command_line_valid_strings() {
        // 测试基本 ASCII 字符串
        assert!(MicroVmConfig::validate_command_line("hello").is_ok());
        assert!(MicroVmConfig::validate_command_line("hello world").is_ok());
        assert!(MicroVmConfig::validate_command_line("Hello, World!").is_ok());

        // 测试有效范围的边界（空格到波浪线）
        assert!(MicroVmConfig::validate_command_line(" ").is_ok()); // 空格 (0x20)
        assert!(MicroVmConfig::validate_command_line("~").is_ok()); // 波浪线 (0x7E)

        // 测试有效范围内的特殊字符
        assert!(MicroVmConfig::validate_command_line("!@#$%^&*()").is_ok());
        assert!(MicroVmConfig::validate_command_line("path/to/file").is_ok());
        assert!(MicroVmConfig::validate_command_line("user-name_123").is_ok());
    }

    /// 测试命令行字符串验证：无效字符串
    ///
    /// 验证各种无效的字符串会被正确拒绝：
    /// - 控制字符（换行、制表、回车、转义）
    /// - 非 ASCII Unicode 字符（emoji、变音符号、中文）
    /// - 混合有效和无效字符的字符串
    #[test]
    fn test_validate_command_line_invalid_strings() {
        // 测试控制字符
        assert!(MicroVmConfig::validate_command_line("\n").is_err()); // 换行符
        assert!(MicroVmConfig::validate_command_line("\t").is_err()); // 制表符
        assert!(MicroVmConfig::validate_command_line("\r").is_err()); // 回车符
        assert!(MicroVmConfig::validate_command_line("\x1B").is_err()); // 转义符

        // 测试非 ASCII Unicode 字符
        assert!(MicroVmConfig::validate_command_line("hello🌎").is_err()); // emoji
        assert!(MicroVmConfig::validate_command_line("über").is_err()); // 变音符号
        assert!(MicroVmConfig::validate_command_line("café").is_err()); // 重音符号
        assert!(MicroVmConfig::validate_command_line("你好").is_err()); // 中文字符

        // 测试混合有效和无效字符的字符串
        assert!(MicroVmConfig::validate_command_line("hello\nworld").is_err());
        assert!(MicroVmConfig::validate_command_line("path/to/file\0").is_err()); // 空字节
        assert!(MicroVmConfig::validate_command_line("hello\x7F").is_err()); // DEL 字符
    }

    /// 测试配置中的命令行字符串验证
    ///
    /// 验证在完整的配置上下文中，无效的可执行文件路径和参数
    /// 会被正确检测到并返回错误。
    #[test]
    fn test_validate_command_line_in_config() {
        let temp_dir = TempDir::new().unwrap();

        // 测试无效的可执行文件路径（包含换行符）
        let config = MicroVmConfig::builder()
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .memory_mib(512)
            .exec_path("/bin/hello\nworld")
            .build();
        assert!(matches!(
            config.validate(),
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::InvalidCommandLineString(_)
            ))
        ));

        // 测试无效参数（包含制表符）
        let config = MicroVmConfig::builder()
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .memory_mib(512)
            .exec_path("/bin/echo")
            .args(["hello\tworld"])
            .build();
        assert!(matches!(
            config.validate(),
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::InvalidCommandLineString(_)
            ))
        ));
    }

    /// 测试访客路径验证
    ///
    /// 验证各种路径组合的正确性：
    /// - 有效路径（无冲突）
    /// - 重复路径
    /// - 子集关系路径
    /// - 父级关系路径
    /// - 需要规范化的路径
    /// - 规范化后的冲突
    #[test]
    fn test_validate_guest_paths() -> anyhow::Result<()> {
        // 测试有效路径（无冲突）
        // 这些路径互不重叠，应该通过验证
        let valid_paths = vec![
            "/app".parse::<PathPair>()?,
            "/data".parse()?,
            "/var/log".parse()?,
            "/etc/config".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&valid_paths).is_ok());

        // 测试冲突路径（直接匹配）
        // 两个 "/app" 路径完全相同，应该被拒绝
        let conflicting_paths = vec![
            "/app".parse()?,
            "/data".parse()?,
            "/app".parse()?, // 重复
        ];
        assert!(MicroVmConfig::validate_guest_paths(&conflicting_paths).is_err());

        // 测试冲突路径（子集）
        // "/app/data" 是 "/app" 的子集，应该被拒绝
        let subset_paths = vec![
            "/app".parse()?,
            "/app/data".parse()?, // /app 的子集
            "/var/log".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&subset_paths).is_err());

        // 测试冲突路径（父级）
        // "/var" 是 "/var/log" 的父级，应该被拒绝
        let parent_paths = vec![
            "/var/log".parse()?,
            "/var".parse()?, // /var/log 的父级
            "/etc".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&parent_paths).is_err());

        // 测试需要规范化的路径
        // 这些路径包含 "." 和 "//"，但规范化后不冲突，应该通过验证
        let unnormalized_paths = vec![
            "/app/./data".parse()?,
            "/var/log".parse()?,
            "/etc//config".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&unnormalized_paths).is_ok());

        // 测试规范化后的冲突
        // "/app/./data" 和 "/app/data/" 规范化后相同，应该被拒绝
        let normalized_conflicts = vec![
            "/app/./data".parse()?,
            "/app/data/".parse()?, // 规范化后与第一个路径相同
            "/var/log".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&normalized_conflicts).is_err());

        Ok(())
    }

    /// 测试带有访客路径的完整配置验证
    ///
    /// 使用真实的临时目录和映射目录，
    /// 验证完整的配置验证流程。
    #[test]
    fn test_microvm_config_validation_with_guest_paths() -> anyhow::Result<()> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let host_dir1 = temp_dir.path().join("dir1");
        let host_dir2 = temp_dir.path().join("dir2");
        std::fs::create_dir_all(&host_dir1)?;
        std::fs::create_dir_all(&host_dir2)?;

        // 测试有效配置
        // 两个映射目录的访客路径不冲突（/app 和 /data）
        let valid_config = MicroVmConfig::builder()
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .memory_mib(1024)
            .exec_path("/bin/echo")
            .mapped_dirs([
                format!("{}:/app", host_dir1.display()).parse()?,
                format!("{}:/data", host_dir2.display()).parse()?,
            ])
            .build();

        assert!(valid_config.validate().is_ok());

        // 测试访客路径冲突的无效配置
        // /app/data 是 /app 的子集，验证应该失败
        let invalid_config = MicroVmConfig::builder()
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .memory_mib(1024)
            .exec_path("/bin/echo")
            .mapped_dirs([
                format!("{}:/app/data", host_dir1.display()).parse()?,
                format!("{}:/app", host_dir2.display()).parse()?,
            ])
            .build();

        // 验证返回 ConflictingGuestPaths 错误，包含两个冲突的路径
        assert!(matches!(
            invalid_config.validate(),
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::ConflictingGuestPaths(_, _)
            ))
        ));

        Ok(())
    }
}
