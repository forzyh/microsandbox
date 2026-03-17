//! # msb 主命令参数定义
//!
//! 本文件定义了 `msb`（microsandbox）主命令的所有命令行参数。
//! `msb` 是用户主要交互的命令行工具，提供沙箱管理、镜像操作、服务器控制等功能。
//!
//! ## 命令结构
//!
//! ```text
//! msb <SUBCOMMAND> [OPTIONS] [ARGUMENTS]
//!
//! 主要子命令：
//! - init: 初始化新项目
//! - add/remove: 添加/删除沙箱
//! - run/shell: 运行沙箱/打开 shell
//! - up/down: 启动/停止所有沙箱
//! - install/uninstall: 安装/卸载全局脚本
//! - server: 管理沙箱服务器
//! - clean: 清理缓存
//! ```

use std::{error::Error, path::PathBuf};

use crate::styles;
use clap::Parser;
use microsandbox_core::oci::Reference;
use typed_path::Utf8UnixPathBuf;

//-------------------------------------------------------------------------------------------------
// Types - 类型定义
//-------------------------------------------------------------------------------------------------

/// ## MicrosandboxArgs 结构体
///
/// `msb` 命令的顶层参数结构体，包含全局选项和子命令。
///
/// ### 全局选项（`global = true`）
/// 这些选项可以出现在命令的任何位置：
/// - `msb --version run myapp`
/// - `msb run --version myapp`
/// - `msb run myapp --version`
///
/// ### 派生属性说明
///
/// **`#[command(name = "msb", ...)]`**
/// - 设置命令名称为 "msb"
/// - `author`: 从 Cargo.toml 自动获取作者信息
/// - `styles=styles::styles()`: 使用自定义 ANSI 样式
#[derive(Debug, Parser)]
#[command(name = "msb", author, styles=styles::styles())]
pub struct MicrosandboxArgs {
    /// ### 子命令
    ///
    /// 可选的子命令，如 `init`, `run`, `add` 等。
    /// 使用 `Option` 类型表示子命令可选（不指定时显示帮助）。
    #[command(subcommand)]
    pub subcommand: Option<MicosandboxSubcommand>,

    /// ### 显示版本
    ///
    /// 显示 microsandbox 的版本号。
    ///
    /// ### `global = true` 说明
    /// 此参数是全局的，可以在命令的任何位置使用。
    #[arg(short = 'V', long, global = true)]
    pub version: bool,

    /// ### 日志级别选项
    ///
    /// 以下五个参数控制日志输出的详细程度。
    /// 它们是互斥的（虽然这里没有显式检查），通常只使用一个。
    ///
    /// ### 日志级别层次（从高到低）
    /// ```text
    /// trace > debug > info > warn > error
    /// ```
    ///
    /// ### 使用示例
    /// ```bash
    /// msb --debug run myapp    # 显示 debug 及以上级别
    /// msb --error run myapp    # 仅显示 error 级别
    /// ```

    /// 仅显示错误级别日志
    #[arg(long, global = true)]
    pub error: bool,

    /// 仅显示警告级别日志
    #[arg(long, global = true)]
    pub warn: bool,

    /// 仅显示信息级别日志
    #[arg(long, global = true)]
    pub info: bool,

    /// 显示调试级别日志
    #[arg(long, global = true)]
    pub debug: bool,

    /// 显示追踪级别日志（最详细）
    #[arg(long, global = true)]
    pub trace: bool,
}

/// ## MicrosandboxSubcommand 枚举
///
/// 定义 msb 命令的所有子命令。
/// 每个枚举变体代表一个子命令，包含该子命令特有的参数。
///
/// ### 命令别名（alias）
/// 许多命令有缩写形式：
/// - `rm` = `remove`
/// - `r` = `run`
/// - `x` = `exe`
/// - `i` = `install`
/// - `ps`/`stat` = `status`
#[derive(Debug, Parser)]
pub enum MicrosandboxSubcommand {
    /// ### 初始化微沙箱项目
    ///
    /// 在当前目录或指定目录创建微沙箱项目的配置文件。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb init                    # 在当前目录初始化
    /// msb init -f /path/to/proj   # 在指定目录初始化
    /// ```
    #[command(name = "init")]
    Init {
        /// ### 配置文件或项目目录路径
        ///
        /// 可以是：
        /// - 配置文件路径（如 `microsandbox.yaml`）
        /// - 项目目录路径（会在其中创建默认配置文件）
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// ### 添加新沙箱到项目
    ///
    /// 向项目配置中添加一个新的沙箱定义。
    ///
    /// ### 使用示例
    /// ```bash
    /// # 添加一个简单的沙箱
    /// msb add myapp --image python:3.9
    ///
    /// # 添加带资源配置的沙箱
    /// msb add myapp --image python:3.9 --memory 1024 --cpus 2
    ///
    /// # 添加带端口映射的沙箱
    /// msb add webapp --image nginx --port 8080:80
    /// ```
    #[command(name = "add")]
    Add {
        /// ### --sandbox 标志
        ///
        /// 指定操作应用于沙箱（而非构建沙箱）。
        /// 这是默认行为，通常不需要显式指定。
        #[arg(short, long)]
        sandbox: bool,

        /// ### --build 标志
        ///
        /// 指定操作应用于构建沙箱。
        /// 构建沙箱用于从沙箱构建镜像。
        #[arg(short, long)]
        build: bool,

        /// ### 沙箱名称
        ///
        /// 要添加的沙箱名称。
        /// 名称在项目中必须唯一。
        #[arg(required = true)]
        names: Vec<String>,

        /// ### 镜像名称
        ///
        /// 沙箱使用的 OCI 镜像名称。
        /// 格式：`registry/namespace/image:tag`
        /// 例如：`python:3.9`, `docker.io/library/nginx:latest`
        #[arg(short, long)]
        image: String,

        /// ### 内存限制（MiB）
        ///
        /// 沙箱可使用的最大内存，单位 MiB。
        #[arg(long)]
        memory: Option<u32>,

        /// ### CPU 核心数
        ///
        /// 分配给沙箱的 CPU 核心数。
        /// `alias = "cpu"` 允许使用 `--cpu` 作为别名。
        #[arg(long, alias = "cpu")]
        cpus: Option<u32>,

        /// ### 卷挂载
        ///
        /// 将主机目录挂载到沙箱内。
        ///
        /// ### 格式
        /// `<host_path>:<container_path>`
        ///
        /// ### 使用示例
        /// ```bash
        /// msb add myapp --image python -v /host/data:/data
        /// ```
        #[arg(short, long = "volume", name = "VOLUME")]
        volumes: Vec<String>,

        /// ### 端口映射
        ///
        /// 将沙箱端口转发到主机。
        ///
        /// ### 格式
        /// `<host_port>:<container_port>`
        ///
        /// ### 使用示例
        /// ```bash
        /// msb add webapp --image nginx -p 8080:80
        /// ```
        #[arg(short, long = "port", name = "PORT")]
        ports: Vec<String>,

        /// ### 环境变量
        ///
        /// 设置沙箱内的环境变量。
        ///
        /// ### 格式
        /// `<key>=<value>`
        ///
        /// ### 使用示例
        /// ```bash
        /// msb add myapp --image python --env PATH=/usr/bin --env DEBUG=true
        /// ```
        #[arg(long = "env", name = "ENV")]
        envs: Vec<String>,

        /// ### 环境变量文件
        ///
        /// 从文件读取环境变量。
        /// 文件格式通常是每行一个 `KEY=VALUE`。
        #[arg(long)]
        env_file: Option<Utf8UnixPathBuf>,

        /// ### 依赖沙箱
        ///
        /// 指定此沙箱依赖的其他沙箱名称。
        /// 启动时会自动先启动依赖的沙箱。
        #[arg(long)]
        depends_on: Vec<String>,

        /// ### 工作目录
        ///
        /// 沙箱内进程的初始工作目录。
        #[arg(long)]
        workdir: Option<Utf8UnixPathBuf>,

        /// ### Shell 类型
        ///
        /// 指定沙箱内使用的 shell 程序。
        /// 例如：`bash`, `sh`, `zsh`
        #[arg(long)]
        shell: Option<String>,

        /// ### 自定义脚本
        ///
        /// 添加自定义脚本到沙箱。
        ///
        /// ### 格式
        /// `script_name=script_content`
        ///
        /// ### `value_parser = parse_key_val` 说明
        /// 使用自定义解析器将 `NAME=VALUE` 格式解析为元组。
        #[arg(long = "script", name = "SCRIPT", value_parser = parse_key_val::<String, String>)]
        scripts: Vec<(String, String)>,

        /// ### 启动脚本
        ///
        /// 沙箱启动时自动执行的脚本。
        /// 这是 `--script` 的特殊形式，脚本名为 "start"。
        #[arg(long)]
        start: Option<String>,

        /// ### 导入文件
        ///
        /// 将文件从主机导入到沙箱。
        ///
        /// ### 格式
        /// `<name>=<path>`
        #[arg(long = "import", name = "IMPORT", value_parser = parse_key_val::<String, String>)]
        imports: Vec<(String, String)>,

        /// ### 导出文件
        ///
        /// 将文件从沙箱导出到主机。
        ///
        /// ### 格式
        /// `<name>=<path>`
        #[arg(long = "export", name = "EXPORT", value_parser = parse_key_val::<String, String>)]
        exports: Vec<(String, String)>,

        /// ### 网络作用域
        ///
        /// 控制沙箱的网络访问范围。
        ///
        /// ### 可选值
        /// - `local`: 仅本地网络
        /// - `public`: 可访问公网
        /// - `any`: 无限制
        /// - `none`: 无网络
        #[arg(long)]
        scope: Option<String>,

        /// ### 配置文件路径
        ///
        /// 指定沙箱配置文件或项目目录。
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// ### 从项目中删除沙箱
    ///
    /// 从项目配置中移除指定的沙箱。
    ///
    /// ### 别名
    /// `rm` 是 `remove` 的缩写形式。
    #[command(name = "remove", alias = "rm")]
    Remove {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 要删除的沙箱名称
        #[arg(required = true)]
        names: Vec<String>,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// ### 列出项目中的沙箱
    ///
    /// 显示项目中定义的所有沙箱。
    #[command(name = "list")]
    List {
        /// 是否仅列出沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否仅列出构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// ### 显示沙箱日志
    ///
    /// 查看沙箱的输出日志。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb log myapp              # 查看 myapp 的日志
    /// msb log -f myapp           # 实时跟踪日志（类似 tail -f）
    /// msb log -t 100 myapp       # 查看最后 100 行
    /// ```
    #[command(name = "log")]
    Log {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 沙箱名称
        #[arg(required = true)]
        name: String,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// ### 实时跟踪模式
        ///
        /// 持续输出新日志（类似 `tail -f`）。
        /// 需要系统安装了 `tail` 命令。
        #[arg(short = 'F', long)]
        follow: bool,

        /// ### 显示行数
        ///
        /// 显示末尾多少行日志。
        #[arg(short, long)]
        tail: Option<usize>,
    },

    /// ### 显示沙箱层级树
    ///
    /// 以树形结构显示组成沙箱的镜像层。
    /// 类似于 Docker 的 `docker image inspect`。
    #[command(name = "tree")]
    Tree {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 沙箱名称
        #[arg(required = true)]
        names: Vec<String>,

        /// ### 最大深度
        ///
        /// 限制显示的层级深度。
        /// `--level 2` 只显示前两层。
        #[arg(short = 'L', long)]
        level: Option<usize>,
    },

    /// ### 运行沙箱
    ///
    /// 启动并运行项目中定义的沙箱。
    ///
    /// ### 别名
    /// `r` 是 `run` 的缩写形式。
    ///
    /// ### 名称格式
    /// `NAME[~SCRIPT]` 表示可以指定沙箱名称和脚本：
    /// - `myapp` - 运行默认脚本
    /// - `myapp~shell` - 运行 shell 脚本
    ///
    /// ### 使用示例
    /// ```bash
    /// msb run myapp              # 运行沙箱
    /// msb run myapp~shell        # 运行 shell 脚本
    /// msb run -d myapp           # 后台运行
    /// msb run -x "python app.py" # 执行指定命令
    /// ```
    #[command(name = "run", alias = "r")]
    Run {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 沙箱名称（可带脚本）
        #[arg(required = true, name = "NAME[~SCRIPT]")]
        name: String,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// ### 后台运行
        ///
        /// 在后台运行沙箱，不阻塞终端。
        /// 也称为 "detach mode"（分离模式）。
        #[arg(short, long)]
        detach: bool,

        /// ### 执行命令
        ///
        /// 在沙箱内执行指定的命令，而不是默认脚本。
        ///
        /// ### 别名
        /// `-x` 是 `--exec` 的缩写。
        #[arg(short, long, short_alias = 'x')]
        exec: Option<String>,

        /// ### 额外参数
        ///
        /// 传递给脚本或命令的参数。
        ///
        /// ### `last = true` 说明
        /// 收集 `--` 之后的所有参数：
        /// ```bash
        /// msb run myapp -- arg1 arg2 arg3
        /// ```
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// ### 打开沙箱 Shell
    ///
    /// 在沙箱内启动一个交互式 shell 会话。
    /// 这是 `run` 命令的特例，自动运行 "shell" 脚本。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb shell myapp            # 打开默认 shell
    /// msb shell myapp -- -l      # 作为登录 shell
    /// ```
    #[command(name = "shell")]
    Shell {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 沙箱名称
        #[arg(required = true)]
        name: String,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// 后台运行
        #[arg(short, long)]
        detach: bool,

        /// 传递给 shell 的额外参数
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// ### 运行临时沙箱
    ///
    /// 直接从镜像运行临时沙箱，无需预先定义。
    /// 沙箱在退出后不会保留。
    ///
    /// ### 别名
    /// `x` 是 `exe` 的缩写。
    ///
    /// ### 与 `run` 的区别
    /// - `run`: 运行项目中已定义的沙箱
    /// - `exe`: 直接从镜像运行临时沙箱
    ///
    /// ### 使用示例
    /// ```bash
    /// # 运行 Python 沙箱
    /// msb exe python:3.9 -- python -c "print('hello')"
    ///
    /// # 运行带端口映射的 nginx
    /// msb exe nginx -p 8080:80
    /// ```
    #[command(name = "exe", alias = "x")]
    Exe {
        /// 是否应用于镜像（已废弃，保留用于兼容）
        #[arg(short, long)]
        image: bool,

        /// 镜像名称（可带脚本）
        #[arg(required = true, name = "NAME[~SCRIPT]")]
        name: String,

        /// CPU 核心数
        #[arg(long, alias = "cpu")]
        cpus: Option<u8>,

        /// 内存（MB）
        #[arg(long)]
        memory: Option<u32>,

        /// 卷挂载
        #[arg(short, long = "volume", name = "VOLUME")]
        volumes: Vec<String>,

        /// 端口映射
        #[arg(short, long = "port", name = "PORT")]
        ports: Vec<String>,

        /// 环境变量
        #[arg(long = "env", name = "ENV")]
        envs: Vec<String>,

        /// 工作目录
        #[arg(long)]
        workdir: Option<Utf8UnixPathBuf>,

        /// 网络作用域
        #[arg(long)]
        scope: Option<String>,

        /// 执行命令
        #[arg(short, long, short_alias = 'x')]
        exec: Option<String>,

        /// 额外参数
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// ### 安装全局脚本
    ///
    /// 从镜像安装脚本到用户级别，使其可在任何目录运行。
    /// 类似于 `npm install -g`。
    ///
    /// ### 别名
    /// `i` 是 `install` 的缩写。
    ///
    /// ### 使用示例
    /// ```bash
    /// # 安装 Python 脚本
    /// msb install python:3.9~my_script.py
    ///
    /// # 安装并指定别名
    /// msb install python:3.9~script.py myscript
    /// ```
    #[command(name = "install", alias = "i")]
    Install {
        /// 是否应用于镜像
        #[arg(short, long)]
        image: bool,

        /// 镜像名称（可带脚本）
        #[arg(required = true, name = "NAME[~SCRIPT]")]
        name: String,

        /// ### 别名
        ///
        /// 为安装的脚本指定一个别名。
        /// 如不指定，使用镜像名的最后一部分。
        #[arg()]
        alias: Option<String>,

        /// CPU 核心数
        #[arg(long, alias = "cpu")]
        cpus: Option<u8>,

        /// 内存（MB）
        #[arg(long)]
        memory: Option<u32>,

        /// 卷挂载
        #[arg(short, long = "volume", name = "VOLUME")]
        volumes: Vec<String>,

        /// 端口映射
        #[arg(short, long = "port", name = "PORT")]
        ports: Vec<String>,

        /// 环境变量
        #[arg(long = "env", name = "ENV")]
        envs: Vec<String>,

        /// 工作目录
        #[arg(long)]
        workdir: Option<Utf8UnixPathBuf>,

        /// 网络作用域
        #[arg(long)]
        scope: Option<String>,

        /// 执行命令
        #[arg(short, long, short_alias = 'x')]
        exec: Option<String>,

        /// 传递给脚本的参数（运行时使用）
        #[arg(last = true)]
        args: Vec<String>,
    },

    /// ### 卸载全局脚本
    ///
    /// 移除之前安装的全局脚本别名。
    #[command(name = "uninstall")]
    Uninstall {
        /// 要卸载的脚本名称
        script: Option<String>,
    },

    /// ### 应用项目配置
    ///
    /// 根据配置文件启动或停止所有沙箱。
    /// 类似于 Docker Compose 的 `up`。
    #[command(name = "apply")]
    Apply {
        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// 后台运行
        #[arg(short, long)]
        detach: bool,
    },

    /// ### 启动项目沙箱
    ///
    /// 启动项目中定义的所有或指定沙箱。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb up                 # 启动所有沙箱
    /// msb up webapp db       # 启动指定的沙箱
    /// ```
    #[command(name = "up")]
    Up {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 要启动的沙箱名称
        /// 如省略，启动配置文件中定义的所有沙箱。
        names: Vec<String>,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// 后台运行
        #[arg(short, long)]
        detach: bool,
    },

    /// ### 停止项目沙箱
    ///
    /// 停止项目中定义的所有或指定沙箱。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb down               # 停止所有沙箱
    /// msb down webapp        # 停止指定的沙箱
    /// ```
    #[command(name = "down")]
    Down {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 要停止的沙箱名称
        /// 如省略，停止配置文件中定义的所有沙箱。
        names: Vec<String>,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// ### 显示沙箱状态
    ///
    /// 显示运行中沙箱的状态信息（CPU、内存、网络等）。
    ///
    /// ### 别名
    /// `ps` 和 `stat` 是 `status` 的别名。
    #[command(name = "status", alias = "ps", alias = "stat")]
    Status {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 是否应用于构建沙箱
        #[arg(short, long)]
        build: bool,

        /// 要显示状态的沙箱名称
        #[arg()]
        names: Vec<String>,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,
    },

    /// ### 清理缓存
    ///
    /// 清理沙箱层、元数据等缓存文件。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb clean               # 清理当前项目
    /// msb clean --user        # 清理用户级缓存
    /// msb clean --all         # 清理所有
    /// msb clean myapp         # 清理指定沙箱
    /// ```
    #[command(name = "clean")]
    Clean {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 沙箱名称
        #[arg()]
        name: Option<String>,

        /// ### 清理用户级缓存
        ///
        /// 清理 `$MICROSANDBOX_HOME` 目录。
        /// 这会影响所有项目。
        #[arg(short, long)]
        user: bool,

        /// ### 清理所有
        ///
        /// 同时清理用户级和项目级缓存。
        #[arg(short, long)]
        all: bool,

        /// 配置文件路径
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// ### 强制清理
        ///
        /// 不询问确认，直接删除。
        #[arg(short = 'F', long)]
        force: bool,
    },

    /// ### 构建镜像
    ///
    /// 从沙箱或构建定义构建 OCI 镜像。
    #[command(name = "build")]
    Build {
        /// 从沙箱构建
        #[arg(short, long)]
        sandbox: bool,

        /// 从构建定义构建
        #[arg(short, long)]
        build: bool,

        /// 要构建的沙箱名称
        #[arg(required = true)]
        names: Vec<String>,

        /// ### 创建快照
        ///
        /// 创建沙箱的快照镜像，而不是完整镜像。
        /// 快照只包含变更层。
        #[arg(long)]
        snapshot: bool,
    },

    /// ### 拉取镜像
    ///
    /// 从容器镜像仓库拉取镜像到本地。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb pull python:3.9
    /// msb pull docker.io/library/nginx:latest
    /// ```
    #[command(name = "pull")]
    Pull {
        /// 镜像名称
        /// 类型 `Reference` 是 OCI 镜像引用格式。
        #[arg(required = true)]
        name: Reference,

        /// ### 层文件存储路径
        ///
        /// 指定存储镜像层文件的目录。
        /// 如不指定，使用默认缓存目录。
        #[arg(short = 'L', long)]
        layer_path: Option<PathBuf>,
    },

    /// ### 登录镜像仓库
    ///
    /// 登录到容器镜像仓库（如 Docker Hub）。
    /// 用于拉取私有镜像或推送镜像。
    #[command(name = "login")]
    Login,

    /// ### 推送镜像
    ///
    /// 将本地镜像推送到远程仓库。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb push myimage:latest
    /// msb push --image myimage:latest
    /// ```
    #[command(name = "push")]
    Push {
        /// 是否应用于镜像
        #[arg(short, long)]
        image: bool,

        /// 镜像名称
        #[arg(required = true)]
        name: String,
    },

    /// ### 管理 microsandbox 自身
    ///
    /// 升级或卸载 microsandbox 工具。
    #[command(name = "self")]
    Self_ {
        /// ### 操作类型
        ///
        /// 使用 `#[arg(value_enum)]` 从 `SelfAction` 枚举定义可选值。
        #[arg(value_enum)]
        action: SelfAction,
    },

    /// ### 管理沙箱服务器
    ///
    /// 启动、停止沙箱服务器，或执行服务器相关操作。
    /// 服务器提供 API 接口和 MCP（Model Context Protocol）服务。
    #[command(name = "server")]
    Server {
        /// 服务器子命令
        #[command(subcommand)]
        subcommand: ServerSubcommand,
    },

    /// ### 显示版本号
    ///
    /// 打印 microsandbox 的版本信息。
    #[command(name = "version")]
    Version,
}

/// ## ServerSubcommand 枚举
///
/// `msb server` 命令的子命令。
/// 用于管理沙箱服务器（API 服务器和 MCP 服务器）。
#[derive(Debug, Parser)]
pub enum ServerSubcommand {
    /// ### 启动沙箱服务器
    ///
    /// 启动一个 HTTP API 服务器，提供：
    /// - REST API 用于沙箱管理
    /// - MCP（Model Context Protocol）接口
    /// - WebSocket 实时通信
    ///
    /// ### 使用示例
    /// ```bash
    /// msb server start                    # 使用默认设置启动
    /// msb server start --port 8080        # 指定端口
    /// msb server start -d                 # 后台运行
    /// ```
    Start {
        /// 监听主机地址
        #[arg(long)]
        host: Option<String>,

        /// 监听端口
        #[arg(long)]
        port: Option<u16>,

        /// 项目目录
        #[arg(short = 'p', long = "path")]
        project_dir: Option<PathBuf>,

        /// ### 开发模式
        ///
        /// 启用开发模式，提供更详细的日志和调试功能。
        #[arg(long = "dev")]
        dev_mode: bool,

        /// ### 密钥
        ///
        /// 用于 JWT 令牌生成和验证的密钥。
        /// 如不指定，自动生成随机密钥。
        #[arg(short, long)]
        key: Option<String>,

        /// 后台运行
        #[arg(short, long)]
        detach: bool,

        /// ### 重置密钥
        ///
        /// 重新生成新的服务器密钥。
        #[arg(short, long)]
        reset_key: bool,
    },

    /// ### 停止沙箱服务器
    ///
    /// 停止正在运行的沙箱服务器进程。
    Stop,

    /// ### 生成 API 密钥
    ///
    /// 生成一个新的 API 密钥用于认证。
    ///
    /// ### 使用示例
    /// ```bash
    /// msb server keygen                   # 生成默认有效期的密钥
    /// msb server keygen --expire 1h       # 1 小时后过期
    /// msb server keygen --expire 7d       # 7 天后过期
    /// ```
    #[command(name = "keygen")]
    Keygen {
        /// ### 令牌过期时间
        ///
        /// 格式：`<数字><单位>`
        ///
        /// ### 支持的时间单位
        /// | 单位 | 含义 |
        /// |------|------|
        /// | `s` | 秒 |
        /// | `m` | 分钟 |
        /// | `h` | 小时 |
        /// | `d` | 天 |
        /// | `w` | 周 |
        /// | `mo` | 月（30 天） |
        /// | `y` | 年（365 天） |
        #[arg(long)]
        expire: Option<String>,
    },

    /// ### 显示服务器沙箱日志
    ///
    /// 查看服务器管理的沙箱的日志。
    #[command(name = "log")]
    Log {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 沙箱名称
        #[arg(required = true)]
        name: String,

        /// 实时跟踪
        #[arg(short, long)]
        follow: bool,

        /// 显示行数
        #[arg(short, long)]
        tail: Option<usize>,
    },

    /// ### 列出服务器沙箱
    ///
    /// 显示服务器管理的所有沙箱。
    #[command(name = "list")]
    List,

    /// ### 显示服务器沙箱状态
    ///
    /// 显示服务器管理的沙箱的运行状态。
    #[command(name = "status")]
    Status {
        /// 是否应用于沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 沙箱名称
        #[arg()]
        names: Vec<String>,
    },

    /// ### SSH 连接到沙箱
    ///
    /// 通过 SSH 协议连接到沙箱。
    /// （当前版本未实现）
    #[command(name = "ssh")]
    Ssh {
        /// 是否 SSH 到沙箱
        #[arg(short, long)]
        sandbox: bool,

        /// 沙箱名称
        #[arg(required = true)]
        name: String,
    },
}

/// ## SelfAction 枚举
///
/// 定义 `msb self` 命令的可用操作。
/// 使用 `clap::ValueEnum` 派生，clap 会自动处理枚举值的解析。
///
/// ### 使用示例
/// ```bash
/// msb self upgrade      # 升级 microsandbox
/// msb self uninstall    # 卸载 microsandbox
/// ```
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SelfAction {
    /// 升级 microsandbox 到最新版本
    Upgrade,

    /// 从系统中卸载 microsandbox
    Uninstall,
}

//-------------------------------------------------------------------------------------------------
// Functions: Helper Functions - 辅助函数
//-------------------------------------------------------------------------------------------------

/// ## 解析 KEY=VALUE 格式的字符串
///
/// 这是一个通用的键值对解析函数，用于解析命令行参数中的 `KEY=VALUE` 格式。
///
/// ### 泛型参数
/// - `T`: 键的类型，必须实现 `FromStr` trait
/// - `U`: 值的类型，必须实现 `FromStr` trait
///
/// ### 参数
/// - `s`: 输入字符串，格式为 `KEY=VALUE`
///
/// ### 返回值
/// - `Ok((key, value))`: 解析成功，返回键值对元组
/// - `Err(...)`: 解析失败，返回错误
///
/// ### 使用示例
/// ```rust,ignore
/// let result = parse_key_val::<String, String>("name=value");
/// assert_eq!(result, Ok(("name".to_string(), "value".to_string())));
/// ```
///
/// ### `where` 子句说明
/// ```rust,ignore
/// where
///     T: std::str::FromStr,              // T 可以从字符串解析
///     T::Err: Error + Send + Sync + 'static,  // T 的错误类型可以实现这些 trait
///     U: std::str::FromStr,              // U 可以从字符串解析
///     U::Err: Error + Send + Sync + 'static,  // U 的错误类型可以实现这些 trait
/// ```
fn parse_key_val<T, U>(s: &str) -> Result<(T, U), Box<dyn Error + Send + Sync + 'static>>
where
    T: std::str::FromStr,
    T::Err: Error + Send + Sync + 'static,
    U: std::str::FromStr,
    U::Err: Error + Send + Sync + 'static,
{
    // 查找等号的位置
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=value: no `=` found in `{s}`"))?;

    // 分割字符串并分别解析键和值
    // `s[..pos]` 是等号前的部分（键）
    // `s[pos + 1..]` 是等号后的部分（值）
    Ok((s[..pos].parse()?, s[pos + 1..].parse()?))
}
