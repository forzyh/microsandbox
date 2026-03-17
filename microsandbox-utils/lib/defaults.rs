//! # 默认配置值和常量
//!
//! 本模块定义了 microsandbox 项目中使用的各种默认配置值、路径和常量。
//! 这些常量被设计为全局可访问，使用 `LazyLock` 实现延迟初始化，
//! 确保在首次使用时才进行计算，提高启动性能。
//!
//! ## 主要内容
//!
//! ### 虚拟机配置
//! - [`DEFAULT_NUM_VCPUS`]: 默认虚拟 CPU 数量
//! - [`DEFAULT_MEMORY_MIB`]: 默认内存大小（MiB）
//!
//! ### 路径配置
//! - [`DEFAULT_MICROSANDBOX_HOME`]: 用户主目录下的 microsandbox 数据目录
//! - [`DEFAULT_MSBRUN_EXE_PATH`]: msbrun 二进制文件的默认路径
//! - [`DEFAULT_MSBSERVER_EXE_PATH`]: msbserver 二进制文件的默认路径
//!
//! ### OCI 镜像仓库配置
//! - [`DEFAULT_OCI_REGISTRY`]: 默认的 OCI 镜像仓库域名
//! - [`DEFAULT_OCI_REFERENCE_TAG`]: 默认的镜像标签（latest）
//! - [`DEFAULT_OCI_REFERENCE_REPO_NAMESPACE`]: 默认的镜像命名空间
//!
//! ### 日志配置
//! - [`DEFAULT_LOG_MAX_SIZE`]: 日志文件最大大小
//!
//! ### 网络配置
//! - [`DEFAULT_SERVER_HOST`]: 默认服务器地址
//! - [`DEFAULT_SERVER_PORT`]: 默认服务器端口
//! - [`DEFAULT_PORTAL_GUEST_PORT`]: 默认沙箱门户端口
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::defaults::{
//!     DEFAULT_NUM_VCPUS,
//!     DEFAULT_MEMORY_MIB,
//!     DEFAULT_MICROSANDBOX_HOME,
//! };
//!
//! // 使用 VM 配置默认值
//! println!("默认 vCPU 数：{}", DEFAULT_NUM_VCPUS);  // 输出：1
//! println!("默认内存：{} MiB", DEFAULT_MEMORY_MIB); // 输出：1024
//!
//! // 访问沙箱主目录路径
//! println!("沙箱主目录：{:?}", DEFAULT_MICROSANDBOX_HOME);
//! ```
//!
//! ## LazyLock 说明
//!
//! 本模块大量使用 `LazyLock`，这是一种延迟初始化的智能指针。
//! 它的特点是：
//! - 首次访问时才进行初始化（懒加载）
//! - 保证只初始化一次（线程安全）
//! - 后续访问直接返回引用（零开销）
//!
//! 例如 `DEFAULT_MICROSANDBOX_HOME` 需要获取用户主目录并拼接路径，
//! 使用 `LazyLock` 可以避免在程序启动时就执行这个操作。

use std::{fs, path::PathBuf, sync::LazyLock};

use crate::MICROSANDBOX_HOME_DIR;  // 从 path 模块导入主目录名称常量

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// ### 日志文件最大大小：10MB
///
/// 当日志文件达到此大小时，会自动触发日志轮转（log rotation），
/// 将当前日志文件重命名为 `.old` 后缀，并创建新的日志文件。
///
/// 计算公式：`10 * 1024 * 1024` 字节 = 10,485,760 字节 = 10 MiB
pub const DEFAULT_LOG_MAX_SIZE: u64 = 10 * 1024 * 1024;

/// ### 默认虚拟 CPU 数量：1 个
///
/// 用于配置 MicroVM（微型虚拟机）的 vCPU 数量。
/// 对于轻量级沙箱应用，1 个 vCPU 通常足够使用。
pub const DEFAULT_NUM_VCPUS: u8 = 1;

/// ### 默认内存大小：1024 MiB（1 GiB）
///
/// 用于配置 MicroVM 的内存大小。
/// 单位是 MiB（Mebibyte，1 MiB = 1024 KiB = 1,048,576 字节）。
/// 1024 MiB = 1 GiB，对于大多数沙箱应用来说是合理的默认值。
pub const DEFAULT_MEMORY_MIB: u32 = 1024;

/// ### Microsandbox 主目录路径
///
/// 这是 microsandbox 在用户主目录下存储全局数据的目录。
/// 默认路径为：`~/.microsandbox`
///
/// ## 目录结构
/// 该目录下通常包含以下子目录和文件：
/// - `layers/`: 存储 OCI 镜像层
/// - `installs/`: 存储已安装的沙箱
/// - `projects/`: 存储项目配置
/// - `oci.db`: OCI 镜像数据库
/// - `server.pid`: 服务器进程 ID 文件
/// - `server.key`: 服务器密钥文件
///
/// ## 注意
/// 使用 `LazyLock` 实现延迟初始化，首次访问时才计算路径。
/// 通过 `dirs::home_dir()` 获取用户主目录，然后拼接 `MICROSANDBOX_HOME_DIR`（".microsandbox"）。
pub static DEFAULT_MICROSANDBOX_HOME: LazyLock<PathBuf> =
    LazyLock::new(|| dirs::home_dir().unwrap().join(MICROSANDBOX_HOME_DIR));

/// ### 默认 OCI 镜像仓库域名：docker.io
///
/// OCI（Open Container Initiative）是一个开放的容器标准组织。
/// 当用户拉取 Docker 镜像时，如果不指定仓库域名，默认使用 `docker.io`（Docker Hub）。
///
/// ## 使用示例
/// 用户输入 `nginx:latest` 时，实际拉取的完整路径是：
/// `docker.io/library/nginx:latest`
pub const DEFAULT_OCI_REGISTRY: &str = "docker.io";

/// ### 默认 OCI 镜像标签：latest
///
/// 当用户没有指定镜像标签时，默认使用 `latest`。
/// 例如：`nginx` 会被解析为 `nginx:latest`。
///
/// ## 注意
/// 在生产环境中，建议明确指定镜像标签而不是依赖 `latest`，
/// 因为 `latest` 可能会随时间变化指向不同的镜像版本。
pub const DEFAULT_OCI_REFERENCE_TAG: &str = "latest";

/// ### 默认 OCI 镜像命名空间：library
///
/// 在 Docker Hub 中，官方镜像位于 `library` 命名空间下。
/// 当用户指定一个没有命名空间的镜像（如 `nginx`）时，
/// 会自动解析为 `library/nginx`。
///
/// ## 镜像名称格式
/// 完整的镜像引用格式为：`[registry]/[namespace]/[repository]:[tag]`
/// - 例如：`docker.io/library/nginx:latest`
pub const DEFAULT_OCI_REFERENCE_REPO_NAMESPACE: &str = "library";

/// ### 默认配置文件内容
///
/// 当创建新的沙箱配置文件（Sandboxfile）时，如果文件不存在，
/// 会使用此默认内容。配置文件采用 YAML 格式。
///
/// ## 格式说明
/// ```yaml
/// # Sandbox configurations
/// sandboxes:
/// ```
/// 用户可以在 `sandboxes:` 下面添加具体的沙箱配置。
pub const DEFAULT_CONFIG: &str = "# Sandbox configurations\nsandboxes:\n";

/// ### 默认 Shell：/bin/sh
///
/// 当启动沙箱交互会话时，默认使用的 shell 路径。
/// `/bin/sh` 是 POSIX 系统标准的 shell，几乎所有 Linux 系统都支持。
///
/// ## 其他常见 shell
/// - `/bin/bash`: GNU Bash，功能更强大
/// - `/bin/zsh`: Z Shell，macOS 默认 shell
/// - `/bin/ash`: Alpine Linux 默认 shell
pub const DEFAULT_SHELL: &str = "/bin/sh";

/// ### msbrun 二进制文件默认路径
///
/// `msbrun` 是 microsandbox 的运行时执行器，负责启动和管理 MicroVM。
///
/// ## 路径解析逻辑
/// 1. 获取当前正在执行的可执行文件路径
/// 2. 解析真实路径（解析所有符号链接）
/// 3. 取其父目录，然后拼接 `msbrun` 得到最终路径
///
/// ## 示例
/// 如果当前可执行文件是 `/opt/microsandbox/bin/microsandbox`，
/// 那么 `msbrun` 的路径就是 `/opt/microsandbox/bin/msbrun`。
///
/// ## 注意
/// 使用 `LazyLock` 延迟初始化，因为路径解析涉及文件系统操作。
pub static DEFAULT_MSBRUN_EXE_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let current_exe = std::env::current_exe().unwrap();  // 获取当前可执行文件路径
    let actual_exe = fs::canonicalize(current_exe).unwrap();  // 解析符号链接得到真实路径
    actual_exe.parent().unwrap().join("msbrun")  // 在父目录下找到 msbrun
});

/// ### msbserver 二进制文件默认路径
///
/// `msbserver` 是 microsandbox 的服务器组件，提供网络服务和 API。
///
/// ## 路径解析逻辑
/// 与 [`DEFAULT_MSBRUN_EXE_PATH`] 相同，在可执行文件所在目录下查找 `msbserver`。
pub static DEFAULT_MSBSERVER_EXE_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let current_exe = std::env::current_exe().unwrap();
    let actual_exe = fs::canonicalize(current_exe).unwrap();
    actual_exe.parent().unwrap().join("msbserver")
});

/// ### 默认工作目录：根目录 `/`
///
/// 当启动沙箱时，如果没有指定工作目录，默认使用根目录。
/// 这意味着进程启动时的当前目录是 `/`。
pub const DEFAULT_WORKDIR: &str = "/";

/// ### 默认服务器主机地址：127.0.0.1
///
/// microsandbox 服务器默认绑定到本地回环地址（localhost）。
/// 这样只有本机可以访问服务器，提高安全性。
///
/// ## 安全说明
/// 如果需要从其他机器访问服务器，可以修改此配置或绑定到 `0.0.0.0`（所有网络接口）。
pub const DEFAULT_SERVER_HOST: &str = "127.0.0.1";

/// ### 默认服务器端口：5555
///
/// microsandbox 服务器默认监听的 TCP 端口。
/// 选择 5555 是因为它不是常用端口，避免冲突。
///
/// ## 自定义端口
/// 如果 5555 端口被占用，可以通过环境变量修改。
pub const DEFAULT_SERVER_PORT: u16 = 5555;

/// ### 默认沙箱门户客户端端口：4444
///
/// 在沙箱内部（guest），门户服务监听的端口。
/// 门户用于在宿主机和沙箱之间建立网络连接，实现端口转发等功能。
pub const DEFAULT_PORTAL_GUEST_PORT: u16 = 4444;
