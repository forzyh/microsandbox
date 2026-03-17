//! # msbrun 可执行文件入口
//!
//! `msbrun` 是一个多态（polymorphic）二进制文件，可以工作在三种模式下：
//!
//! 1. **MicroVM 模式**: 作为轻量级虚拟机运行，提供隔离的执行环境
//! 2. **Supervisor 模式**: 作为监督进程，管理和监控子进程（MicroVM）
//! 3. **Sandbox Server 模式**: （未在此文件中实现）
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      msbrun 二进制                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────┐         ┌─────────────────────────┐    │
//! │  │  MicroVM 模式    │         │   Supervisor 模式        │    │
//! │  │                 │         │                         │    │
//! │  │ - 创建虚拟机     │         │ - 生成子进程命令行       │    │
//! │  │ - 配置资源      │         │ - 启动 MicroVM          │    │
//! │  │ - 执行命令      │◄────────│ - 监控进程状态          │    │
//! │  │                 │         │ - 管理日志              │    │
//! │  └─────────────────┘         └─────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 为什么需要 Supervisor 模式？
//!
//! Supervisor 模式的设计目的是：
//! 1. **日志隔离**: 将 MicroVM 的输出重定向到日志文件，便于调试和审计
//! 2. **状态持久化**: 将沙箱状态写入数据库，支持进程重启后恢复
//! 3. **进程监控**: 检测子进程异常退出，进行清理和报告
//!
//! ## 使用示例
//!
//! ### MicroVM 模式
//! ```bash
//! msbrun microvm \
//!     --exec-path=/usr/bin/python3 \
//!     --memory-mib=512 \
//!     --num-vcpus=1 \
//!     --native-rootfs=/path/to/rootfs \
//!     -- -c "print('hello')"
//! ```
//!
//! ### Supervisor 模式
//! ```bash
//! msbrun supervisor \
//!     --log-dir=/var/log/sandboxes \
//!     --sandbox-db-path=/var/lib/msb.db \
//!     --sandbox-name=myapp \
//!     --exec-path=/usr/bin/python3 \
//!     --forward-output
//! ```

use std::env;

use anyhow::Result;
use clap::Parser;
use microsandbox_cli::{McrunArgs, McrunSubcommand};
use microsandbox_core::{
    config::{EnvPair, PathPair, PortPair},
    runtime::MicroVmMonitor,
    vm::{MicroVm, Rootfs},
};
use microsandbox_utils::runtime::Supervisor;

//--------------------------------------------------------------------------------------------------
// Functions: main - 主函数
//--------------------------------------------------------------------------------------------------

/// ## msbrun 主入口函数
///
/// 这是 msbrun 命令的异步主函数。
/// 使用 `tokio` 异步运行时执行 I/O 密集型操作。
///
/// ### 返回类型
/// `anyhow::Result<()>` 是一个通用的错误处理类型：
/// - `Ok(())`: 成功执行
/// - `Err(e)`: 失败，返回错误上下文
///
/// ### anyhow vs thiserror
/// - `anyhow`: 应用层错误处理，提供丰富的错误上下文
/// - `thiserror`: 库层错误定义，专注于错误类型
#[tokio::main]
async fn main() -> Result<()> {
    // ======================================================================
    // 步骤 1: 解析命令行参数
    // ======================================================================
    let args = McrunArgs::parse();

    // ======================================================================
    // 步骤 2: 根据子命令执行不同逻辑
    // ======================================================================
    match args.subcommand {
        // ------------------------------------------------------------------
        // MicroVM 模式：直接创建并启动虚拟机
        // ------------------------------------------------------------------
        McrunSubcommand::Microvm {
            log_level,
            native_rootfs,
            overlayfs_layer,
            num_vcpus,
            memory_mib,
            workdir_path,
            exec_path,
            env,
            mapped_dir,
            port_map,
            scope,
            ip,
            subnet,
            args,
        } => {
            // 初始化日志订阅器
            tracing_subscriber::fmt::init();

            // 调试日志：输出所有参数
            // 使用 {:#?} 格式进行美化输出（pretty print）
            tracing::debug!("log_level: {:#?}", log_level);
            tracing::debug!("native_rootfs: {:#?}", native_rootfs);
            tracing::debug!("overlayfs_layer: {:#?}", overlayfs_layer);
            tracing::debug!("num_vcpus: {:#?}", num_vcpus);
            tracing::debug!("memory_mib: {:#?}", memory_mib);
            tracing::debug!("workdir_path: {:#?}", workdir_path);
            tracing::debug!("exec_path: {:#?}", exec_path);
            tracing::debug!("env: {:#?}", env);
            tracing::debug!("mapped_dir: {:#?}", mapped_dir);
            tracing::debug!("port_map: {:#?}", port_map);
            tracing::debug!("scope: {:#?}", scope);
            tracing::debug!("ip: {:#?}", ip);
            tracing::debug!("subnet: {:#?}", subnet);
            tracing::debug!("args: {:#?}", args);

            // --------------------------------------------------------------
            // 根文件系统类型检查
            // --------------------------------------------------------------
            // native_rootfs 和 overlayfs_layer 是互斥的：
            // - Native: 直接使用一个目录作为根文件系统
            // - OverlayFS: 使用多层联合文件系统
            let rootfs = match (native_rootfs, overlayfs_layer.is_empty()) {
                (Some(path), true) => Rootfs::Native(path),           // 仅指定 native_rootfs
                (None, false) => Rootfs::Overlayfs(overlayfs_layer),  // 仅指定 overlayfs_layer
                (Some(_), false) => {
                    // 两者都指定，报错
                    anyhow::bail!("Cannot specify both native_rootfs and overlayfs_rootfs")
                }
                (None, true) => {
                    // 两者都没指定，报错
                    anyhow::bail!("Must specify either native_rootfs or overlayfs_rootfs")
                }
            };

            tracing::info!("rootfs: {:#?}", rootfs);

            // --------------------------------------------------------------
            // 解析各类配置参数
            // --------------------------------------------------------------
            // 解析目录映射：将 "host:guest" 格式字符串解析为 PathPair
            let mapped_dir: Vec<PathPair> = mapped_dir
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;

            // 解析端口映射：将 "host:guest" 格式字符串解析为 PortPair
            let port_map: Vec<PortPair> = port_map
                .iter()
                .map(|s| s.parse())
                .collect::<Result<_, _>>()?;

            // 解析环境变量：将 "KEY=VALUE" 格式字符串解析为 EnvPair
            let env: Vec<EnvPair> = env.iter().map(|s| s.parse()).collect::<Result<_, _>>()?;

            // --------------------------------------------------------------
            // 使用 Builder 模式创建 MicroVM
            // --------------------------------------------------------------
            // Builder 模式优势：
            // 1. 清晰的链式调用
            // 2. 可选参数不需要 Option 包装
            // 3. 编译时检查必填参数
            let mut builder = MicroVm::builder().rootfs(rootfs).exec_path(exec_path);

            // 设置虚拟 CPU 数量（如果提供）
            if let Some(num_vcpus) = num_vcpus {
                builder = builder.num_vcpus(num_vcpus);
            }

            // 设置内存大小（如果提供）
            if let Some(memory_mib) = memory_mib {
                builder = builder.memory_mib(memory_mib);
            }

            // 设置日志级别（如果提供）
            if let Some(log_level) = log_level {
                // try_into() 尝试类型转换，可能失败
                builder = builder.log_level(log_level.try_into()?);
            }

            // 设置工作目录（如果提供）
            if let Some(workdir_path) = workdir_path {
                builder = builder.workdir_path(workdir_path);
            }

            // 设置目录映射（如果有）
            if !mapped_dir.is_empty() {
                builder = builder.mapped_dirs(mapped_dir);
            }

            // 设置端口映射（如果有）
            if !port_map.is_empty() {
                builder = builder.port_map(port_map);
            }

            // 设置网络作用域（如果提供）
            if let Some(scope) = scope {
                builder = builder.scope(scope.parse()?);
            }

            // 设置 IP 地址（如果提供）
            if let Some(ip) = ip {
                builder = builder.ip(ip.parse()?);
            }

            // 设置子网（如果提供）
            if let Some(subnet) = subnet {
                builder = builder.subnet(subnet.parse()?);
            }

            // 设置环境变量（如果有）
            if !env.is_empty() {
                builder = builder.env(env);
            }

            // 设置额外参数（如果有）
            if !args.is_empty() {
                builder = builder.args(args.iter().map(|s| s.as_str()));
            }

            // 构建并启动虚拟机
            let vm = builder.build()?;

            tracing::info!("starting µvm");  // µvm 是 MicroVM 的缩写
            vm.start()?;
        }

        // ------------------------------------------------------------------
        // Supervisor 模式：作为监督进程管理子 MicroVM
        // ------------------------------------------------------------------
        McrunSubcommand::Supervisor {
            log_dir,
            sandbox_db_path,
            sandbox_name,
            config_file,
            config_last_modified,
            log_level,
            forward_output,
            native_rootfs,
            overlayfs_layer,
            num_vcpus,
            memory_mib,
            workdir_path,
            exec_path,
            env,
            mapped_dir,
            port_map,
            scope,
            ip,
            subnet,
            args,
        } => {
            // 初始化日志
            tracing_subscriber::fmt::init();
            tracing::info!("setting up supervisor");

            // --------------------------------------------------------------
            // 获取当前可执行文件路径（用于启动子进程）
            // --------------------------------------------------------------
            // 关键点：Supervisor 和 MicroVM 是同一个二进制文件
            // Supervisor 通过传入不同的参数来启动 MicroVM 模式
            let child_exe = env::current_exe()?;

            // 获取当前进程 ID（Supervisor 的 PID）
            let supervisor_pid = std::process::id();

            // 获取根文件系统配置（与 MicroVM 模式相同逻辑）
            let rootfs = match (&native_rootfs, &overlayfs_layer.is_empty()) {
                (Some(path), true) => Rootfs::Native(path.clone()),
                (None, false) => Rootfs::Overlayfs(overlayfs_layer.clone()),
                (Some(_), false) => {
                    anyhow::bail!("Cannot specify both native_rootfs and overlayfs_rootfs")
                }
                (None, true) => {
                    anyhow::bail!("Must specify either native_rootfs or overlayfs_rootfs")
                }
            };

            // --------------------------------------------------------------
            // 创建 MicroVM 监控器
            // --------------------------------------------------------------
            // MicroVmMonitor 负责：
            // 1. 监控子进程状态
            // 2. 将日志写入指定目录
            // 3. 更新沙箱状态数据库
            let process_monitor = MicroVmMonitor::new(
                supervisor_pid,      // 父进程 PID
                sandbox_db_path,     // 沙箱数据库路径
                sandbox_name,        // 沙箱名称
                config_file,         // 配置文件路径
                config_last_modified, // 配置文件修改时间
                log_dir.clone(),     // 日志目录
                rootfs.clone(),      // 根文件系统
                forward_output,      // 是否转发输出到父进程
            )
            .await?;

            // --------------------------------------------------------------
            // 构建子进程（MicroVM）的命令行参数
            // --------------------------------------------------------------
            // 这是 Supervisor 模式的核心：
            // 1. 接收用户的 Supervisor 参数
            // 2. 转换为 MicroVM 模式的参数
            // 3. 启动自身（msbrun microvm ...）作为子进程
            let mut child_args = vec!["microvm".to_string(), format!("--exec-path={}", exec_path)];

            // 传递资源配置
            if let Some(num_vcpus) = num_vcpus {
                child_args.push(format!("--num-vcpus={}", num_vcpus));
            }
            if let Some(memory_mib) = memory_mib {
                child_args.push(format!("--memory-mib={}", memory_mib));
            }
            if let Some(workdir_path) = workdir_path {
                child_args.push(format!("--workdir-path={}", workdir_path));
            }

            // 传递根文件系统配置
            if let Some(native_rootfs) = native_rootfs {
                child_args.push(format!("--native-rootfs={}", native_rootfs.display()));
            }
            if !overlayfs_layer.is_empty() {
                for path in overlayfs_layer {
                    child_args.push(format!("--overlayfs-layer={}", path.display()));
                }
            }

            // 传递环境变量
            if !env.is_empty() {
                for env in env {
                    child_args.push(format!("--env={}", env));
                }
            }

            // 传递目录映射
            if !mapped_dir.is_empty() {
                for dir in mapped_dir {
                    child_args.push(format!("--mapped-dir={}", dir));
                }
            }

            // 传递端口映射
            if !port_map.is_empty() {
                for port_map in port_map {
                    child_args.push(format!("--port-map={}", port_map));
                }
            }

            // 传递网络配置
            if let Some(scope) = scope {
                child_args.push(format!("--scope={}", scope));
            }
            if let Some(ip) = ip {
                child_args.push(format!("--ip={}", ip));
            }
            if let Some(subnet) = subnet {
                child_args.push(format!("--subnet={}", subnet));
            }

            // 传递日志级别
            if let Some(log_level) = log_level {
                child_args.push(format!("--log-level={}", log_level));
            }

            // 传递额外参数
            if !args.is_empty() {
                child_args.push("--".to_string());
                for arg in args {
                    child_args.push(arg);
                }
            }

            // --------------------------------------------------------------
            // 构建子进程环境变量
            // --------------------------------------------------------------
            let mut child_envs = Vec::<(String, String)>::new();

            // 仅当父进程有 RUST_LOG 时才传递
            // 这确保子进程的日志级别与父进程一致
            if let Ok(rust_log) = std::env::var("RUST_LOG") {
                tracing::debug!("using existing RUST_LOG: {:?}", rust_log);
                child_envs.push(("RUST_LOG".to_string(), rust_log));
            }

            // --------------------------------------------------------------
            // 创建并启动 Supervisor
            // --------------------------------------------------------------
            // Supervisor 会：
            // 1. 启动子进程
            // 2. 捕获子进程的输出
            // 3. 写入日志文件
            // 4. 监控子进程状态
            let mut supervisor =
                Supervisor::new(child_exe, child_args, child_envs, log_dir, process_monitor);

            supervisor.start().await?;
        }
    }

    // ================================================================
    // 强制退出
    // ================================================================
    // 注意：这里显式调用 exit() 是为了确保在 TTY 模式下正确退出
    // 否则，进程可能会等待用户按回车键才退出
    std::process::exit(0);
}
