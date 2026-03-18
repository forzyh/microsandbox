//! MicroVM 构建器模块
//!
//! 本模块提供了用于构建 `MicroVm` 实例的 Builder 模式实现。
//! Builder 模式是一种创建型设计模式，用于解决构造函数参数过多的问题。
//!
//! ## Builder 模式的优势
//!
//! 1. **参数可选性**: 不需要为所有参数提供值，只需设置需要的配置
//! 2. **类型安全**: 通过泛型参数和类型状态模式，在编译期确保必需字段被设置
//! 3. **链式调用**: 支持流畅的链式方法调用，代码可读性高
//! 4. **不可变性**: 每次调用 builder 方法返回新实例，避免状态污染
//!
//! ## 类型状态模式（Type State Pattern）
//!
//! 本模块使用 Rust 的类型系统来确保正确配置：
//!
//! ```text
//! MicroVmConfigBuilder<(), ()>   // 初始状态：rootfs 和 exec_path 未设置
//!       ↓ rootfs()
//! MicroVmConfigBuilder<Rootfs, ()> // rootfs 已设置，exec_path 未设置
//!       ↓ exec_path()
//! MicroVmConfigBuilder<Rootfs, Utf8UnixPathBuf> // 所有必需字段已设置
//!       ↓ build()
//! MicroVmConfig                    // 构建完成
//! ```
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
//! use microsandbox_core::config::NetworkScope;
//! use tempfile::TempDir;
//!
//! # fn main() -> anyhow::Result<()> {
//! let temp_dir = TempDir::new()?;
//! let vm = MicroVmBuilder::default()
//!     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
//!     .memory_mib(512)
//!     .num_vcpus(1)
//!     .exec_path("/bin/sh")
//!     .args(["-c", "echo Hello"])
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use std::net::Ipv4Addr;

use ipnetwork::Ipv4Network;
use microsandbox_utils::{DEFAULT_MEMORY_MIB, DEFAULT_NUM_VCPUS};
use typed_path::Utf8UnixPathBuf;

use crate::{
    MicrosandboxResult,
    config::{EnvPair, NetworkScope, PathPair, PortPair},
};

use super::{
    LinuxRlimit,
    microvm::{LogLevel, MicroVm, MicroVmConfig, Rootfs},
};

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// MicroVM 配置构建器
///
/// 此结构体用于逐步构建 `MicroVmConfig` 实例。
/// 通过链式调用的方式设置各种配置参数，最后调用 `build()` 方法完成构建。
///
/// ## 泛型参数
///
/// - `R`: rootfs 字段的类型
///   - `()` - 初始状态，尚未设置 rootfs
///   - `Rootfs` - 已设置 rootfs
/// - `E`: exec_path 字段的类型
///   - `()` - 初始状态，尚未设置 exec_path
///   - `Utf8UnixPathBuf` - 已设置 exec_path
///
/// ## 类型状态流转
///
/// ```text
/// MicroVmConfigBuilder<(), ()>  --rootfs()-->  MicroVmConfigBuilder<Rootfs, ()>
///       |                                              |
///    exec_path()                                   exec_path()
///       ↓                                              ↓
/// MicroVmConfigBuilder<(), Utf8UnixPathBuf>  -->  MicroVmConfigBuilder<Rootfs, Utf8UnixPathBuf>
///                                                       |
///                                                    build()
///                                                       ↓
///                                                MicroVmConfig
/// ```
///
/// ## 必需字段
///
/// 以下字段必须在使用 `build()` 之前设置：
/// - `rootfs`: 根文件系统，包含要运行的操作系统环境
/// - `exec_path`: 要在 MicroVM 中执行的可执行程序路径
///
/// ## 可选字段
///
/// 以下字段有默认值，可根据需要设置：
/// - `num_vcpus`: 虚拟 CPU 数量（默认 1）
/// - `memory_mib`: 内存大小（默认 512 MiB）
/// - `mapped_dirs`: 目录挂载列表
/// - `port_map`: 端口映射列表
/// - `rlimits`: 资源限制列表
/// - `workdir_path`: 工作目录路径
/// - `args`: 命令行参数列表
/// - `env`: 环境变量列表
/// - `console_output`: 控制台输出文件路径
#[derive(Debug)]
pub struct MicroVmConfigBuilder<R, E> {
    /// 日志级别
    ///
    /// 控制 MicroVM 的日志输出详细程度
    log_level: LogLevel,
    /// 根文件系统
    ///
    /// 可以是原生目录或 overlayfs 多层文件系统
    rootfs: R,
    /// 虚拟 CPU 数量
    ///
    /// 分配给虚拟机的 CPU 核心数
    num_vcpus: u8,
    /// 内存大小（单位：MiB）
    ///
    /// 分配给虚拟机的内存容量
    memory_mib: u32,
    /// 目录挂载列表
    ///
    /// 将主机目录挂载到虚拟机内部
    mapped_dirs: Vec<PathPair>,
    /// 端口映射列表
    ///
    /// 将虚拟机端口映射到主机
    port_map: Vec<PortPair>,
    /// 网络作用域
    ///
    /// 控制虚拟机的网络隔离级别
    scope: NetworkScope,
    /// IP 地址
    ///
    /// 虚拟机的 IPv4 地址（可选）
    ip: Option<Ipv4Addr>,
    /// 子网
    ///
    /// 虚拟机所属的子网（可选）
    subnet: Option<Ipv4Network>,
    /// 资源限制列表
    ///
    /// 限制虚拟机进程可使用的系统资源
    rlimits: Vec<LinuxRlimit>,
    /// 工作目录路径
    ///
    /// 虚拟机进程的当前工作目录
    workdir_path: Option<Utf8UnixPathBuf>,
    /// 可执行程序路径
    ///
    /// 要在虚拟机中运行的程序
    exec_path: E,
    /// 命令行参数列表
    ///
    /// 传递给可执行程序的参数
    args: Vec<String>,
    /// 环境变量列表
    ///
    /// 虚拟机进程的环境变量
    env: Vec<EnvPair>,
    /// 控制台输出文件路径
    ///
    /// 保存虚拟机控制台输出的文件路径
    console_output: Option<Utf8UnixPathBuf>,
}

/// MicroVM 构建器
///
/// 此结构体提供流畅的链式接口来配置和创建 `MicroVm` 实例。
/// `MicroVmBuilder` 是对 `MicroVmConfigBuilder` 的封装，
/// 最终通过 `build()` 方法创建实际运行的 `MicroVm` 实例。
///
/// ## 与 MicroVmConfigBuilder 的关系
///
/// - `MicroVmConfigBuilder`: 构建配置对象 `MicroVmConfig`
/// - `MicroVmBuilder`: 构建可运行的 `MicroVm` 实例
///
/// `MicroVmBuilder` 内部持有 `MicroVmConfigBuilder` 作为 `inner` 字段，
/// 所有配置方法都委托给 `inner` 处理。
///
/// ## 泛型参数
///
/// - `R`: rootfs 字段的类型（`()` 或 `Rootfs`）
/// - `E`: exec_path 字段的类型（`()` 或 `Utf8UnixPathBuf`）
///
/// ## 必需字段
///
/// 以下字段必须在使用 `build()` 之前设置：
/// - `rootfs`: 根文件系统，包含要运行的操作系统环境
/// - `exec_path`: 要在 MicroVM 中执行的可执行程序路径
///
/// ## 可选字段
///
/// - `num_vcpus`: 虚拟 CPU 数量
/// - `memory_mib`: 内存大小（MiB）
/// - `mapped_dirs`: 目录挂载列表
/// - `port_map`: 端口映射列表
/// - `scope`: 网络作用域
/// - `ip`: IP 地址
/// - `subnet`: 子网
/// - `rlimits`: 资源限制列表
/// - `workdir_path`: 工作目录路径
/// - `args`: 命令行参数列表
/// - `env`: 环境变量列表
/// - `console_output`: 控制台输出文件路径
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_core::vm::{MicroVmBuilder, LogLevel, Rootfs};
/// use microsandbox_core::config::NetworkScope;
/// use tempfile::TempDir;
///
/// # fn main() -> anyhow::Result<()> {
/// let temp_dir = TempDir::new()?;
/// let vm = MicroVmBuilder::default()
///     .log_level(LogLevel::Debug)
///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
///     .num_vcpus(2)
///     .memory_mib(1024)
///     .mapped_dirs(["/home:/guest/mount".parse()?])
///     .port_map(["8080:80".parse()?])
///     .scope(NetworkScope::Public)
///     .ip("192.168.1.100".parse()?)
///     .subnet("192.168.1.0/24".parse()?)
///     .rlimits(["RLIMIT_NOFILE=1024:1024".parse()?])
///     .workdir_path("/workdir")
///     .exec_path("/bin/example")
///     .args(["arg1", "arg2"])
///     .env(["KEY1=VALUE1".parse()?, "KEY2=VALUE2".parse()?])
///     .console_output("/tmp/console.log")
///     .build()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct MicroVmBuilder<R, E> {
    /// 内部的配置构建器
    ///
    /// 所有配置参数实际存储在此字段中
    inner: MicroVmConfigBuilder<R, E>,
}

//--------------------------------------------------------------------------------------------------
// MicroVmConfigBuilder 方法实现
//--------------------------------------------------------------------------------------------------

impl<R, M> MicroVmConfigBuilder<R, M> {
    /// 设置 MicroVM 的日志级别
    ///
    /// 日志级别控制 MicroVM 运行时的日志输出详细程度。
    ///
    /// ## 参数
    ///
    /// * `log_level` - 日志级别，可选值：
    ///   - `Off`: 关闭所有日志（默认）
    ///   - `Error`: 仅错误消息
    ///   - `Warn`: 警告和错误
    ///   - `Info`: 信息性消息、警告和错误
    ///   - `Debug`: 调试信息及所有 above
    ///   - `Trace`: 详细跟踪信息及所有 above
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmConfigBuilder, LogLevel};
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .log_level(LogLevel::Debug);  // 启用调试日志
    /// ```
    pub fn log_level(mut self, log_level: LogLevel) -> Self {
        self.log_level = log_level;
        self
    }

    /// 设置根文件系统类型
    ///
    /// 此方法决定根文件系统如何与虚拟机共享，有两种选项：
    ///
    /// ## 根文件系统选项
    ///
    /// - `Rootfs::Native`: 直接将一个目录作为根文件系统（透传模式）
    /// - `Rootfs::Overlayfs`: 使用 overlayfs 多层文件系统作为根文件系统
    ///
    /// ## 参数
    ///
    /// * `rootfs` - 根文件系统配置
    ///
    /// ## 返回值
    ///
    /// 返回 `MicroVmConfigBuilder<Rootfs, M>`，表示 rootfs 已设置。
    /// 注意返回类型变化，这是类型状态模式的一部分。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmConfigBuilder, Rootfs};
    /// use std::path::PathBuf;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     // 选项 1：直接透传一个目录作为根文件系统
    ///     .rootfs(Rootfs::Native(PathBuf::from("/path/to/rootfs")));
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     // 选项 2：使用 overlayfs 多层文件系统
    ///     .rootfs(Rootfs::Overlayfs(vec![
    ///         PathBuf::from("/path/to/layer1"),  // 底层
    ///         PathBuf::from("/path/to/layer2")   // 上层（优先级高）
    ///     ]));
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 对于 `Native` 模式：目录必须存在并包含有效的根文件系统结构
    /// - 对于 `Overlayfs` 模式：层按顺序堆叠，后面的层优先级更高
    /// - 常见选择包括 Alpine Linux 或 Ubuntu rootfs
    pub fn rootfs(self, rootfs: Rootfs) -> MicroVmConfigBuilder<Rootfs, M> {
        MicroVmConfigBuilder {
            log_level: self.log_level,
            rootfs,
            num_vcpus: self.num_vcpus,
            memory_mib: self.memory_mib,
            mapped_dirs: self.mapped_dirs,
            port_map: self.port_map,
            scope: self.scope,
            ip: self.ip,
            subnet: self.subnet,
            rlimits: self.rlimits,
            workdir_path: self.workdir_path,
            exec_path: self.exec_path,
            args: self.args,
            env: self.env,
            console_output: self.console_output,
        }
    }

    /// 设置虚拟 CPU 数量
    ///
    /// 此方法决定分配给虚拟机的 CPU 核心数。
    ///
    /// ## 参数
    ///
    /// * `num_vcpus` - 虚拟 CPU 核心数量
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .num_vcpus(2);  // 分配 2 个虚拟 CPU 核心
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 默认值为 1 个 vCPU
    /// - vCPU 数量不应超过宿主机的物理 CPU 核心数
    /// - 更多的 vCPU 并不总是更好，需考虑工作负载的实际需求
    pub fn num_vcpus(mut self, num_vcpus: u8) -> Self {
        self.num_vcpus = num_vcpus;
        self
    }

    /// 设置内存大小（单位：MiB）
    ///
    /// 此方法决定分配给虚拟机的内存容量。
    ///
    /// ## 参数
    ///
    /// * `memory_mib` - 内存大小，单位为 MiB（1 GiB = 1024 MiB）
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .memory_mib(1024);  // 分配 1 GiB 内存
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 值以 MiB 为单位（1 GiB = 1024 MiB）
    /// - 设置时需考虑宿主机的可用内存
    /// - 常见值：512 MiB 用于最小系统，1024-2048 MiB 用于典型工作负载
    pub fn memory_mib(mut self, memory_mib: u32) -> Self {
        self.memory_mib = memory_mib;
        self
    }

    /// 设置目录映射（使用 virtio-fs）
    ///
    /// 每个映射遵循 Docker 的卷映射约定，使用 `host:guest` 格式。
    ///
    /// ## 参数
    ///
    /// * `mapped_dirs` - 目录映射列表，每个元素为 `PathPair` 类型
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let config = MicroVmConfigBuilder::default()
    ///     .mapped_dirs([
    ///         // 将主机的 /data 目录挂载为虚拟机内的 /mnt/data
    ///         "/data:/mnt/data".parse()?,
    ///         // 将当前目录挂载为虚拟机内的 /app
    ///         "./:/app".parse()?,
    ///         // 在主机和虚拟机内使用相同路径
    ///         "/shared".parse()?
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 主机路径必须存在且可访问
    /// - 虚拟机路径如果不存在会自动创建
    /// - 共享目录中的更改对两个系统立即可见
    /// - 适用于开发、配置文件和数据共享场景
    pub fn mapped_dirs(mut self, mapped_dirs: impl IntoIterator<Item = PathPair>) -> Self {
        self.mapped_dirs = mapped_dirs.into_iter().collect();
        self
    }

    /// 设置端口映射
    ///
    /// 端口映射遵循 Docker 的约定，使用 `host:guest` 格式，其中：
    /// - `host` 是宿主机上的端口号
    /// - `guest` 是 MicroVM 内部的端口号
    ///
    /// ## 参数
    ///
    /// * `port_map` - 端口映射列表，每个元素为 `PortPair` 类型
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    /// use microsandbox_core::config::PortPair;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let config = MicroVmConfigBuilder::default()
    ///     .port_map([
    ///         // 将主机端口 8080 映射到虚拟机端口 80（Web 服务器）
    ///         "8080:80".parse()?,
    ///         // 将主机端口 2222 映射到虚拟机端口 22（SSH）
    ///         "2222:22".parse()?,
    ///         // 在主机和虚拟机内使用相同端口（3000）
    ///         "3000".parse()?
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 如果不调用此方法，主机和虚拟机之间不会有端口映射
    /// - 虚拟机内的应用程序需要使用虚拟机端口号监听连接
    /// - 外部连接应使用主机端口号连接到服务
    pub fn port_map(mut self, port_map: impl IntoIterator<Item = PortPair>) -> Self {
        self.port_map = port_map.into_iter().collect();
        self
    }

    /// 设置网络作用域
    ///
    /// 网络作用域控制 MicroVM 的网络隔离级别和连接能力。
    ///
    /// ## 参数
    ///
    /// * `scope` - 网络作用域级别
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    /// use microsandbox_core::config::NetworkScope;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .scope(NetworkScope::Public);  // 允许访问公共网络
    /// ```
    ///
    /// ## 网络作用域选项
    ///
    /// - `None` - 沙箱无法与其他沙箱通信
    /// - `Group` - 沙箱只能在其子网内通信（默认）
    /// - `Public` - 沙箱可以与任何非私有地址通信
    /// - `Any` - 沙箱可以与任何地址通信
    ///
    /// ## 注意事项
    ///
    /// - 根据安全需求选择合适的作用域
    /// - 更严格的作用域提供更好的隔离性
    /// - 如果未指定，默认作用域为 `Group`
    pub fn scope(mut self, scope: NetworkScope) -> Self {
        self.scope = scope;
        self
    }

    /// 设置 IP 地址
    ///
    /// 为虚拟机设置特定的 IPv4 地址。
    ///
    /// ## 参数
    ///
    /// * `ip` - 要分配给虚拟机的 IPv4 地址
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    /// use std::net::Ipv4Addr;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .ip(Ipv4Addr::new(192, 168, 1, 100));  // 分配 IP 192.168.1.100 给虚拟机
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - IP 地址应该在分配给虚拟机的子网范围内
    /// - 如果未指定，IP 地址可能会被自动分配
    /// - 当运行多个 MicroVM 时，可用于可预测的寻址
    /// - 建议与 `subnet` 方法一起使用来定义网络
    pub fn ip(mut self, ip: Ipv4Addr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// 设置子网
    ///
    /// 为虚拟机设置 IPv4 子网和掩码。
    ///
    /// ## 参数
    ///
    /// * `subnet` - 子网配置，如 `192.168.1.0/24`
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    /// use ipnetwork::Ipv4Network;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let config = MicroVmConfigBuilder::default()
    ///     .subnet("192.168.1.0/24".parse()?);  // 设置子网为 192.168.1.0/24
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 子网定义了虚拟机可用的 IP 地址范围
    /// - 常见子网掩码：/24（256 个地址）、/16（65536 个地址）
    /// - 分配给虚拟机的 IP 地址应该在此子网内
    /// - 对于同一组内多个 MicroVM 之间的网络通信很重要
    pub fn subnet(mut self, subnet: Ipv4Network) -> Self {
        self.subnet = Some(subnet);
        self
    }

    /// 设置资源限制
    ///
    /// 资源限制控制虚拟机中进程可使用的系统资源，遵循 Linux rlimit 约定。
    ///
    /// ## 格式
    ///
    /// 资源限制使用格式 `RESOURCE=SOFT:HARD` 或 `NUMBER=SOFT:HARD`，其中：
    /// - `RESOURCE` 是资源名称（如 `RLIMIT_NOFILE`）
    /// - `NUMBER` 是资源编号（如 7 对应 `RLIMIT_NOFILE`）
    /// - `SOFT` 是软限制（内核执行的限制值）
    /// - `HARD` 是硬限制（软限制的上限）
    ///
    /// ## 参数
    ///
    /// * `rlimits` - 资源限制列表，每个元素为 `LinuxRlimit` 类型
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let config = MicroVmConfigBuilder::default()
    ///     .rlimits([
    ///         // 限制打开文件数量
    ///         "RLIMIT_NOFILE=1024:2048".parse()?,
    ///         // 限制进程内存
    ///         "RLIMIT_AS=1073741824:2147483648".parse()?,  // 1GB:2GB
    ///         // 也可以使用资源编号
    ///         "7=1024:2048".parse()?  // 同 RLIMIT_NOFILE
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 常见资源限制
    ///
    /// - `RLIMIT_NOFILE` (7) - 最大打开文件描述符数量
    /// - `RLIMIT_AS` (9) - 进程虚拟内存最大大小
    /// - `RLIMIT_NPROC` (6) - 最大进程数
    /// - `RLIMIT_CPU` (0) - CPU 时间限制（秒）
    /// - `RLIMIT_FSIZE` (1) - 最大文件大小
    pub fn rlimits(mut self, rlimits: impl IntoIterator<Item = LinuxRlimit>) -> Self {
        self.rlimits = rlimits.into_iter().collect();
        self
    }

    /// 设置工作目录
    ///
    /// 此目录将成为虚拟机中任何进程的当前工作目录（cwd）。
    ///
    /// ## 参数
    ///
    /// * `workdir_path` - 工作目录路径，必须是绝对路径
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .workdir_path("/app")  // 设置工作目录为 /app
    ///     .exec_path("/app/myapp")  // 从 /app 运行可执行文件
    ///     .args(["--config", "config.json"]);  // 配置文件将在 /app 中查找
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 路径必须是绝对路径
    /// - 目录必须在虚拟机文件系统中存在
    /// - 适用于需要访问相对位置文件的应用程序
    pub fn workdir_path(mut self, workdir_path: impl Into<Utf8UnixPathBuf>) -> Self {
        self.workdir_path = Some(workdir_path.into());
        self
    }

    /// 设置可执行程序路径
    ///
    /// 指定 MicroVM 启动时要执行的程序。
    ///
    /// ## 参数
    ///
    /// * `exec_path` - 可执行程序的路径，必须是绝对路径
    ///
    /// ## 返回值
    ///
    /// 返回 `MicroVmConfigBuilder<R, Utf8UnixPathBuf>`，表示 exec_path 已设置。
    /// 注意返回类型变化，这是类型状态模式的一部分。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .exec_path("/usr/local/bin/nginx")  // 运行 nginx Web 服务器
    ///     .args(["-c", "/etc/nginx/nginx.conf"]);  // 使用指定配置文件
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 路径必须是绝对路径
    /// - 可执行文件必须在虚拟机文件系统中存在且可执行
    /// - 路径是相对于虚拟机根文件系统的路径
    pub fn exec_path(
        self,
        exec_path: impl Into<Utf8UnixPathBuf>,
    ) -> MicroVmConfigBuilder<R, Utf8UnixPathBuf> {
        MicroVmConfigBuilder {
            log_level: self.log_level,
            rootfs: self.rootfs,
            num_vcpus: self.num_vcpus,
            memory_mib: self.memory_mib,
            mapped_dirs: self.mapped_dirs,
            port_map: self.port_map,
            scope: self.scope,
            ip: self.ip,
            subnet: self.subnet,
            rlimits: self.rlimits,
            workdir_path: self.workdir_path,
            exec_path: exec_path.into(),
            args: self.args,
            env: self.env,
            console_output: self.console_output,
        }
    }

    /// 设置命令行参数
    ///
    /// 这些参数将传递给 `exec_path` 指定的程序。
    ///
    /// ## 参数
    ///
    /// * `args` - 命令行参数列表
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .exec_path("/usr/bin/python3")
    ///     .args([
    ///         "-m", "http.server",  // 运行 Python 的 HTTP 服务器模块
    ///         "8080",               // 监听端口 8080
    ///         "--directory", "/data" // 从 /data 提供文件
    ///     ]);
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 参数按它们在迭代器中出现的顺序传递
    /// - 程序名（argv[0]）会自动从 exec_path 设置
    /// - 每个参数应该是独立的字符串
    pub fn args<'a>(mut self, args: impl IntoIterator<Item = &'a str>) -> Self {
        self.args = args.into_iter().map(|s| s.to_string()).collect();
        self
    }

    /// 设置环境变量
    ///
    /// 环境变量遵循标准格式 `KEY=VALUE`，对虚拟机中的所有进程可用。
    ///
    /// ## 参数
    ///
    /// * `env` - 环境变量列表，每个元素为 `EnvPair` 类型
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let config = MicroVmConfigBuilder::default()
    ///     .env([
    ///         // 设置应用环境
    ///         "APP_ENV=production".parse()?,
    ///         // 配置日志
    ///         "LOG_LEVEL=info".parse()?,
    ///         // 设置时区
    ///         "TZ=UTC".parse()?,
    ///         // 可以有多个值
    ///         "ALLOWED_HOSTS=localhost,127.0.0.1".parse()?
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 变量对虚拟机中的所有进程可用
    /// - 如果值包含特殊字符，应正确转义
    /// - 常见用途包括配置和运行时设置
    /// - 某些程序需要特定的环境变量才能正常工作
    pub fn env(mut self, env: impl IntoIterator<Item = EnvPair>) -> Self {
        self.env = env.into_iter().collect();
        self
    }

    /// 设置控制台输出文件路径
    ///
    /// 此方法允许将虚拟机中的所有控制台输出（stdout/stderr）重定向
    /// 并保存到主机上的一个文件中。
    ///
    /// ## 参数
    ///
    /// * `console_output` - 控制台输出文件路径
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmConfigBuilder;
    ///
    /// let config = MicroVmConfigBuilder::default()
    ///     .console_output("/var/log/microvm.log")  // 保存输出到日志文件
    ///     .exec_path("/usr/local/bin/myapp");      // 运行应用程序
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 路径必须在主机系统上可写
    /// - 如果文件不存在会自动创建
    /// - 适用于调试和日志记录
    /// - 捕获 stdout 和 stderr 两种输出
    pub fn console_output(mut self, console_output: impl Into<Utf8UnixPathBuf>) -> Self {
        self.console_output = Some(console_output.into());
        self
    }
}

//--------------------------------------------------------------------------------------------------
// MicroVmBuilder 方法实现
//--------------------------------------------------------------------------------------------------

impl<R, M> MicroVmBuilder<R, M> {
    /// 设置 MicroVM 的日志级别
    ///
    /// 日志级别控制 MicroVM 运行时的日志输出详细程度。
    ///
    /// ## 参数
    ///
    /// * `log_level` - 日志级别
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{LogLevel, MicroVmBuilder, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let vm = MicroVmBuilder::default()
    ///     .log_level(LogLevel::Debug)  // 启用调试日志
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/bin/echo")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 日志级别
    ///
    /// - `Off`: 关闭所有日志（默认）
    /// - `Error`: 仅错误消息
    /// - `Warn`: 警告和错误
    /// - `Info`: 信息性消息、警告和错误
    /// - `Debug`: 调试信息及所有 above
    /// - `Trace`: 详细跟踪信息及所有 above
    pub fn log_level(mut self, log_level: LogLevel) -> Self {
        self.inner = self.inner.log_level(log_level);
        self
    }

    /// 设置根文件系统类型
    ///
    /// 此方法决定根文件系统如何与虚拟机共享。
    ///
    /// ## 根文件系统选项
    ///
    /// - `Rootfs::Native`: 直接将一个目录作为根文件系统（透传模式）
    /// - `Rootfs::Overlayfs`: 使用 overlayfs 多层文件系统作为根文件系统
    ///
    /// ## 参数
    ///
    /// * `rootfs` - 根文件系统配置
    ///
    /// ## 返回值
    ///
    /// 返回 `MicroVmBuilder<Rootfs, M>`，表示 rootfs 已设置。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// // 选项 1：直接透传一个目录
    /// let vm = MicroVmBuilder::default()
    ///     .rootfs(Rootfs::Native(PathBuf::from("/path/to/rootfs")));
    ///
    /// // 选项 2：使用 overlayfs 多层文件系统
    /// let vm = MicroVmBuilder::default()
    ///     .rootfs(Rootfs::Overlayfs(vec![
    ///         PathBuf::from("/path/to/layer1"),
    ///         PathBuf::from("/path/to/layer2")
    ///     ]));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 对于 `Native` 模式：目录必须存在并包含有效的根文件系统结构
    /// - 对于 `Overlayfs` 模式：层按顺序堆叠，后面的层优先级更高
    /// - 常见选择包括 Alpine Linux 或 Ubuntu rootfs
    /// - 这是必需字段 - 如果未设置，构建将失败
    pub fn rootfs(self, rootfs: Rootfs) -> MicroVmBuilder<Rootfs, M> {
        MicroVmBuilder {
            inner: self.inner.rootfs(rootfs),
        }
    }

    /// 设置虚拟 CPU 数量
    ///
    /// 此方法决定分配给虚拟机的 CPU 核心数。
    ///
    /// ## 参数
    ///
    /// * `num_vcpus` - 虚拟 CPU 核心数量
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let vm = MicroVmBuilder::default()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .num_vcpus(2)  // 分配 2 个虚拟 CPU 核心
    ///     .exec_path("/bin/echo")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 默认值为 1 个 vCPU
    /// - 更多的 vCPU 并不总是更好，需考虑工作负载的实际需求
    pub fn num_vcpus(mut self, num_vcpus: u8) -> Self {
        self.inner = self.inner.num_vcpus(num_vcpus);
        self
    }

    /// 设置内存大小（单位：MiB）
    ///
    /// 此方法决定分配给虚拟机的内存容量。
    ///
    /// ## 参数
    ///
    /// * `memory_mib` - 内存大小，单位为 MiB
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let vm = MicroVmBuilder::default()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)  // 分配 1 GiB 内存
    ///     .exec_path("/bin/echo")
    ///     .build()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 值以 MiB 为单位（1 GiB = 1024 MiB）
    /// - 设置时需考虑宿主机的可用内存
    pub fn memory_mib(mut self, memory_mib: u32) -> Self {
        self.inner = self.inner.memory_mib(memory_mib);
        self
    }

    /// 设置目录映射（使用 virtio-fs）
    ///
    /// 每个映射遵循 Docker 的卷映射约定，使用 `host:guest` 格式。
    ///
    /// ## 参数
    ///
    /// * `mapped_dirs` - 目录映射列表
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .mapped_dirs([
    ///         // 将主机的 /data 目录挂载为虚拟机内的 /mnt/data
    ///         "/data:/mnt/data".parse()?,
    ///         // 将当前目录挂载为虚拟机内的 /app
    ///         "./:/app".parse()?,
    ///         // 在主机和虚拟机内使用相同路径
    ///         "/shared".parse()?
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn mapped_dirs(mut self, mapped_dirs: impl IntoIterator<Item = PathPair>) -> Self {
        self.inner = self.inner.mapped_dirs(mapped_dirs);
        self
    }

    /// 设置端口映射
    ///
    /// 端口映射遵循 Docker 的约定，使用 `host:guest` 格式，其中：
    /// - `host` 是宿主机上的端口号
    /// - `guest` 是 MicroVM 内部的端口号
    ///
    /// ## 参数
    ///
    /// * `port_map` - 端口映射列表
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    /// use microsandbox_core::config::PortPair;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .port_map([
    ///         // 将主机端口 8080 映射到虚拟机端口 80（Web 服务器）
    ///         "8080:80".parse()?,
    ///         // 将主机端口 2222 映射到虚拟机端口 22（SSH）
    ///         "2222:22".parse()?,
    ///         // 在主机和虚拟机内使用相同端口（3000）
    ///         "3000".parse()?
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 如果不调用此方法，主机和虚拟机之间不会有端口映射
    /// - 虚拟机内的应用程序需要使用虚拟机端口号监听连接
    /// - 外部连接应使用主机端口号连接到服务
    /// - 使用 passt 网络模式时不支持端口映射
    pub fn port_map(mut self, port_map: impl IntoIterator<Item = PortPair>) -> Self {
        self.inner = self.inner.port_map(port_map);
        self
    }

    /// 设置网络作用域
    ///
    /// 网络作用域控制 MicroVM 的网络隔离级别和连接能力。
    ///
    /// ## 参数
    ///
    /// * `scope` - 网络作用域级别
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use microsandbox_core::config::NetworkScope;
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .scope(NetworkScope::Public)  // 允许访问公共网络
    ///     .rootfs(Rootfs::Native(PathBuf::from("/path/to/rootfs")))
    ///     .exec_path("/bin/echo");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 网络作用域选项
    ///
    /// - `None` - 沙箱无法与其他沙箱通信
    /// - `Group` - 沙箱只能在其子网内通信（默认）
    /// - `Public` - 沙箱可以与任何非私有地址通信
    /// - `Any` - 沙箱可以与任何地址通信
    pub fn scope(mut self, scope: NetworkScope) -> Self {
        self.inner = self.inner.scope(scope);
        self
    }

    /// 设置 IP 地址
    ///
    /// 为虚拟机设置特定的 IPv4 地址。
    ///
    /// ## 参数
    ///
    /// * `ip` - 要分配给虚拟机的 IPv4 地址
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use std::path::PathBuf;
    /// use std::net::Ipv4Addr;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .ip(Ipv4Addr::new(192, 168, 1, 100))  // 分配 IP 192.168.1.100 给虚拟机
    ///     .rootfs(Rootfs::Native(PathBuf::from("/path/to/rootfs")))
    ///     .exec_path("/bin/echo");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - IP 地址应该在分配给虚拟机的子网范围内
    /// - 如果未指定，IP 地址可能会被自动分配
    pub fn ip(mut self, ip: Ipv4Addr) -> Self {
        self.inner = self.inner.ip(ip);
        self
    }

    /// 设置子网
    ///
    /// 为虚拟机设置 IPv4 子网和掩码。
    ///
    /// ## 参数
    ///
    /// * `subnet` - 子网配置
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use std::path::PathBuf;
    /// use ipnetwork::Ipv4Network;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .subnet("192.168.1.0/24".parse()?)  // 设置子网为 192.168.1.0/24
    ///     .rootfs(Rootfs::Native(PathBuf::from("/path/to/rootfs")))
    ///     .exec_path("/bin/echo");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 子网定义了虚拟机可用的 IP 地址范围
    /// - 用于同一组内多个 MicroVM 之间的网络通信
    pub fn subnet(mut self, subnet: Ipv4Network) -> Self {
        self.inner = self.inner.subnet(subnet);
        self
    }

    /// 设置资源限制
    ///
    /// ## 参数
    ///
    /// * `rlimits` - 资源限制列表
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// MicroVmBuilder::default().rlimits(["RLIMIT_NOFILE=1024:1024".parse()?]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn rlimits(mut self, rlimits: impl IntoIterator<Item = LinuxRlimit>) -> Self {
        self.inner = self.inner.rlimits(rlimits);
        self
    }

    /// 设置工作目录路径
    ///
    /// ## 参数
    ///
    /// * `workdir_path` - 工作目录路径
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// MicroVmBuilder::default().workdir_path("/path/to/workdir");
    /// ```
    pub fn workdir_path(mut self, workdir_path: impl Into<Utf8UnixPathBuf>) -> Self {
        self.inner = self.inner.workdir_path(workdir_path);
        self
    }

    /// 设置可执行程序路径
    ///
    /// ## 参数
    ///
    /// * `exec_path` - 可执行程序路径
    ///
    /// ## 返回值
    ///
    /// 返回 `MicroVmBuilder<R, Utf8UnixPathBuf>`，表示 exec_path 已设置。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// MicroVmBuilder::default().exec_path("/path/to/exec");
    /// ```
    pub fn exec_path(
        self,
        exec_path: impl Into<Utf8UnixPathBuf>,
    ) -> MicroVmBuilder<R, Utf8UnixPathBuf> {
        MicroVmBuilder {
            inner: self.inner.exec_path(exec_path),
        }
    }

    /// 设置命令行参数
    ///
    /// 这些参数将传递给 `exec_path` 指定的程序。
    ///
    /// ## 参数
    ///
    /// * `args` - 命令行参数列表
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .args([
    ///         "-m", "http.server",  // 运行 Python 的 HTTP 服务器模块
    ///         "8080",               // 监听端口 8080
    ///         "--directory", "/data" // 从 /data 提供文件
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 参数按它们在迭代器中出现的顺序传递
    /// - 程序名（argv[0]）会自动从 exec_path 设置
    /// - 每个参数应该是独立的字符串
    pub fn args<'a>(mut self, args: impl IntoIterator<Item = &'a str>) -> Self {
        self.inner = self.inner.args(args);
        self
    }

    /// 设置环境变量
    ///
    /// 环境变量遵循标准格式 `KEY=VALUE`，对虚拟机中的所有进程可用。
    ///
    /// ## 参数
    ///
    /// * `env` - 环境变量列表
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .env([
    ///         // 设置应用环境
    ///         "APP_ENV=production".parse()?,
    ///         // 配置日志
    ///         "LOG_LEVEL=info".parse()?,
    ///         // 设置时区
    ///         "TZ=UTC".parse()?,
    ///         // 可以有多个值
    ///         "ALLOWED_HOSTS=localhost,127.0.0.1".parse()?
    ///     ]);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 变量对虚拟机中的所有进程可用
    /// - 如果值包含特殊字符，应正确转义
    /// - 常见用途包括配置和运行时设置
    /// - 某些程序需要特定的环境变量才能正常工作
    pub fn env(mut self, env: impl IntoIterator<Item = EnvPair>) -> Self {
        self.inner = self.inner.env(env);
        self
    }

    /// 设置控制台输出文件路径
    ///
    /// 此方法允许将虚拟机中的所有控制台输出（stdout/stderr）重定向
    /// 并保存到主机上的一个文件中。
    ///
    /// ## 参数
    ///
    /// * `console_output` - 控制台输出文件路径
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::MicroVmBuilder;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let vm = MicroVmBuilder::default()
    ///     .console_output("/var/log/microvm.log")  // 保存输出到日志文件
    ///     .exec_path("/usr/local/bin/myapp");      // 运行应用程序
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 路径必须在主机系统上可写
    /// - 如果文件不存在会自动创建
    /// - 适用于调试和日志记录
    /// - 捕获 stdout 和 stderr 两种输出
    pub fn console_output(mut self, console_output: impl Into<Utf8UnixPathBuf>) -> Self {
        self.inner = self.inner.console_output(console_output);
        self
    }
}

impl MicroVmConfigBuilder<Rootfs, Utf8UnixPathBuf> {
    /// 构建 MicroVM 配置
    ///
    /// 此方法在所有必需字段设置完成后调用，用于创建最终的
    /// `MicroVmConfig` 配置对象。
    ///
    /// ## 返回值
    ///
    /// 返回构建完成的 `MicroVmConfig` 实例。
    ///
    /// ## 使用示例
    ///
    /// ```rust
    /// use microsandbox_core::vm::{MicroVmConfigBuilder, Rootfs};
    /// use std::path::PathBuf;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let config = MicroVmConfigBuilder::default()
    ///     .rootfs(Rootfs::Native(PathBuf::from("/tmp")))
    ///     .exec_path("/bin/sh")
    ///     .build();  // 构建配置对象
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> MicroVmConfig {
        MicroVmConfig {
            log_level: self.log_level,
            rootfs: self.rootfs,
            num_vcpus: self.num_vcpus,
            memory_mib: self.memory_mib,
            mapped_dirs: self.mapped_dirs,
            port_map: self.port_map,
            scope: self.scope,
            ip: self.ip,
            subnet: self.subnet,
            rlimits: self.rlimits,
            workdir_path: self.workdir_path,
            exec_path: self.exec_path,
            args: self.args,
            env: self.env,
            console_output: self.console_output,
        }
    }
}

impl MicroVmBuilder<Rootfs, Utf8UnixPathBuf> {
    /// 构建 MicroVM 实例
    ///
    /// 此方法根据构建器中设置的配置创建 `MicroVm` 实例。
    /// 构建完成后，MicroVM 已准备好但尚未运行，需要调用 `start()` 方法来启动。
    ///
    /// ## 返回值
    ///
    /// 返回 `MicrosandboxResult<MicroVm>`：
    /// - 成功时返回配置好的 `MicroVm` 实例
    /// - 失败时返回错误，可能的原因包括配置缺失或路径不存在
    ///
    /// ## 使用示例
    ///
    /// ```no_run
    /// use microsandbox_core::vm::{MicroVmBuilder, Rootfs};
    /// use tempfile::TempDir;
    ///
    /// # fn main() -> anyhow::Result<()> {
    /// let temp_dir = TempDir::new()?;
    /// let vm = MicroVmBuilder::default()
    ///     .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
    ///     .memory_mib(1024)
    ///     .exec_path("/usr/bin/python3")
    ///     .args(["-c", "print('Hello from MicroVm!')"])
    ///     .build()?;
    ///
    /// // 启动 MicroVM
    /// vm.start()?;  // 这会实际运行虚拟机
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 注意事项
    ///
    /// - 如果缺少必需的配置，构建将失败
    /// - 如果根路径不存在，构建将失败
    /// - 如果内存值无效，构建将失败
    /// - 构建完成后，使用 `start()` 方法来运行 MicroVM
    pub fn build(self) -> MicrosandboxResult<MicroVm> {
        MicroVm::from_config(MicroVmConfig {
            log_level: self.inner.log_level,
            rootfs: self.inner.rootfs,
            num_vcpus: self.inner.num_vcpus,
            memory_mib: self.inner.memory_mib,
            mapped_dirs: self.inner.mapped_dirs,
            port_map: self.inner.port_map,
            scope: self.inner.scope,
            ip: self.inner.ip,
            subnet: self.inner.subnet,
            rlimits: self.inner.rlimits,
            workdir_path: self.inner.workdir_path,
            exec_path: self.inner.exec_path,
            args: self.inner.args,
            env: self.inner.env,
            console_output: self.inner.console_output,
        })
    }
}

//--------------------------------------------------------------------------------------------------
// Default Trait 实现
//--------------------------------------------------------------------------------------------------

impl Default for MicroVmConfigBuilder<(), ()> {
    /// 创建默认的构建器实例
    ///
    /// 所有必需字段初始化为单元类型 `()`，表示未设置。
    /// 可选字段使用以下默认值：
    /// - `log_level`: `LogLevel::default()`（Off）
    /// - `num_vcpus`: `DEFAULT_NUM_VCPUS`（1）
    /// - `memory_mib`: `DEFAULT_MEMORY_MIB`（512）
    /// - `scope`: `NetworkScope::default()`（Group）
    /// - 其他集合字段为空向量
    fn default() -> Self {
        Self {
            log_level: LogLevel::default(),
            rootfs: (),
            num_vcpus: DEFAULT_NUM_VCPUS,
            memory_mib: DEFAULT_MEMORY_MIB,
            mapped_dirs: vec![],
            port_map: vec![],
            scope: NetworkScope::default(),
            ip: None,
            subnet: None,
            rlimits: vec![],
            workdir_path: None,
            exec_path: (),
            args: vec![],
            env: vec![],
            console_output: None,
        }
    }
}

impl Default for MicroVmBuilder<(), ()> {
    /// 创建默认的构建器实例
    ///
    /// 内部使用 `MicroVmConfigBuilder::default()` 初始化。
    fn default() -> Self {
        Self {
            inner: MicroVmConfigBuilder::default(),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// 测试 MicroVmBuilder 的所有配置方法
    #[test]
    fn test_microvm_builder() -> anyhow::Result<()> {
        let rootfs = Rootfs::Overlayfs(vec![PathBuf::from("/tmp")]);
        let workdir_path = "/workdir";
        let exec_path = "/bin/example";

        let builder = MicroVmBuilder::default()
            .log_level(LogLevel::Debug)
            .rootfs(rootfs.clone())
            .num_vcpus(2)
            .memory_mib(1024)
            .mapped_dirs(["/guest/mount:/host/mount".parse()?])
            .port_map(["8080:80".parse()?])
            .rlimits(["RLIMIT_NOFILE=1024:1024".parse()?])
            .workdir_path(workdir_path)
            .exec_path(exec_path)
            .args(["arg1", "arg2"])
            .env(["KEY1=VALUE1".parse()?, "KEY2=VALUE2".parse()?])
            .console_output("/tmp/console.log");

        assert_eq!(builder.inner.log_level, LogLevel::Debug);
        assert_eq!(builder.inner.rootfs, rootfs);
        assert_eq!(builder.inner.num_vcpus, 2);
        assert_eq!(builder.inner.memory_mib, 1024);
        assert_eq!(
            builder.inner.mapped_dirs,
            ["/guest/mount:/host/mount".parse()?]
        );
        assert_eq!(builder.inner.port_map, ["8080:80".parse()?]);
        assert_eq!(builder.inner.rlimits, ["RLIMIT_NOFILE=1024:1024".parse()?]);
        assert_eq!(
            builder.inner.workdir_path,
            Some(Utf8UnixPathBuf::from(workdir_path))
        );
        assert_eq!(builder.inner.exec_path, Utf8UnixPathBuf::from(exec_path));
        assert_eq!(builder.inner.args, ["arg1", "arg2"]);
        assert_eq!(
            builder.inner.env,
            ["KEY1=VALUE1".parse()?, "KEY2=VALUE2".parse()?]
        );
        assert_eq!(
            builder.inner.console_output,
            Some(Utf8UnixPathBuf::from("/tmp/console.log"))
        );
        Ok(())
    }

    /// 测试 MicroVmBuilder 的最小配置（仅必需字段）
    #[test]
    fn test_microvm_builder_minimal() -> anyhow::Result<()> {
        let rootfs = Rootfs::Native(PathBuf::from("/tmp"));
        let memory_mib = 1024;

        let builder = MicroVmBuilder::default()
            .rootfs(rootfs.clone())
            .exec_path("/bin/echo");

        assert_eq!(builder.inner.rootfs, rootfs);
        assert_eq!(builder.inner.memory_mib, memory_mib);

        // 检查其他字段有默认值
        assert_eq!(builder.inner.log_level, LogLevel::default());
        assert_eq!(builder.inner.num_vcpus, DEFAULT_NUM_VCPUS);
        assert_eq!(builder.inner.memory_mib, DEFAULT_MEMORY_MIB);
        assert!(builder.inner.mapped_dirs.is_empty());
        assert!(builder.inner.mapped_dirs.is_empty());
        assert!(builder.inner.port_map.is_empty());
        assert!(builder.inner.rlimits.is_empty());
        assert_eq!(builder.inner.workdir_path, None);
        assert_eq!(builder.inner.exec_path, Utf8UnixPathBuf::from("/bin/echo"));
        assert!(builder.inner.args.is_empty());
        assert!(builder.inner.env.is_empty());
        assert_eq!(builder.inner.console_output, None);
        Ok(())
    }
}
