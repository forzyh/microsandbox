//! # msbrun 命令参数定义
//!
//! 本文件定义了 `msbrun` 命令的命令行参数。
//! `msbrun` 是一个多态二进制文件，可以运行在三种模式下：
//! 1. **MicroVM 模式**: 作为轻量级虚拟机运行
//! 2. **Supervisor 模式**: 作为监督进程管理子进程
//! 3. **Sandbox Server 模式**: 作为沙箱编排服务器
//!
//! ## 什么是 MicroVM？
//!
//! MicroVM 是一种极轻量的虚拟机，特点是：
//! - 启动速度快（毫秒级）
//! - 资源占用小（几 MB 内存）
//! - 安全隔离（基于 KVM 硬件虚拟化）
//! - 常用于 serverless 计算和容器替代方案

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};

use crate::styles;

//--------------------------------------------------------------------------------------------------
// Types - 类型定义
//--------------------------------------------------------------------------------------------------

/// ## McrunArgs 结构体
///
/// `msbrun` 命令的顶层参数结构体。
/// 包含一个子命令字段，用于选择运行模式。
#[derive(Debug, Parser)]
#[command(name = "msbrun", author, styles=styles::styles())]
pub struct McrunArgs {
    /// ### 子命令
    ///
    /// 指定运行模式：`microvm` 或 `supervisor`
    /// `#[command(subcommand)]` 告诉 clap 这是一个子命令字段
    #[command(subcommand)]
    pub subcommand: McrunSubcommand,
}

/// ## McrunSubcommand 枚举
///
/// 定义 msbrun 的两种运行模式。
/// 使用 `#[derive(Subcommand)]` 宏，clap 会为每个变体自动生成解析逻辑。
///
/// ### 子命令使用示例
/// ```bash
/// # MicroVM 模式
/// msbrun microvm --exec-path=/usr/bin/python3 --memory-mib=512 ...
///
/// # Supervisor 模式
/// msbrun supervisor --log-dir=/var/log --sandbox-name=myapp --exec-path=/app/main ...
/// ```
#[derive(Subcommand, Debug)]
pub enum McrunSubcommand {
    /// ### MicroVM 模式
    ///
    /// 作为轻量级虚拟机运行，提供隔离的执行环境。
    ///
    /// ## 工作原理
    /// 1. 使用 KVM（Kernel-based Virtual Machine）创建虚拟机
    /// 2. 配置虚拟 CPU、内存、网络设备
    /// 3. 挂载根文件系统（原生或 OverlayFS）
    /// 4. 在 VM 内执行指定的命令
    #[command(name = "microvm")]
    Microvm {
        /// ### 日志级别
        ///
        /// 控制日志输出的详细程度。
        /// 通常为 0-4 的整数：
        /// - 0: Error
        /// - 1: Warn
        /// - 2: Info
        /// - 3: Debug
        /// - 4: Trace
        #[arg(long)]
        log_level: Option<u8>,

        /// ### 原生根文件系统路径
        ///
        /// 直接使用的根文件系统目录路径。
        ///
        /// ### Rootfs 类型说明
        /// Microsandbox 支持两种根文件系统类型：
        /// 1. **Native**: 直接使用一个目录作为根文件系统
        /// 2. **OverlayFS**: 使用多个层叠的目录（类似 Docker 镜像层）
        ///
        /// ### 互斥关系
        /// 此参数与 `overlayfs_layer` 互斥，只能指定其一。
        #[arg(long)]
        native_rootfs: Option<PathBuf>,

        /// ### OverlayFS 层
        ///
        /// OverlayFS 是一种联合文件系统，可以将多个目录叠加成一个统一的视图。
        ///
        /// ### 工作原理
        /// ```text
        /// Layer 3 (top, 可写)  ← 最新层，所有写入到这里
        /// Layer 2 (只读)       ← 中间层
        /// Layer 1 (bottom, 只读) ← 基础层
        /// ```
        ///
        /// ### 类似技术
        /// Docker 镜像层也使用类似的原理。
        ///
        /// ### Vec<PathBuf> 类型
        /// - 可接受多个 `--overlayfs-layer` 参数
        /// - 例如：`--overlayfs-layer=/base --overlayfs-layer=/overlay`
        #[arg(long)]
        overlayfs_layer: Vec<PathBuf>,

        /// ### 虚拟 CPU 数量
        ///
        /// 分配给虚拟机的 CPU 核心数。
        #[arg(long)]
        num_vcpus: Option<u8>,

        /// ### 内存大小（MiB）
        ///
        /// 分配给虚拟机的内存大小，单位是 MiB（Mebibyte）。
        ///
        /// ### MiB vs MB
        /// - 1 MiB = 1024 KiB = 1,048,576 字节
        /// - 1 MB = 1000 KB = 1,000,000 字节
        /// - Rust/计算机领域通常使用 MiB
        #[arg(long)]
        memory_mib: Option<u32>,

        /// ### 工作目录路径
        ///
        /// 虚拟机内进程的初始工作目录。
        #[arg(long)]
        workdir_path: Option<String>,

        /// ### 可执行文件路径
        ///
        /// 在虚拟机内执行的程序路径。
        ///
        /// ### `required = true` 说明
        /// - 此参数是必需的
        /// - 如不指定，clap 会显示错误并退出
        #[arg(long, required = true)]
        exec_path: String,

        /// ### 环境变量
        ///
        /// 设置虚拟机内进程的环境变量。
        ///
        /// ### 格式
        /// `KEY=VALUE` 格式，例如：
        /// - `--env=PATH=/usr/bin`
        /// - `--env=RUST_LOG=debug`
        ///
        /// ### Vec<String> 说明
        /// - 可指定多个环境变量
        /// - 每个 `--env` 添加一个变量
        #[arg(long)]
        env: Vec<String>,

        /// ### 目录映射
        ///
        /// 将主机目录挂载到虚拟机内。
        ///
        /// ### 格式
        /// `host_path:guest_path` 格式，例如：
        /// - `--mapped-dir=/home/user/data:/data`
        ///
        /// 这会将主机的 `/home/user/data` 挂载到虚拟机的 `/data`
        #[arg(long)]
        mapped_dir: Vec<String>,

        /// ### 端口映射
        ///
        /// 将虚拟机端口转发到主机。
        ///
        /// ### 格式
        /// `host_port:guest_port` 格式，例如：
        /// - `--port-map=8080:80`
        ///
        /// 这会将主机的 8080 端口转发到虚拟机的 80 端口
        #[arg(long)]
        port_map: Vec<String>,

        /// ### 网络作用域
        ///
        /// 控制虚拟机的网络访问范围。
        ///
        /// ### 可选值
        /// - `local`: 仅本地网络
        /// - `public`: 可访问公网
        /// - `any`: 无限制
        /// - `none`: 无网络访问
        #[arg(long)]
        scope: Option<String>,

        /// ### 分配的 IP 地址
        ///
        /// 为虚拟机分配的静态 IP 地址。
        #[arg(long)]
        ip: Option<String>,

        /// ### 分配的子网
        ///
        /// 虚拟机所在的子网 CIDR。
        /// 例如：`192.168.1.0/24`
        #[arg(long)]
        subnet: Option<String>,

        /// ### 额外参数
        ///
        /// 传递给可执行文件的额外命令行参数。
        ///
        /// ### `last = true` 说明
        /// - 收集 `--` 之后的所有参数
        /// - 例如：`msbrun microvm --exec-path=/bin/sh -- -c "echo hello"`
        /// - `-c "echo hello"` 会被收集到 `args` 中
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// ### Supervisor 模式
    ///
    /// 作为监督进程，负责管理和监控子进程（MicroVM）。
    ///
    /// ## 工作原理
    /// 1. 接收启动参数
    /// 2. 生成子进程（MicroVM 模式）的命令行
    /// 3. 启动子进程并监控其状态
    /// 4. 将子进程日志写入指定目录
    /// 5. 更新沙箱状态数据库
    ///
    /// ## 为什么需要 Supervisor？
    /// - 日志管理：将子进程输出重定向到日志文件
    /// - 状态持久化：记录沙箱运行状态到数据库
    /// - 进程监控：检测子进程异常退出
    #[command(name = "supervisor")]
    Supervisor {
        /// ### 日志目录
        ///
        /// 存放日志文件的目录。
        /// 子进程（MicroVM）的标准输出和标准错误会写入此目录。
        #[arg(long)]
        log_dir: PathBuf,

        /// ### 沙箱数据库路径
        ///
        /// 存放沙箱元数据和状态信息的 SQLite 数据库文件路径。
        #[arg(long)]
        sandbox_db_path: PathBuf,

        /// ### 沙箱名称
        ///
        /// 子进程（沙箱）的唯一标识名称。
        #[arg(long)]
        sandbox_name: String,

        /// ### 配置文件路径
        ///
        /// 沙箱配置文件的字符串路径。
        #[arg(long)]
        config_file: String,

        /// ### 配置文件最后修改时间
        ///
        /// 用于检测配置文件变更，实现配置热更新。
        ///
        /// ### DateTime<Utc> 类型
        /// - `chrono` crate 提供的日期时间类型
        /// - `Utc` 表示 UTC 时区
        #[arg(long)]
        config_last_modified: DateTime<Utc>,

        /// ### 日志级别
        ///
        /// 同 MicroVM 模式的 `log_level`。
        #[arg(long)]
        log_level: Option<u8>,

        /// ### 转发输出标志
        ///
        /// 是否将子进程的输出转发到当前进程的 stdout/stderr。
        ///
        /// ### `default_value = "true"` 说明
        /// - 默认值为 true
        /// - 可通过 `--forward-output=false` 禁用
        #[arg(long, default_value = "true")]
        forward_output: bool,

        // --- 以下是传递给子进程的沙箱参数 ---
        // 这些参数与 MicroVM 模式的参数相同

        /// 原生根文件系统路径（传递给子进程）
        #[arg(long)]
        native_rootfs: Option<PathBuf>,

        /// OverlayFS 层（传递给子进程）
        #[arg(long)]
        overlayfs_layer: Vec<PathBuf>,

        /// 虚拟 CPU 数量（传递给子进程）
        #[arg(long)]
        num_vcpus: Option<u8>,

        /// 内存大小（传递给子进程）
        #[arg(long)]
        memory_mib: Option<u32>,

        /// 工作目录路径（传递给子进程）
        #[arg(long)]
        workdir_path: Option<String>,

        /// 可执行文件路径（传递给子进程）
        #[arg(long, required = true)]
        exec_path: String,

        /// 环境变量（传递给子进程）
        #[arg(long)]
        env: Vec<String>,

        /// 目录映射（传递给子进程）
        #[arg(long)]
        mapped_dir: Vec<String>,

        /// 端口映射（传递给子进程）
        #[arg(long)]
        port_map: Vec<String>,

        /// 网络作用域（传递给子进程）
        #[arg(long)]
        scope: Option<String>,

        /// IP 地址（传递给子进程）
        #[arg(long)]
        ip: Option<String>,

        /// 子网（传递给子进程）
        #[arg(long)]
        subnet: Option<String>,

        /// 额外参数（传递给子进程）
        #[arg(last = true)]
        args: Vec<String>,
    },
}
