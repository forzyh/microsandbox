//! # 启动选项模块
//!
//! 这个模块定义了 `StartOptions` 结构体，用于配置沙箱容器的启动参数。
//! 通过 `StartOptions`，你可以自定义沙箱的资源限制、环境变量、端口映射等。
//!
//! ## StartOptions 字段说明
//!
//! | 字段 | 类型 | 默认值 | 说明 |
//! |------|------|--------|------|
//! | `image` | `Option<String>` | `None` | Docker 镜像名称 |
//! | `memory` | `u32` | `512` | 内存限制（MB） |
//! | `cpus` | `f32` | `1.0` | CPU 核心数 |
//! | `volumes` | `Vec<String>` | `[]` | 挂载的卷列表 |
//! | `ports` | `Vec<String>` | `[]` | 端口映射列表 |
//! | `envs` | `Vec<String>` | `[]` | 环境变量列表 |
//! | `depends_on` | `Vec<String>` | `[]` | 依赖的沙箱列表 |
//! | `workdir` | `Option<String>` | `None` | 工作目录 |
//! | `shell` | `Option<String>` | `None` | 使用的 Shell |
//! | `scripts` | `HashMap<String, String>` | `{}` | 预定义脚本 |
//! | `exec` | `Option<String>` | `None` | 启动时执行的命令 |
//! | `timeout` | `f32` | `180.0` | 启动超时（秒） |
//!
//! ## 使用示例
//!
//! ### 基本使用
//!
//! ```rust,no_run
//! use microsandbox_sdk::{PythonSandbox, BaseSandbox, StartOptions};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut sandbox = PythonSandbox::create("test").await?;
//!
//!     // 创建自定义启动选项
//!     let mut opts = StartOptions::default();
//!     opts.memory = 1024;  // 1GB 内存
//!     opts.cpus = 2.0;     // 2 个 CPU 核心
//!
//!     // 启动沙箱
//!     sandbox.start(Some(opts)).await?;
//!
//!     // 执行代码...
//!
//!     sandbox.stop().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### 配置环境变量
//!
//! ```rust,no_run
//! # use microsandbox_sdk::{PythonSandbox, BaseSandbox, StartOptions};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut sandbox = PythonSandbox::create("test").await?;
//!
//! let mut opts = StartOptions::default();
//! opts.envs = vec![
//!     "PYTHONUNBUFFERED=1".to_string(),
//!     "MY_APP_ENV=production".to_string(),
//! ];
//!
//! sandbox.start(Some(opts)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### 配置端口映射
//!
//! ```rust,no_run
//! # use microsandbox_sdk::{PythonSandbox, BaseSandbox, StartOptions};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut sandbox = PythonSandbox::create("test").await?;
//!
//! let mut opts = StartOptions::default();
//! opts.ports = vec![
//!     "8080:80".to_string(),  // 宿主机的 8080 映射到容器的 80
//! ];
//!
//! sandbox.start(Some(opts)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### 配置卷挂载
//!
//! ```rust,no_run
//! # use microsandbox_sdk::{PythonSandbox, BaseSandbox, StartOptions};
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut sandbox = PythonSandbox::create("test").await?;
//!
//! let mut opts = StartOptions::default();
//! opts.volumes = vec![
//!     "/host/path:/container/path".to_string(),
//! ];
//!
//! sandbox.start(Some(opts)).await?;
//! # Ok(())
//! # }
//! ```

/// # 沙箱启动选项
///
/// `StartOptions` 结构体包含了启动沙箱容器时可以配置的所有选项。
/// 这些选项会传递给 Docker 或底层容器运行时。
///
/// ## 资源配置
///
/// ### 内存限制（memory）
///
/// 以 MB 为单位限制沙箱可使用的最大内存。
///
/// - **默认值**: 512 MB
/// - **建议值**:
///   - 简单脚本：256-512 MB
///   - 一般应用：512-1024 MB
///   - 大型应用：2048+ MB
///
/// ```rust
/// use microsandbox_sdk::StartOptions;
///
/// let mut opts = StartOptions::default();
/// opts.memory = 1024; // 1GB
/// ```
///
/// ### CPU 限制（cpus）
///
/// 限制沙箱可使用的 CPU 核心数。
///
/// - **默认值**: 1.0 核心
/// - **建议值**:
///   - 简单任务：0.5-1.0
///   - 计算密集型：2.0-4.0
///
/// ```rust
/// # use microsandbox_sdk::StartOptions;
/// let mut opts = StartOptions::default();
/// opts.cpus = 2.0; // 2 个核心
/// ```
///
/// ## 网络配置
///
/// ### 端口映射（ports）
///
/// 将容器内的端口映射到宿主机。
///
/// 格式：`"host_port:container_port"`
///
/// ```rust
/// # use microsandbox_sdk::StartOptions;
/// let mut opts = StartOptions::default();
/// opts.ports = vec![
///     "8080:80".to_string(),   // HTTP
///     "4433:443".to_string(),  // HTTPS
/// ];
/// ```
///
/// ## 存储配置
///
/// ### 卷挂载（volumes）
///
/// 将宿主机的目录挂载到容器中。
///
/// 格式：`"host_path:container_path[:options]"`
///
/// ```rust
/// # use microsandbox_sdk::StartOptions;
/// let mut opts = StartOptions::default();
/// opts.volumes = vec![
///     "/data/app:/app/data".to_string(),
///     "/logs:/var/log:rw".to_string(),  // 可写挂载
/// ];
/// ```
///
/// ## 环境配置
///
/// ### 环境变量（envs）
///
/// 设置容器内的环境变量。
///
/// 格式：`"KEY=value"`
///
/// ```rust
/// # use microsandbox_sdk::StartOptions;
/// let mut opts = StartOptions::default();
/// opts.envs = vec![
///     "NODE_ENV=production".to_string(),
///     "DATABASE_URL=postgres://...".to_string(),
/// ];
/// ```
///
/// ## 依赖配置
///
/// ### 依赖沙箱（depends_on）
///
/// 指定当前沙箱依赖的其他沙箱。
/// 这确保依赖的沙箱先启动。
///
/// ```rust
/// # use microsandbox_sdk::StartOptions;
/// let mut opts = StartOptions::default();
/// opts.depends_on = vec![
///     "database".to_string(),
///     "cache".to_string(),
/// ];
/// ```
#[derive(Debug, Clone)]
pub struct StartOptions {
    /// Docker 镜像名称
    ///
    /// 指定要使用的 Docker 镜像。如果为 `None`，将使用沙箱类型的默认镜像。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.image = Some("python:3.9-slim".to_string());
    /// ```
    pub image: Option<String>,

    /// 内存限制（MB）
    ///
    /// 限制沙箱容器可使用的最大内存（以 MB 为单位）。
    ///
    /// ## 默认值
    ///
    /// 512 MB
    ///
    /// ## 注意事项
    ///
    /// - 设置过低可能导致 OOM（内存溢出）错误
    /// - 设置过高可能影响其他沙箱
    /// - Python/Node.js 等运行时本身就有内存开销
    pub memory: u32,

    /// CPU 限制
    ///
    /// 限制沙箱容器可使用的 CPU 核心数。
    ///
    /// - `0.5` - 半个核心（50% CPU 时间）
    /// - `1.0` - 一个完整核心
    /// - `2.0` - 两个核心
    ///
    /// ## 默认值
    ///
    /// 1.0 核心
    pub cpus: f32,

    /// 卷挂载列表
    ///
    /// 定义要从宿主机挂载到容器的目录。
    ///
    /// ## 格式
    ///
    /// - `"host_path:container_path"` - 基本挂载
    /// - `"host_path:container_path:ro"` - 只读挂载
    /// - `"host_path:container_path:rw"` - 读写挂载
    ///
    /// ## 安全提示
    ///
    /// - 只挂载必要的目录
    /// - 避免挂载敏感目录（如 `/etc`、`/root`）
    /// - 使用只读挂载保护重要数据
    pub volumes: Vec<String>,

    /// 端口映射列表
    ///
    /// 定义容器端口到宿主机端口的映射。
    ///
    /// ## 格式
    ///
    /// - `"host_port:container_port"` - 基本映射
    /// - `"127.0.0.1:8080:80"` - 绑定到特定 IP
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.ports = vec![
    ///     "8080:80".to_string(),      // Web 服务
    ///     "5432:5432".to_string(),    // 数据库
    /// ];
    /// ```
    pub ports: Vec<String>,

    /// 环境变量列表
    ///
    /// 设置在容器内可用的环境变量。
    ///
    /// ## 格式
    ///
    /// `"KEY=value"`
    ///
    /// ## 常见用途
    ///
    /// - 配置应用行为（`DEBUG=true`）
    /// - 提供凭据（`API_KEY=xxx`）
    /// - 指定环境（`NODE_ENV=production`）
    pub envs: Vec<String>,

    /// 依赖的沙箱列表
    ///
    /// 指定当前沙箱启动前必须启动的其他沙箱。
    /// 用于构建微服务架构或需要依赖服务的场景。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.depends_on = vec![
    ///     "postgres-db".to_string(),  // 先启动数据库
    ///     "redis-cache".to_string(),  // 先启动缓存
    /// ];
    /// ```
    pub depends_on: Vec<String>,

    /// 工作目录
    ///
    /// 设置容器内的工作目录（当前目录）。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.workdir = Some("/app".to_string());
    /// ```
    pub workdir: Option<String>,

    /// Shell 类型
    ///
    /// 指定容器内使用的默认 Shell。
    ///
    /// ## 常见值
    ///
    /// - `"/bin/bash"` - Bash（大多数 Linux 发行版）
    /// - `"/bin/sh"` - 标准 Shell
    /// - `"/bin/zsh"` - Zsh
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.shell = Some("/bin/bash".to_string());
    /// ```
    pub shell: Option<String>,

    /// 预定义脚本
    ///
    /// 存储在容器内可执行的脚本。键是脚本名称，值是脚本内容。
    ///
    /// ## 用途
    ///
    /// - 初始化脚本
    /// - 工具函数
    /// - 快捷命令
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use std::collections::HashMap;
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.scripts = HashMap::from([
    ///     ("setup".to_string(), "pip install -r requirements.txt".to_string()),
    ///     ("test".to_string(), "pytest tests/".to_string()),
    /// ]);
    /// ```
    pub scripts: HashMap<String, String>,

    /// 启动时执行的命令
    ///
    /// 容器启动后立即执行的命令。
    ///
    /// ## 用途
    ///
    /// - 安装依赖
    /// - 初始化配置
    /// - 启动服务
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::StartOptions;
    ///
    /// let mut opts = StartOptions::default();
    /// opts.exec = Some("pip install flask && python app.py".to_string());
    /// ```
    pub exec: Option<String>,

    /// 启动超时时间（秒）
    ///
    /// 等待沙箱启动完成的最大时间。
    ///
    /// ## 默认值
    ///
    /// 180.0 秒（3 分钟）
    ///
    /// ## 调整建议
    ///
    /// - 大型镜像需要更长时间拉取：增加 timeout
    /// - 需要运行复杂初始化脚本：增加 timeout
    /// - 快速测试场景：可以减少 timeout
    ///
    /// ## 注意
    ///
    /// 超时时间包括：
    /// - 镜像拉取时间（如果本地没有）
    /// - 容器创建时间
    /// - 初始化脚本执行时间
    pub timeout: f32,
}

/// # Default trait 实现
///
/// 为 `StartOptions` 提供默认值。
///
/// ## 默认配置
///
/// | 字段 | 默认值 | 说明 |
/// |------|--------|------|
/// | `image` | `None` | 使用沙箱类型的默认镜像 |
/// | `memory` | `512` | 512 MB 内存 |
/// | `cpus` | `1.0` | 1 个 CPU 核心 |
/// | `volumes` | `[]` | 不挂载卷 |
/// | `ports` | `[]` | 不暴露端口 |
/// | `envs` | `[]` | 无额外环境变量 |
/// | `depends_on` | `[]` | 无依赖 |
/// | `workdir` | `None` | 使用镜像默认工作目录 |
/// | `shell` | `None` | 使用镜像默认 Shell |
/// | `scripts` | `{}` | 无预定义脚本 |
/// | `exec` | `None` | 不执行启动命令 |
/// | `timeout` | `180.0` | 3 分钟超时 |
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_sdk::StartOptions;
///
/// // 使用默认值
/// let opts = StartOptions::default();
///
/// // 修改部分选项
/// let mut opts = StartOptions::default();
/// opts.memory = 1024;
/// opts.cpus = 2.0;
/// ```
impl Default for StartOptions {
    fn default() -> Self {
        Self {
            image: None,
            memory: 512,
            cpus: 1.0,
            volumes: Vec::new(),
            ports: Vec::new(),
            envs: Vec::new(),
            depends_on: Vec::new(),
            workdir: None,
            shell: None,
            scripts: HashMap::new(),
            exec: None,
            timeout: 180.0,
        }
    }
}
