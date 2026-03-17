//! 轻量级 Linux 虚拟机实现
//!
//! 本模块提供了 MicroVm（微虚拟机）的实现，用于创建和管理安全隔离的
//! Linux 虚拟机环境。MicroVm 基于 libkrun 库，可以提供 VM 级别的隔离，
//! 同时保持快速的启动速度。
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
    InvalidMicroVMConfigError, MicrosandboxError, MicrosandboxResult,
    config::{EnvPair, NetworkScope, PathPair, PortPair},
    utils,
};

use super::{LinuxRlimit, MicroVmBuilder, MicroVmConfigBuilder, ffi};

//--------------------------------------------------------------------------------------------------
// 常量
//--------------------------------------------------------------------------------------------------

/// virtio-fs 挂载标签的前缀
///
/// 当挂载共享目录时，每个目录都会被分配一个标签，格式为 "virtiofs_N"
/// 其中 N 是目录的索引号
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
    /// 这是 libkrun 库内部使用的标识符，用于引用和管理 VM 实例
    ctx_id: u32,

    /// MicroVm 的配置
    ///
    /// 包含 VM 的所有配置参数，如 CPU、内存、挂载点等
    #[get = "pub with_prefix"]
    config: MicroVmConfig,
}

/// MicroVm 使用的根文件系统类型
///
/// 此枚举定义了可用于 MicroVm 的不同类型的根文件系统。
///
/// ## 变体说明
/// * `Native(PathBuf)` - 使用单个路径的原生根文件系统
/// * `Overlayfs(Vec<PathBuf>)` - 使用多个路径的 overlayfs 根文件系统
///
/// ## OverlayFS 说明
/// OverlayFS 是一种联合文件系统，可以将多个目录合并成一个
/// 统一的文件系统视图。在 OCI 镜像中，每个层都是一个目录，
/// 通过 OverlayFS 合并成最终的根文件系统。
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
/// let overlayfs_root = Rootfs::Overlayfs(vec![
///     PathBuf::from("/path/to/root1"),
///     PathBuf::from("/path/to/root2")
/// ]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rootfs {
    /// 使用底层原生文件系统的根文件系统
    ///
    /// 直接使用单个目录作为 VM 的根文件系统
    Native(PathBuf),

    /// 使用 overlayfs 的根文件系统
    ///
    /// 将多个目录（层）合并成一个统一的文件系统视图
    /// 第一个路径是最底层，后续路径逐层叠加
    Overlayfs(Vec<PathBuf>),
}

/// MicroVm 实例的配置结构
///
/// 此结构包含创建和运行 MicroVm 所需的所有设置，
/// 包括系统资源、文件系统配置、网络设置和进程执行详情。
///
/// 建议使用 `MicroVmConfigBuilder` 或 `MicroVmBuilder` 来
/// 构建配置，而不是直接创建此结构。
///
/// ## 字段说明
/// * `log_level` - 日志级别
/// * `rootfs` - 根文件系统类型
/// * `num_vcpus` - vCPU 数量
/// * `memory_mib` - 内存大小（MiB）
/// * `mapped_dirs` - 通过 virtio-fs 挂载的目录列表
/// * `port_map` - 端口转发映射
/// * `scope` - 网络范围
/// * `ip` - IP 地址（可选）
/// * `subnet` - 子网（可选）
/// * `rlimits` - 资源限制列表
/// * `workdir_path` - 工作目录
/// * `exec_path` - 可执行文件路径
/// * `args` - 命令行参数
/// * `env` - 环境变量
/// * `console_output` - 控制台输出路径
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
    /// 控制 libkrun 库的日志输出级别
    pub log_level: LogLevel,

    /// MicroVm 的根文件系统
    ///
    /// 可以是原生文件系统或 overlayfs
    pub rootfs: Rootfs,

    /// MicroVm 使用的 vCPU 数量
    ///
    /// 虚拟 CPU 核心数，通常为 1-8 之间
    pub num_vcpus: u8,

    /// MicroVm 使用的内存大小（MiB）
    ///
    /// 以 MiB（兆字节）为单位的内存量
    pub memory_mib: u32,

    /// 通过 virtio-fs 挂载的目录列表
    ///
    /// 每个 PathPair 代表一个主机到访客的路径映射
    /// virtio-fs 是一种高性能的虚拟化文件系统协议
    pub mapped_dirs: Vec<PathPair>,

    /// MicroVm 使用的端口映射
    ///
    /// 每个 PortPair 定义了一个端口转发规则
    pub port_map: Vec<PortPair>,

    /// MicroVm 使用的网络范围
    ///
    /// 定义 VM 可以访问的网络地址范围
    pub scope: NetworkScope,

    /// MicroVm 使用的 IP 地址
    ///
    /// 如果为 None，则自动分配
    pub ip: Option<Ipv4Addr>,

    /// MicroVm 使用的子网
    ///
    /// 如果为 None，则使用默认子网
    pub subnet: Option<Ipv4Network>,

    /// MicroVm 使用的资源限制列表
    ///
    /// 每个 LinuxRlimit 定义了一种资源的软/硬限制
    pub rlimits: Vec<LinuxRlimit>,

    /// MicroVm 使用的工作目录路径
    ///
    /// VM 中进程启动时的工作目录
    pub workdir_path: Option<Utf8UnixPathBuf>,

    /// MicroVm 使用的可执行文件路径
    ///
    /// VM 中要运行的程序路径
    pub exec_path: Utf8UnixPathBuf,

    /// 传递给可执行文件的参数列表
    ///
    /// 命令行参数，args[0] 通常是程序名
    pub args: Vec<String>,

    /// 为可执行文件设置的环境变量列表
    ///
    /// 每个 EnvPair 格式为 "NAME=value"
    pub env: Vec<EnvPair>,

    /// MicroVm 使用的控制台输出路径
    ///
    /// VM 控制台输出的目标文件路径
    pub console_output: Option<Utf8UnixPathBuf>,
}

/// MicroVm 使用的日志级别
///
/// 定义 libkrun 库的日志详细程度
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum LogLevel {
    /// 关闭日志
    ///
    /// 不输出任何日志消息
    #[default]
    Off = 0,

    /// 错误消息
    ///
    /// 只输出错误级别的日志
    Error = 1,

    /// 警告消息
    ///
    /// 输出错误和警告级别的日志
    Warn = 2,

    /// 信息消息
    ///
    /// 输出错误、警告和信息级别的日志
    Info = 3,

    /// 调试消息
    ///
    /// 输出所有级别的日志，包括调试信息
    Debug = 4,

    /// 追踪消息
    ///
    /// 最详细的日志级别，包括追踪信息
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
    /// * `config` - MicroVm 配置
    ///
    /// ## 返回值
    /// 成功时返回 MicroVm 实例，失败时返回错误
    ///
    /// ## 错误情况
    /// * 配置无效
    /// * 无法分配所需资源
    /// * 系统缺少必需的功能
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
    /// * `Ok(i32)` - 进程的退出状态码（0 表示成功）
    /// * `Err(MicrosandboxError)` - 启动失败时的错误
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
    /// * MicroVm 会在返回后自动清理
    /// * 非零状态表示访客进程失败
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
    /// 调用 libkrun 的 krun_create_ctx 函数创建 VM 上下文
    ///
    /// ## 返回值
    /// 上下文 ID（非负整数）
    ///
    /// ## Panics
    /// 如果创建上下文失败会 panic
    fn create_ctx() -> u32 {
        let ctx_id = unsafe { ffi::krun_create_ctx() };
        assert!(ctx_id >= 0, "failed to create microvm context: {}", ctx_id);
        ctx_id as u32
    }

    /// 将配置应用到 MicroVm 上下文
    ///
    /// 此方法配置 MicroVm 的所有方面：
    /// - 基本 VM 设置（vCPU、内存）
    /// - 根文件系统
    /// - 通过 virtio-fs 的目录映射
    /// - 端口映射
    /// - 资源限制
    /// - 工作目录
    /// - 可执行文件和参数
    /// - 环境变量
    /// - 控制台输出
    /// - 网络设置
    ///
    /// ## 参数
    /// * `ctx_id` - 要配置的 MicroVm 上下文 ID
    /// * `config` - 要应用的配置
    ///
    /// ## Panics
    /// * 任何 libkrun API 调用失败
    /// * 无法更新根文件系统 fstab 文件
    fn apply_config(ctx_id: u32, config: &MicroVmConfig) {
        // 设置日志级别
        unsafe {
            let status = ffi::krun_set_log_level(config.log_level as u32);
            assert!(status >= 0, "failed to set log level: {}", status);
        }

        // 设置基本 VM 配置（vCPU 数量和内存大小）
        unsafe {
            let status = ffi::krun_set_vm_config(ctx_id, config.num_vcpus, config.memory_mib);
            assert!(status >= 0, "failed to set VM config: {}", status);
        }

        // 设置根文件系统
        match &config.rootfs {
            Rootfs::Native(path) => {
                // 对于原生根文件系统，直接使用单个路径
                let c_path = CString::new(path.to_str().unwrap().as_bytes()).unwrap();
                unsafe {
                    let status = ffi::krun_set_root(ctx_id, c_path.as_ptr());
                    assert!(status >= 0, "failed to set rootfs: {}", status);
                }
            }
            Rootfs::Overlayfs(paths) => {
                // 对于 overlayfs，使用多个路径（层）
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
        let mapped_dirs = &config.mapped_dirs;
        for (idx, dir) in mapped_dirs.iter().enumerate() {
            // 为每个目录生成唯一的 virtio-fs 标签
            let tag = CString::new(format!("{}_{}", VIRTIOFS_TAG_PREFIX, idx)).unwrap();
            tracing::debug!("adding virtiofs mount for {}", tag.to_string_lossy());

            // 规范化主机路径
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
                let status = ffi::krun_add_virtiofs(ctx_id, tag.as_ptr(), host_path.as_ptr());
                assert!(status >= 0, "failed to add mapped directory: {}", status);
            }
        }

        // 设置端口映射
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
        unsafe {
            let status =
                ffi::krun_set_tsi_scope(ctx_id, ptr::null(), ptr::null(), config.scope as u8);
            assert!(status >= 0, "failed to set network scope: {}", status);
        }

        // 设置资源限制
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
        if let Some(workdir) = &config.workdir_path {
            let c_workdir = CString::new(workdir.to_string().as_bytes()).unwrap();
            unsafe {
                let status = ffi::krun_set_workdir(ctx_id, c_workdir.as_ptr());
                assert!(status >= 0, "Failed to set working directory: {}", status);
            }
        }

        // 设置可执行文件路径、参数和环境变量
        let c_exec_path = CString::new(config.exec_path.to_string().as_bytes()).unwrap();

        let c_argv: Vec<_> = config
            .args
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap())
            .collect();
        let c_argv_ptrs = utils::to_null_terminated_c_array(&c_argv);

        let c_env: Vec<_> = config
            .env
            .iter()
            .map(|s| CString::new(s.to_string()).unwrap())
            .collect();
        let c_env_ptrs = utils::to_null_terminated_c_array(&c_env);

        unsafe {
            // 调用 krun_set_exec 设置执行配置
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
    /// 这是创建 MicroVmConfig 实例的推荐方式。构建器模式
    /// 提供了更友好的接口，确保所有必需字段都被设置。
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
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> MicroVmConfigBuilder<(), ()> {
        MicroVmConfigBuilder::default()
    }

    /// 验证访客路径不是彼此的子集
    ///
    /// 例如，以下路径会产生冲突：
    /// - /app 和 /app/data（子集关系）
    /// - /var/log 和 /var（子集关系）
    /// - /data 和 /data（重复）
    ///
    /// ## 参数
    /// * `mapped_dirs` - 要验证的映射目录列表
    ///
    /// ## 返回值
    /// * `Ok(())` - 如果没有路径冲突
    /// * `Err` - 包含冲突路径的详细信息
    fn validate_guest_paths(mapped_dirs: &[PathPair]) -> MicrosandboxResult<()> {
        // 如果有 0 或 1 个路径，不可能有冲突
        if mapped_dirs.len() <= 1 {
            return Ok(());
        }

        // 预先规范化所有路径，避免重复规范化
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
        for i in 0..normalized_paths.len() {
            let path1 = &normalized_paths[i];

            // 只需要检查后面的路径，前面的已经比较过了
            for path2 in &normalized_paths[i + 1..] {
                // 检查两个方向的前缀关系
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
    /// 执行一系列检查以确保配置有效：
    /// - 验证根路径存在并可访问
    /// - 验证 mapped_dirs 中的所有主机路径存在并可访问
    /// - 确保 vCPU 数量非零
    /// - 确保内存分配非零
    /// - 验证可执行文件路径和参数只包含可打印 ASCII 字符
    /// - 验证访客路径不重叠或冲突
    ///
    /// ## 返回值
    /// * `Ok(())` - 如果配置有效
    /// * `Err(MicrosandboxError::InvalidMicroVMConfig)` - 包含失败详情
    ///
    /// ## 使用示例
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
        match &self.rootfs {
            Rootfs::Native(path) => {
                if !path.exists() {
                    return Err(MicrosandboxError::InvalidMicroVMConfig(
                        InvalidMicroVMConfigError::RootPathDoesNotExist(
                            path.to_str().unwrap().into(),
                        ),
                    ));
                }
            }
            Rootfs::Overlayfs(paths) => {
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
        if self.num_vcpus == 0 {
            return Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::NumVCPUsIsZero,
            ));
        }

        // 验证内存非零
        if self.memory_mib == 0 {
            return Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::MemoryIsZero,
            ));
        }

        // 验证命令行字符串
        Self::validate_command_line(self.exec_path.as_ref())?;

        for arg in &self.args {
            Self::validate_command_line(arg)?;
        }

        // 验证访客路径不重叠或冲突
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
    pub fn validate_command_line(s: &str) -> MicrosandboxResult<()> {
        // 检查字符是否在有效范围内（空格到波浪线）
        fn valid_char(c: char) -> bool {
            matches!(c, ' '..='~')
        }

        if s.chars().all(valid_char) {
            Ok(())
        } else {
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
    /// 当 MicroVm 实例被销毁时，调用 libkrun 的 krun_free_ctx 函数
    /// 释放 VM 上下文资源
    fn drop(&mut self) {
        unsafe { ffi::krun_free_ctx(self.ctx_id) };
    }
}

impl TryFrom<u8> for LogLevel {
    type Error = MicrosandboxError;

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

    #[test]
    fn test_validate_command_line_in_config() {
        let temp_dir = TempDir::new().unwrap();

        // 测试无效的可执行文件路径
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

        // 测试无效参数
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

    #[test]
    fn test_validate_guest_paths() -> anyhow::Result<()> {
        // 测试有效路径（无冲突）
        let valid_paths = vec![
            "/app".parse::<PathPair>()?,
            "/data".parse()?,
            "/var/log".parse()?,
            "/etc/config".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&valid_paths).is_ok());

        // 测试冲突路径（直接匹配）
        let conflicting_paths = vec![
            "/app".parse()?,
            "/data".parse()?,
            "/app".parse()?, // 重复
        ];
        assert!(MicroVmConfig::validate_guest_paths(&conflicting_paths).is_err());

        // 测试冲突路径（子集）
        let subset_paths = vec![
            "/app".parse()?,
            "/app/data".parse()?, // /app 的子集
            "/var/log".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&subset_paths).is_err());

        // 测试冲突路径（父级）
        let parent_paths = vec![
            "/var/log".parse()?,
            "/var".parse()?, // /var/log 的父级
            "/etc".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&parent_paths).is_err());

        // 测试需要规范化的路径
        let unnormalized_paths = vec![
            "/app/./data".parse()?,
            "/var/log".parse()?,
            "/etc//config".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&unnormalized_paths).is_ok());

        // 测试规范化后的冲突
        let normalized_conflicts = vec![
            "/app/./data".parse()?,
            "/app/data/".parse()?, // 规范化后与第一个路径相同
            "/var/log".parse()?,
        ];
        assert!(MicroVmConfig::validate_guest_paths(&normalized_conflicts).is_err());

        Ok(())
    }

    #[test]
    fn test_microvm_config_validation_with_guest_paths() -> anyhow::Result<()> {
        use tempfile::TempDir;

        let temp_dir = TempDir::new()?;
        let host_dir1 = temp_dir.path().join("dir1");
        let host_dir2 = temp_dir.path().join("dir2");
        std::fs::create_dir_all(&host_dir1)?;
        std::fs::create_dir_all(&host_dir2)?;

        // 测试有效配置
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
        let invalid_config = MicroVmConfig::builder()
            .rootfs(Rootfs::Native(temp_dir.path().to_path_buf()))
            .memory_mib(1024)
            .exec_path("/bin/echo")
            .mapped_dirs([
                format!("{}:/app/data", host_dir1.display()).parse()?,
                format!("{}:/app", host_dir2.display()).parse()?,
            ])
            .build();

        assert!(matches!(
            invalid_config.validate(),
            Err(MicrosandboxError::InvalidMicroVMConfig(
                InvalidMicroVMConfigError::ConflictingGuestPaths(_, _)
            ))
        ));

        Ok(())
    }
}
