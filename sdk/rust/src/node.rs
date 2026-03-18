//! # Node.js 沙箱模块
//!
//! 这个模块提供了 `NodeSandbox` 结构体，专门用于执行 JavaScript/Node.js 代码。
//! `NodeSandbox` 实现了 [`BaseSandbox`] trait，提供了与其他沙箱类型一致的 API。
//!
//! ## Node.js 沙箱的特点
//!
//! - **Node.js 环境** - 提供完整的 Node.js 运行时
//! - **npm 包管理** - 可以安装和使用 npm 包
//! - **异步执行** - 支持 Promise 和 async/await
//! - **沙箱隔离** - 代码在隔离的容器中运行，无法访问宿主机资源
//!
//! ## 使用示例
//!
//! ### 基本使用
//!
//! ```rust,no_run
//! use microsandbox_sdk::{NodeSandbox, BaseSandbox};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 创建 Node.js 沙箱
//!     let mut sandbox = NodeSandbox::create("node-test").await?;
//!
//!     // 启动沙箱
//!     sandbox.start(None).await?;
//!
//!     // 执行 JavaScript 代码
//!     let result = sandbox.run("console.log('Hello, Node.js!')").await?;
//!     println!("输出：{}", result.output().await?);
//!
//!     // 执行异步代码
//!     let result = sandbox.run(r#"
//!         const sleep = (ms) => new Promise(resolve => setTimeout(resolve, Ms));
//!         await sleep(1000);
//!         console.log('1 秒后...');
//!     "#).await?;
//!
//!     // 停止沙箱
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### 使用自定义配置
//!
//! ```rust,no_run
//! use microsandbox_sdk::{NodeSandbox, SandboxOptions, StartOptions, BaseSandbox};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // 使用自定义选项创建沙箱
//!     let options = SandboxOptions::builder()
//!         .name("custom-node")
//!         .server_url("http://localhost:5555")
//!         .build();
//!
//!     let mut sandbox = NodeSandbox::create_with_options(options).await?;
//!
//!     // 配置启动选项
//!     let mut start_opts = StartOptions::default();
//!     start_opts.memory = 1024; // 1GB 内存
//!     start_opts.cpus = 2.0;    // 2 个 CPU 核心
//!
//!     sandbox.start(Some(start_opts)).await?;
//!
//!     // 执行代码...
//!
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### 使用命令接口
//!
//! ```rust,no_run
//! use microsandbox_sdk::{NodeSandbox, BaseSandbox};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut sandbox = NodeSandbox::create("node-with-command").await?;
//!     sandbox.start(None).await?;
//!
//!     // 使用 npm 安装包
//!     let command = sandbox.command().await?;
//!     let result = command.run("npm", Some(vec!["install", "lodash"]), None).await?;
//!
//!     if result.is_success() {
//!         println!("包安装成功");
//!     } else {
//!         eprintln!("安装失败：{}", result.error().await?);
//!     }
//!
//!     // 使用安装的包
//!     let result = sandbox.run(r#"
//!         const _ = require('lodash');
//!         console.log(_.join(['Hello', 'Node.js'], ' '));
//!     "#).await?;
//!
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```

use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    BaseSandbox, Execution, Metrics, SandboxBase, SandboxOptions, StartOptions, command::Command,
};

/// # Node.js 沙箱
///
/// `NodeSandbox` 是专门用于执行 JavaScript/Node.js 代码的沙箱环境。
/// 它封装了底层的 `SandboxBase`，提供了针对 Node.js 的特定实现。
///
/// ## 架构说明
///
/// ```text
/// ┌─────────────────────┐
/// │    NodeSandbox      │
/// │                     │
/// │  - create()         │  // 创建沙箱
/// │  - create_with_options() │
/// │  - command()        │  // 获取命令接口
/// │  - metrics()        │  // 获取监控接口
/// │                     │
/// │  BaseSandbox impl:  │
/// │  - run()            │  // 执行 JS 代码
/// │  - start()          │  // 启动容器
/// │  - stop()           │  // 停止容器
/// │  - is_started()     │  // 检查状态
/// │  - get_default_image() │
/// └──────────┬──────────┘
///            │
///            │ 内部持有
///            ▼
/// ┌─────────────────────┐
/// │  Arc<Mutex<         │
/// │   SandboxBase>>     │  // 基础沙箱实现
/// └─────────────────────┘
/// ```
///
/// ## 字段说明
pub struct NodeSandbox {
    /// 基础沙箱实现
    ///
    /// 使用 `Arc<Mutex<...>>` 包裹的原因：
    /// - `Arc` - 允许多个所有者共享（例如 `NodeSandbox` 和 `Command`）
    /// - `Mutex` - 确保异步环境中的线程安全
    /// - 这种模式在异步 Rust 中很常见
    base: Arc<Mutex<SandboxBase>>,
}

impl NodeSandbox {
    /// # 创建 Node.js 沙箱
    ///
    /// 使用默认配置创建一个新的 Node.js 沙箱。
    ///
    /// ## 参数
    ///
    /// * `name` - 沙箱名称，用于在服务器上标识
    ///
    /// ## 返回值
    ///
    /// * `Ok(NodeSandbox)` - 创建成功
    /// * `Err(...)` - 创建失败
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// use microsandbox_sdk::NodeSandbox;
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let sandbox = NodeSandbox::create("my-node-sandbox").await?;
    ///     // 使用沙箱...
    ///     Ok(())
    /// }
    /// ```
    ///
    /// ## 注意
    ///
    /// 这个方法只创建 `NodeSandbox` 实例，不会启动容器。
    /// 需要调用 `start()` 方法后才能执行代码。
    pub async fn create(name: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // 使用给定的名称构建选项
        let options = SandboxOptions::builder().name(name).build();
        Self::create_with_options(options).await
    }

    /// # 使用自定义选项创建 Node.js 沙箱
    ///
    /// 允许自定义服务器 URL、API 密钥等配置。
    ///
    /// ## 参数
    ///
    /// * `options` - [`SandboxOptions`] 配置选项
    ///
    /// ## 返回值
    ///
    /// * `Ok(NodeSandbox)` - 创建成功
    /// * `Err(...)` - 创建失败
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// use microsandbox_sdk::{NodeSandbox, SandboxOptions};
    ///
    /// #[tokio::main]
    /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///     let options = SandboxOptions::builder()
    ///         .name("custom-node")
    ///         .server_url("http://localhost:5555")
    ///         .api_key("my-secret-key")
    ///         .build();
    ///
    ///     let sandbox = NodeSandbox::create_with_options(options).await?;
    ///     // 使用沙箱...
    ///     Ok(())
    /// }
    /// ```
    pub async fn create_with_options(
        options: SandboxOptions,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // 创建基础沙箱实例
        let base = SandboxBase::new(&options);

        // 创建 NodeSandbox，将 base 包裹在 Arc<Mutex<...>> 中
        let sandbox = Self {
            base: Arc::new(Mutex::new(base)),
        };

        Ok(sandbox)
    }

    /// # 获取命令接口
    ///
    /// 返回一个 `Command` 实例，用于在沙箱中执行 shell 命令。
    ///
    /// ## 返回值
    ///
    /// * `Ok(Command)` - 命令接口
    /// * `Err(...)` - 获取失败
    ///
    /// ## 使用场景
    ///
    /// - 安装 npm 包：`npm install <package>`
    /// - 查看文件：`ls`、`cat`
    /// - 运行脚本：`node script.js`
    /// - 其他系统命令
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::NodeSandbox;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = NodeSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// // 获取命令接口
    /// let command = sandbox.command().await?;
    ///
    /// // 安装 npm 包
    /// let result = command.run("npm", Some(vec!["install", "express"]), None).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn command(&self) -> Result<Command, Box<dyn Error + Send + Sync>> {
        Ok(Command::new(self.base.clone()))
    }

    /// # 获取监控接口
    ///
    /// 返回一个 `Metrics` 实例，用于查询沙箱的资源使用情况。
    ///
    /// ## 返回值
    ///
    /// * `Ok(Metrics)` - 监控接口
    /// * `Err(...)` - 获取失败
    ///
    /// ## 可获取的指标
    ///
    /// - `cpu()` - CPU 使用率
    /// - `memory()` - 内存使用量
    /// - `disk()` - 磁盘使用量
    /// - `is_running()` - 运行状态
    /// - `all()` - 所有指标
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::NodeSandbox;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = NodeSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// let metrics = sandbox.metrics().await?;
    /// if let Some(cpu) = metrics.cpu().await? {
    ///     println!("CPU: {:.1}%", cpu);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn metrics(&self) -> Result<Metrics, Box<dyn Error + Send + Sync>> {
        Ok(Metrics::new(self.base.clone()))
    }
}

/// # NodeSandbox 的 BaseSandbox trait 实现
///
/// 这里实现了 [`BaseSandbox`] trait 的所有必需方法，使 `NodeSandbox`
/// 可以作为通用的沙箱类型使用。
///
/// ## Trait 方法说明
///
/// ### `get_default_image()`
///
/// 返回 Node.js 沙箱的默认 Docker 镜像：`"microsandbox/node"`。
/// 这个镜像应该包含：
/// - Node.js 运行时
/// - npm 包管理器
/// - 常用的开发工具
///
/// ### `run()`
///
/// 在沙箱中执行 JavaScript 代码。代码会被发送到服务器，
/// 在 Node.js 环境中执行，返回执行结果。
///
/// ### `start()`
///
/// 启动 Node.js 容器。可以自定义资源配置（内存、CPU 等）。
///
/// ### `stop()`
///
/// 停止并清理容器资源。
#[async_trait]
impl BaseSandbox for NodeSandbox {
    /// # 获取默认的 Docker 镜像
    ///
    /// 返回 `"microsandbox/node"`，这是 Node.js 沙箱的默认镜像。
    ///
    /// ## 返回值
    ///
    /// 返回镜像名称字符串。
    async fn get_default_image(&self) -> String {
        "microsandbox/node".to_string()
    }

    /// # 检查沙箱是否已启动
    ///
    /// 返回底层 `SandboxBase` 的 `is_started` 状态。
    ///
    /// ## 返回值
    ///
    /// * `true` - 沙箱已启动，可以执行代码
    /// * `false` - 沙箱未启动
    async fn is_started(&self) -> bool {
        // 获取互斥锁并读取状态
        // .await 会等待锁可用
        let base = self.base.lock().await;
        base.is_started
    }

    /// # 执行 JavaScript 代码
    ///
    /// 在沙箱中运行给定的 JavaScript 代码。
    ///
    /// ## 参数
    ///
    /// * `code` - JavaScript 源代码字符串
    ///
    /// ## 返回值
    ///
    /// * `Ok(Execution)` - 执行成功，返回结果
    /// * `Err(SandboxError::NotStarted)` - 沙箱未启动
    /// * `Err(...)` - 执行失败
    ///
    /// ## 支持的语言特性
    ///
    /// - ES6+ 语法（箭头函数、解构、类等）
    /// - Promise 和 async/await
    /// - Node.js 内置模块（fs、path、http 等）
    /// - 已安装的 npm 包
    ///
    /// ## 使用示例
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{NodeSandbox, BaseSandbox};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut sandbox = NodeSandbox::create("test").await?;
    /// sandbox.start(None).await?;
    ///
    /// // 简单输出
    /// let result = sandbox.run("console.log('Hello')").await?;
    ///
    /// // 异步代码
    /// let result = sandbox.run(r#"
    ///     await new Promise(r => setTimeout(r, 1000));
    ///     console.log('Done');
    /// "#).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn run(&self, code: &str) -> Result<Execution, Box<dyn Error + Send + Sync>> {
        // 检查沙箱是否已启动
        let is_started = {
            let base = self.base.lock().await;
            base.is_started
        };

        // 如果未启动，返回错误
        if !is_started {
            return Err(Box::new(crate::SandboxError::NotStarted));
        }

        // 执行代码
        // 使用 "javascript" 作为语言标识符
        let base = self.base.lock().await;
        base.run_code("javascript", code).await
    }

    /// # 启动 Node.js 沙箱
    ///
    /// 创建并启动一个包含 Node.js 运行时的 Docker 容器。
    ///
    /// ## 参数
    ///
    /// * `options` - 可选的启动配置
    ///   - `Some(opts)` - 使用自定义配置
    ///   - `None` - 使用默认配置
    ///
    /// ## 默认配置
    ///
    /// | 选项 | 默认值 |
    /// |------|--------|
    /// | 内存 | 512 MB |
    /// | CPU | 1 核心 |
    /// | 超时 | 180 秒 |
    /// | 镜像 | microsandbox/node |
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 启动成功
    /// * `Err(...)` - 启动失败
    async fn start(
        &mut self,
        options: Option<StartOptions>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // 使用提供的选项或默认值
        let opts = options.unwrap_or_default();

        // 获取默认镜像
        let default_image = self.get_default_image().await;
        // 优先使用自定义镜像，否则使用默认镜像
        let image = opts.image.clone().or_else(|| Some(default_image));

        // 启动沙箱
        let mut base = self.base.lock().await;
        base.start_sandbox(image, &opts).await
    }

    /// # 停止 Node.js 沙箱
    ///
    /// 停止容器并释放资源。
    ///
    /// ## 返回值
    ///
    /// * `Ok(())` - 停止成功
    /// * `Err(...)` - 停止失败
    ///
    /// ## 注意
    ///
    /// 这个方法具有幂等性：多次调用不会出错。
    async fn stop(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        // 检查是否已经停止
        let is_started = {
            let base = self.base.lock().await;
            base.is_started
        };

        // 如果已经停止，直接返回成功
        if !is_started {
            return Ok(());
        }

        // 停止沙箱
        let mut base = self.base.lock().await;
        base.stop_sandbox().await
    }

    /// # 获取监控接口
    ///
    /// 返回用于查询资源使用情况的 `Metrics` 实例。
    ///
    /// ## 返回值
    ///
    /// * `Ok(Metrics)` - 监控接口
    /// * `Err(...)` - 获取失败
    async fn metrics(&self) -> Result<Metrics, Box<dyn Error + Send + Sync>> {
        Ok(Metrics::new(self.base.clone()))
    }
}
