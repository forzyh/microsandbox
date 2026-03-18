//! # Microsandbox Rust SDK
//!
//! Microsandbox 项目的 Rust 语言软件开发工具包（SDK），用于提供安全的沙箱环境
//! 来执行不受信任的代码。本 SDK 允许您创建隔离环境，以受控的方式访问系统资源。
//!
//! ## 什么是沙箱（Sandbox）？
//!
//! 沙箱是一种安全机制，它为一个程序提供完全隔离的运行环境。在沙箱中运行的代码：
//! - 无法访问宿主机的文件系统（除非明确授权）
//! - 无法访问网络资源（除非明确授权）
//! - 无法影响宿主机上运行的其他进程
//! - 资源使用受到限制（CPU、内存等）
//!
//! ## 本 SDK 的主要功能
//!
//! - **创建沙箱环境**：支持多种编程语言（Python、Node.js 等）
//! - **执行代码**：在隔离环境中安全地执行代码
//! - **命令执行**：在沙箱内执行 shell 命令
//! - **资源监控**：获取沙箱的资源使用情况（CPU、内存、磁盘）
//! - **生命周期管理**：启动、停止和管理沙箱
//!
//! ## 基本使用示例
//!
//! ```rust,no_run
//! use microsandbox_sdk::{PythonSandbox, BaseSandbox, SandboxOptions};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 创建一个 Python 沙箱
//!     let mut sandbox = PythonSandbox::create("my-sandbox").await?;
//!
//!     // 启动沙箱
//!     sandbox.start(None).await?;
//!
//!     // 执行 Python 代码
//!     let result = sandbox.run("print('Hello, World!')").await?;
//!     println!("输出：{}", result.output().await?);
//!
//!     // 停止沙箱
//!     sandbox.stop().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## 模块结构
//!
//! - [`base`]: 基础沙箱实现，包含通用的沙箱功能
//! - [`builder`]: 构建器模式，用于方便地创建配置选项
//! - [`command`]: 命令执行接口，用于在沙箱中执行 shell 命令
//! - [`error`]: 错误类型定义
//! - [`execution`]: 代码执行结果的处理
//! - [`metrics`]: 资源监控接口
//! - [`node`]: Node.js 专用沙箱实现
//! - [`python`]: Python 专用沙箱实现
//! - [`start_options`]: 沙箱启动配置选项

use async_trait::async_trait;

// 重新导出常用类型，方便用户直接使用
pub use base::SandboxBase;
pub use builder::SandboxOptions;
pub use command::Command;
pub use error::SandboxError;
pub use execution::Execution;
pub use metrics::Metrics;
pub use node::NodeSandbox;
pub use python::PythonSandbox;
pub use start_options::StartOptions;

mod base;
mod builder;
mod command;
mod error;
mod execution;
mod metrics;
mod node;
mod python;
mod start_options;

/// # 沙箱基础 Trait
///
/// 这是所有沙箱类型必须实现的基础接口。定义了一个沙箱应该具备的基本功能。
///
/// ## Trait 对象安全
///
/// 这个 trait 使用了 `#[async_trait]` 宏，这是为了支持异步方法。
/// 在 Rust 中，普通的 trait 不能直接包含 `async fn`，因为 async 函数
/// 实际上返回的是 `Future` 类型。`#[async_trait]` 宏会处理这些细节。
///
/// ## 实现说明
///
/// 当你需要创建一个新的沙箱类型时（比如 RubySandbox、GoSandbox 等），
/// 你需要实现这个 trait 的所有方法。
#[async_trait]
pub trait BaseSandbox: Send + Sync {
    /// # 获取默认的 Docker 镜像
    ///
    /// 返回此沙箱类型对应的默认 Docker 镜像名称。
    ///
    /// ## 什么是 Docker 镜像？
    ///
    /// Docker 镜像是一个轻量级的、独立的软件包，包含运行某个软件所需的所有内容：
    /// 代码、运行时、库、环境变量和配置文件。对于沙箱来说，Docker 镜像定义了
    /// 沙箱内部的环境，比如 Python 沙箱会使用包含 Python 解释器的镜像。
    ///
    /// ## 返回值
    ///
    /// 返回一个字符串，表示 Docker 镜像的名称，例如：
    /// - `"microsandbox/python"` - Python 沙箱
    /// - `"microsandbox/node"` - Node.js 沙箱
    async fn get_default_image(&self) -> String;

    /// # 在沙箱中执行代码
    ///
    /// 这是沙箱的核心功能：在隔离环境中执行给定语言的代码。
    ///
    /// ## 参数
    ///
    /// * `code` - 要执行的源代码字符串
    ///
    /// ## 返回值
    ///
    /// * `Ok(Execution)` - 执行成功，返回执行结果
    /// * `Err(...)` - 执行失败，返回错误信息
    ///
    /// ## 常见错误
    ///
    /// - [`SandboxError::NotStarted`] - 沙箱尚未启动
    /// - 语法错误 - 代码本身有语法问题
    /// - 运行时错误 - 代码执行过程中出错
    async fn run(&self, code: &str) -> Result<Execution, Box<dyn std::error::Error + Send + Sync>>;

    /// # 执行代码（自动启动沙箱）
    ///
    /// 这是一个便捷方法，它会检查沙箱是否已启动，如果没有则自动启动，
    /// 然后再执行代码。适合"即用即走"的使用场景。
    ///
    /// ## 工作流程
    ///
    /// 1. 检查沙箱是否已经启动
    /// 2. 如果未启动，调用 [`start`](Self::start) 方法启动沙箱
    /// 3. 调用 [`run`](Self::run) 方法执行代码
    ///
    /// ## 参数
    ///
    /// * `code` - 要执行的源代码字符串
    ///
    /// ## 注意
    ///
    /// 虽然这个方法很方便，但在需要多次执行代码的场景下，建议手动管理
    /// 沙箱的生命周期（先启动，执行多次，然后停止），以避免重复启动的开销。
    async fn run_or_start(
        &mut self,
        code: &str,
    ) -> Result<Execution, Box<dyn std::error::Error + Send + Sync>> {
        // 检查沙箱是否已启动
        let is_started = self.is_started().await;

        if !is_started {
            // 启动沙箱
            self.start(None).await?;
        }

        // 执行代码
        self.run(code).await
    }

    /// # 检查沙箱是否已启动
    ///
    /// 返回沙箱的当前运行状态。
    ///
    /// ## 返回值
    ///
    /// * `true` - 沙箱已启动并可以执行代码
    /// * `false` - 沙箱未启动
    ///
    /// ## 注意
    ///
    /// 默认实现返回 `false`。具体的沙箱类型需要覆盖这个方法，
    /// 返回实际的状态。
    async fn is_started(&self) -> bool {
        false // 默认实现，具体类型需要覆盖
    }

    /// # 启动沙箱容器
    ///
    /// 创建并启动一个 Docker 容器作为沙箱环境。
    ///
    /// ## 参数
    ///
    /// * `options` - 可选的启动配置选项
    ///   - `Some(options)` - 使用自定义配置
    ///   - `None` - 使用默认配置
    ///
    /// ## 启动过程
    ///
    /// 1. 向 Microsandbox 服务器发送启动请求
    /// 2. 服务器创建并配置 Docker 容器
    /// 3. 等待容器就绪
    /// 4. 更新内部状态为"已启动"
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 启动成功
    /// * `Err(...)` - 启动失败，可能的原因：
    ///   - 服务器不可达
    ///   - 资源不足
    ///   - 配置无效
    ///   - 超时
    async fn start(
        &mut self,
        options: Option<StartOptions>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// # 停止沙箱容器
    ///
    /// 停止并清理沙箱占用的资源。
    ///
    /// ## 重要性
    ///
    /// 停止沙箱是一个重要的清理步骤：
    /// - 释放 CPU 和内存资源
    /// - 清理临时文件
    /// - 避免资源泄漏
    ///
    /// ## 最佳实践
    ///
    /// 使用 `Drop` trait 或者 `try/finally` 模式确保沙箱总是被停止：
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut sandbox = PythonSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// // 使用沙箱...
    ///
    /// // 确保停止（即使在上面的代码中发生 panic）
    /// sandbox.stop().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 停止成功
    /// * `Err(...)` - 停止失败
    async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// # 获取沙箱的监控接口
    ///
    /// 返回一个 [`Metrics`] 对象，用于查询沙箱的资源使用情况。
    ///
    /// ## 可获取的指标
    ///
    /// - CPU 使用率（百分比）
    /// - 内存使用量（MiB）
    /// - 磁盘使用量（字节）
    /// - 运行状态
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = PythonSandbox::create("test").await?;
    /// let metrics = sandbox.metrics().await?;
    ///
    /// if let Some(cpu) = metrics.cpu().await? {
    ///     println!("CPU 使用率：{}%", cpu);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## 返回值
    ///
    /// * `Ok(Metrics)` - 返回监控接口对象
    /// * `Err(...)` - 获取失败
    async fn metrics(&self) -> Result<Metrics, Box<dyn std::error::Error + Send + Sync>>;
}
