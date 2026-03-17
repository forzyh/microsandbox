//! Microsandbox 配置类型和辅助函数
//!
//! 本模块定义了 microsandbox 系统的核心配置结构。这些配置通过 YAML 文件定义，
//! 用于描述沙箱 (Sandbox)、构建 (Build)、元数据 (Meta) 等信息。
//!
//! ## 配置结构说明
//!
//! Microsandbox 配置文件包含以下几个主要部分：
//! - **meta** - 配置的元数据信息，如作者、描述、仓库等
//! - **modules** - 模块导入配置，用于组合多个配置文件
//! - **builds** - 构建任务配置，定义如何构建镜像或处理文件
//! - **sandboxes** - 沙箱配置，定义要运行的虚拟机实例

use std::{
    collections::HashMap,
    fmt::{self, Display},
    str::FromStr,
};

use getset::{Getters, Setters};
use semver::Version;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;
use typed_path::Utf8UnixPathBuf;

use crate::{
    MicrosandboxError, MicrosandboxResult,
    config::{EnvPair, PathPair, PortPair, ReferenceOrPath},
};

use super::{MicrosandboxBuilder, SandboxBuilder};

//--------------------------------------------------------------------------------------------------
// 常量
//--------------------------------------------------------------------------------------------------

/// 启动脚本的名称
///
/// 在沙箱配置中，如果定义了名为 "start" 的脚本，它将被用作默认的启动命令
pub const START_SCRIPT_NAME: &str = "start";

/// 沙箱的默认网络范围
///
/// NetworkScope::Public 表示沙箱可以与任何非私有地址通信
pub const DEFAULT_NETWORK_SCOPE: NetworkScope = NetworkScope::Public;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// Microsandbox 配置结构
///
/// 这是配置文件的顶层结构，包含所有沙箱、构建任务和模块的定义。
/// 通过 serde 从 YAML 文件反序列化而来。
///
/// ## 字段说明
/// * `meta` - 配置的元数据信息（可选）
/// * `modules` - 要导入的模块映射
/// * `builds` - 要运行的构建任务映射
/// * `sandboxes` - 要运行的沙箱映射
///
/// ## 使用示例
/// ```yaml
/// meta:
///   authors: ["John Doe"]
///   description: "示例配置"
///
/// sandboxes:
///   web_server:
///     image: "nginx:latest"
///     ports:
///       - "8080:80"
///
/// builds:
///   build_app:
///     image: "rust:1.70"
///     steps:
///       - "cargo build --release"
/// ```
#[derive(Debug, Default, Clone, Deserialize, Serialize, PartialEq, Getters)]
#[getset(get = "pub with_prefix")]
pub struct Microsandbox {
    /// 配置的元数据信息
    ///
    /// 包含作者、描述、仓库等信息，用于文档和识别
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) meta: Option<Meta>,

    /// 要导入的模块
    ///
    /// 模块允许将配置分散到多个文件中，然后在此处导入
    /// 键是配置文件的路径，值是模块中要导入的组件
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub(crate) modules: HashMap<String, Module>,

    /// 要运行的构建任务
    ///
    /// 构建任务用于在运行沙箱之前执行一些预处理工作，
    /// 如编译代码、安装依赖等
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub(crate) builds: HashMap<String, Build>,

    /// 要运行的沙箱
    ///
    /// 每个沙箱代表一个独立的微虚拟机实例
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub(crate) sandboxes: HashMap<String, Sandbox>,
}

/// 配置元数据
///
/// 提供关于配置文件的描述性信息，便于文档化和团队协件。
/// 所有字段都是可选的。
///
/// ## 字段说明
/// * `authors` - 作者列表，通常为 "姓名 <邮箱>" 格式
/// * `description` - 配置的简短描述
/// * `homepage` - 项目主页 URL
/// * `repository` - 代码仓库 URL
/// * `readme` - README 文件的路径
/// * `tags` - 标签列表，用于分类
/// * `icon` - 图标文件的路径
#[derive(Debug, Default, Clone, Serialize, Deserialize, TypedBuilder, PartialEq, Eq, Getters)]
#[getset(get = "pub with_prefix")]
pub struct Meta {
    /// 配置的作者列表
    ///
    /// 建议使用 "姓名 <邮箱>" 的格式，如 "John Doe <john@example.com>"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) authors: Option<Vec<String>>,

    /// 沙箱或配置的描述
    ///
    /// 简短说明此配置用途的文本
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) description: Option<String>,

    /// 项目主页 URL
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) homepage: Option<String>,

    /// 代码仓库 URL
    ///
    /// 通常是 GitHub、GitLab 等平台的仓库地址
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) repository: Option<String>,

    /// README 文件的路径
    ///
    /// 指向包含详细文档的 Markdown 文件
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        serialize_with = "serialize_optional_path",
        deserialize_with = "deserialize_optional_path"
    )]
    #[builder(default, setter(strip_option))]
    pub(crate) readme: Option<Utf8UnixPathBuf>,

    /// 标签列表
    ///
    /// 用于分类和搜索的关键词，如 ["rust", "web", "api"]
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) tags: Option<Vec<String>>,

    /// 图标文件的路径
    ///
    /// 用于 UI 显示的图标文件路径
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        serialize_with = "serialize_optional_path",
        deserialize_with = "deserialize_optional_path"
    )]
    #[builder(default, setter(strip_option))]
    pub(crate) icon: Option<Utf8UnixPathBuf>,
}

/// 模块导入的组件映射
///
/// 当从其他配置文件导入模块时，可以指定只导入某些组件，
/// 并可以为它们设置别名。
///
/// ## 字段说明
/// * `as_` - 组件的别名（可选）
///
/// ## 使用示例
/// ```yaml
/// modules:
///   "./database.yaml":
///     database: {}  # 使用原名
///   "./redis.yaml":
///     redis:
///       as: "cache"  # 使用别名 "cache"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder, PartialEq, Getters)]
#[getset(get = "pub with_prefix")]
pub struct ComponentMapping {
    /// 组件的别名
    ///
    /// 当设置别名时，导入的组件将使用此名称而非原名
    #[serde(skip_serializing_if = "Option::is_none", default, rename = "as")]
    #[builder(default, setter(strip_option))]
    pub(crate) as_: Option<String>,
}

/// 模块导入配置
///
/// 封装了模块中组件的映射关系。
/// HashMap 的键是组件名，值是可选的 ComponentMapping（用于设置别名）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Module(pub HashMap<String, Option<ComponentMapping>>);

/// 构建任务配置
///
/// 定义了一个构建任务的所有参数。构建任务用于在运行沙箱之前
/// 执行预处理工作，如编译代码、安装依赖、处理文件等。
///
/// ## 核心字段说明
/// * `image` - 使用的镜像（OCI 引用或本地 rootfs 路径）
/// * `memory` - 内存大小（MiB）
/// * `cpus` - vCPU 数量
/// * `volumes` - 要挂载的卷列表
/// * `ports` - 要暴露的端口列表
/// * `envs` - 环境变量列表
/// * `steps` - 要执行的步骤（命令）列表
/// * `command` - 默认命令
/// * `imports` - 要导入的文件映射
/// * `exports` - 构建产物（导出文件）映射
///
/// ## 使用示例
/// ```yaml
/// builds:
///   build_app:
///     image: "rust:1.70"
///     memory: 2048
///     cpus: 2
///     volumes:
///       - "./src:/app/src"
///     envs:
///       - "RUST_BACKTRACE=1"
///     workdir: "/app"
///     shell: "/bin/bash"
///     steps:
///       - "cargo build --release"
///     exports:
///       binary: "/app/target/release/myapp"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder, PartialEq, Getters)]
#[getset(get = "pub with_prefix")]
pub struct Build {
    /// 使用的镜像
    ///
    /// 可以是 OCI 镜像引用（如 "rust:1.70"）或本地 rootfs 路径
    pub(crate) image: ReferenceOrPath,

    /// 内存大小（MiB）
    ///
    /// 分配给构建任务的内存量，单位为 MiB（兆字节）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) memory: Option<u32>,

    /// vCPU 数量
    ///
    /// 分配给构建任务的虚拟 CPU 核心数
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) cpus: Option<u8>,

    /// 要挂载的卷列表
    ///
    /// 格式为 "主机路径：访客路径"，用于在主机和 VM 之间共享文件
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[builder(default)]
    pub(crate) volumes: Vec<PathPair>,

    /// 要暴露的端口列表
    ///
    /// 格式为 "主机端口：访客端口"，用于端口转发
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[builder(default)]
    pub(crate) ports: Vec<PortPair>,

    /// 环境变量列表
    ///
    /// 格式为 "NAME=value"，设置到构建环境中
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[builder(default)]
    pub(crate) envs: Vec<EnvPair>,

    /// 依赖的构建任务列表
    ///
    /// 此构建任务执行前必须先完成的构建任务名称
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[builder(default)]
    pub(crate) depends_on: Vec<String>,

    /// 工作目录
    ///
    /// 构建命令执行时的工作目录
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        serialize_with = "serialize_optional_path",
        deserialize_with = "deserialize_optional_path"
    )]
    #[builder(default, setter(strip_option))]
    pub(crate) workdir: Option<Utf8UnixPathBuf>,

    /// 使用的 shell
    ///
    /// 执行命令时使用的 shell，如 "/bin/bash" 或 "/bin/sh"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[builder(default, setter(strip_option))]
    pub(crate) shell: Option<String>,

    /// 构建步骤列表
    ///
    /// 按顺序执行的命令列表，每个步骤都是一个 shell 命令
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[builder(default)]
    pub(crate) steps: Vec<String>,

    /// 默认命令
    ///
    /// 构建任务执行的主要命令，作为参数列表
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    #[builder(default)]
    pub(crate) command: Vec<String>,

    /// 要导入的文件映射
    ///
    /// 键是导入后的名称，值是源文件路径
    #[serde(
        skip_serializing_if = "HashMap::is_empty",
        default,
        serialize_with = "serialize_path_map",
        deserialize_with = "deserialize_path_map"
    )]
    #[builder(default)]
    pub(crate) imports: HashMap<String, Utf8UnixPathBuf>,

    /// 构建产物（导出文件）映射
    ///
    /// 键是导出名称，值是构建产物的路径
    #[serde(
        skip_serializing_if = "HashMap::is_empty",
        default,
        serialize_with = "serialize_path_map",
        deserialize_with = "deserialize_path_map"
    )]
    #[builder(default)]
    pub(crate) exports: HashMap<String, Utf8UnixPathBuf>,
}

/// 沙箱网络范围配置
///
/// 定义沙箱可以访问的网络地址范围。这提供了网络隔离的不同级别。
///
/// ## 变体说明
/// * `None` - 完全隔离，沙箱无法与任何其他沙箱通信
/// * `Group` - 组内通信，沙箱只能与同组沙箱通信（未实现）
/// * `Public` - 公共网络，沙箱可以与任何非私有地址通信（默认）
/// * `Any` - 完全开放，沙箱可以与任何地址通信
///
/// ## 使用示例
/// ```yaml
/// sandboxes:
///   web_server:
///     image: "nginx:latest"
///     scope: "public"  # 可以访问外部网络
///   internal_db:
///     image: "postgres:15"
///     scope: "none"    # 完全隔离，只能本地访问
/// ```
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum NetworkScope {
    /// 完全隔离
    ///
    /// 沙箱无法与任何其他沙箱或外部网络通信
    #[serde(rename = "none")]
    None = 0,

    /// 组内通信（未实现）
    ///
    /// 沙箱只能与同组内的其他沙箱通信
    #[serde(rename = "group")]
    Group = 1,

    /// 公共网络（默认）
    ///
    /// 沙箱可以与任何非私有地址通信
    /// 这是默认值，适用于大多数需要访问外部服务的场景
    #[serde(rename = "public")]
    #[default]
    Public = 2,

    /// 完全开放
    ///
    /// 沙箱可以与任何地址通信，包括私有地址
    #[serde(rename = "any")]
    Any = 3,
}

/// 沙箱配置
///
/// 定义了一个沙箱实例的所有参数。沙箱是 microsandbox 的核心，
/// 代表一个正在运行或可以运行的微虚拟机实例。
///
/// ## 核心字段说明
/// * `version` - 沙箱版本（语义化版本）
/// * `meta` - 沙箱元数据
/// * `image` - 使用的镜像（OCI 引用或本地 rootfs 路径）
/// * `memory` - 内存大小（MiB）
/// * `cpus` - vCPU 数量
/// * `volumes` - 要挂载的卷列表
/// * `ports` - 要暴露的端口列表
/// * `envs` - 环境变量列表
/// * `depends_on` - 依赖的沙箱列表
/// * `scripts` - 可运行的脚本映射
/// * `command` - 默认命令
/// * `scope` - 网络范围配置
///
/// ## 使用示例
/// ```yaml
/// sandboxes:
///   api_server:
///     version: "1.0.0"
///     image: "rust:1.70"
///     memory: 1024
///     cpus: 2
///     volumes:
///       - "./src:/app/src"
///     ports:
///       - "8080:8080"
///     envs:
///       - "RUST_BACKTRACE=1"
///     depends_on:
///       - "database"
///     workdir: "/app"
///     shell: "/bin/bash"
///     scripts:
///       start: "cargo run"
///       test: "cargo test"
///     scope: "public"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Getters, Setters)]
#[getset(get = "pub with_prefix", set = "pub with_prefix")]
pub struct Sandbox {
    /// 沙箱版本
    ///
    /// 使用语义化版本（Semantic Versioning），如 "1.0.0"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) version: Option<Version>,

    /// 沙箱元数据
    ///
    /// 包含作者、描述、标签等信息
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) meta: Option<Meta>,

    /// 使用的镜像
    ///
    /// 可以是 OCI 镜像引用（如 "alpine:latest"）或本地 rootfs 路径
    pub(crate) image: ReferenceOrPath,

    /// 内存大小（MiB）
    ///
    /// 分配给沙箱的内存量，单位为 MiB（兆字节）
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) memory: Option<u32>,

    /// vCPU 数量
    ///
    /// 分配给沙箱的虚拟 CPU 核心数
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) cpus: Option<u8>,

    /// 要挂载的卷列表
    ///
    /// 格式为 "主机路径：访客路径"，用于在主机和 VM 之间共享文件
    /// PathPair 类型确保路径格式正确
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) volumes: Vec<PathPair>,

    /// 要暴露的端口列表
    ///
    /// 格式为 "主机端口：访客端口"，用于端口转发
    /// PortPair 类型确保端口格式正确
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) ports: Vec<PortPair>,

    /// 环境变量列表
    ///
    /// 格式为 "NAME=value"，设置到沙箱环境中
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) envs: Vec<EnvPair>,

    /// 依赖的沙箱列表
    ///
    /// 此沙箱启动前必须先启动的沙箱名称
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) depends_on: Vec<String>,

    /// 工作目录
    ///
    /// 沙箱中进程启动时的工作目录
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        serialize_with = "serialize_optional_path",
        deserialize_with = "deserialize_optional_path"
    )]
    pub(crate) workdir: Option<Utf8UnixPathBuf>,

    /// 使用的 shell
    ///
    /// 执行命令时使用的 shell，如 "/bin/bash" 或 "/bin/sh"
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub(crate) shell: Option<String>,

    /// 可运行的脚本映射
    ///
    /// 键是脚本名称，值是脚本内容
    /// 特殊脚本 "start" 被用作默认启动脚本
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub(crate) scripts: HashMap<String, String>,

    /// 默认命令
    ///
    /// 沙箱启动时执行的主要命令，作为参数列表
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) command: Vec<String>,

    /// 要导入的文件映射
    ///
    /// 键是导入后的名称，值是源文件路径
    #[serde(
        skip_serializing_if = "HashMap::is_empty",
        default,
        serialize_with = "serialize_path_map",
        deserialize_with = "deserialize_path_map"
    )]
    pub(crate) imports: HashMap<String, Utf8UnixPathBuf>,

    /// 沙箱产物（导出文件）映射
    ///
    /// 键是导出名称，值是沙箱中产物的路径
    #[serde(
        skip_serializing_if = "HashMap::is_empty",
        default,
        serialize_with = "serialize_path_map",
        deserialize_with = "deserialize_path_map"
    )]
    pub(crate) exports: HashMap<String, Utf8UnixPathBuf>,

    /// 沙箱的网络范围
    ///
    /// 定义沙箱可以访问的网络地址范围
    /// 默认为 NetworkScope::Public
    #[serde(default)]
    pub(crate) scope: NetworkScope,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl Microsandbox {
    /// 沙箱依赖链的最大深度
    ///
    /// 用于防止循环依赖和过深的依赖链
    pub const MAX_DEPENDENCY_DEPTH: usize = 32;

    /// 根据名称获取沙箱配置
    ///
    /// ## 参数
    /// * `sandbox_name` - 沙箱的名称
    ///
    /// ## 返回值
    /// 如果找到则返回沙箱配置的引用，否则返回 None
    pub fn get_sandbox(&self, sandbox_name: &str) -> Option<&Sandbox> {
        self.sandboxes.get(sandbox_name)
    }

    /// 根据名称获取构建任务配置
    ///
    /// ## 参数
    /// * `build_name` - 构建任务的名称
    ///
    /// ## 返回值
    /// 如果找到则返回构建任务配置的引用，否则返回 None
    pub fn get_build(&self, build_name: &str) -> Option<&Build> {
        self.builds.get(build_name)
    }

    /// 验证配置的有效性
    ///
    /// 检查配置是否满足所有约束条件。
    /// 当前主要验证所有沙箱配置的有效性。
    ///
    /// ## 返回值
    /// 如果配置有效则返回 Ok(())，否则返回错误
    pub fn validate(&self) -> MicrosandboxResult<()> {
        // 验证所有沙箱配置
        for sandbox in self.sandboxes.values() {
            sandbox.validate()?;
        }

        Ok(())
    }

    /// 创建 Microsandbox 配置的构建器
    ///
    /// 使用 TypedBuilder 模式来构建 Microsandbox 配置。
    /// 参见 [`MicrosandboxBuilder`] 了解可用的选项。
    pub fn builder() -> MicrosandboxBuilder {
        MicrosandboxBuilder::default()
    }
}

impl Sandbox {
    /// 创建沙箱配置的构建器
    ///
    /// 使用 TypedBuilder 模式来构建 Sandbox 配置。
    /// 参见 [`SandboxBuilder`] 了解可用的选项。
    pub fn builder() -> SandboxBuilder<()> {
        SandboxBuilder::default()
    }

    /// 验证沙箱配置的有效性
    ///
    /// 检查沙箱是否定义了启动方式。
    /// 必须有 start 脚本、command 或 shell 中的至少一个。
    ///
    /// ## 返回值
    /// 如果配置有效则返回 Ok(())，否则返回错误
    ///
    /// ## 错误情况
    /// * `MissingStartOrExecOrShell` - 没有定义任何启动方式
    pub fn validate(&self) -> MicrosandboxResult<()> {
        // 如果没有定义 start 脚本、command 和 shell，则返回错误
        if !self.scripts.contains_key(START_SCRIPT_NAME)
            && self.command.is_empty()
            && self.shell.is_none()
        {
            return Err(MicrosandboxError::MissingStartOrExecOrShell);
        }

        Ok(())
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl TryFrom<&str> for NetworkScope {
    type Error = MicrosandboxError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "none" => Ok(NetworkScope::None),
            "group" => Ok(NetworkScope::Group),
            "public" => Ok(NetworkScope::Public),
            "any" => Ok(NetworkScope::Any),
            _ => Err(MicrosandboxError::InvalidNetworkScope(s.to_string())),
        }
    }
}

impl Display for NetworkScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkScope::None => write!(f, "none"),
            NetworkScope::Group => write!(f, "group"),
            NetworkScope::Public => write!(f, "public"),
            NetworkScope::Any => write!(f, "any"),
        }
    }
}

impl FromStr for NetworkScope {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(NetworkScope::try_from(s)?)
    }
}

impl TryFrom<String> for NetworkScope {
    type Error = MicrosandboxError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        NetworkScope::try_from(s.as_str())
    }
}

impl TryFrom<u8> for NetworkScope {
    type Error = MicrosandboxError;

    fn try_from(u: u8) -> Result<Self, Self::Error> {
        match u {
            0 => Ok(NetworkScope::None),
            1 => Ok(NetworkScope::Group),
            2 => Ok(NetworkScope::Public),
            3 => Ok(NetworkScope::Any),
            _ => Err(MicrosandboxError::InvalidNetworkScope(u.to_string())),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// 函数：序列化辅助函数
//--------------------------------------------------------------------------------------------------

/// 序列化可选的 Unix 路径
///
/// 将 Option<Utf8UnixPathBuf> 转换为字符串进行序列化。
/// 如果为 None，则序列化为 null。
fn serialize_optional_path<S>(
    path: &Option<Utf8UnixPathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match path {
        Some(p) => serializer.serialize_str(p.as_str()),
        None => serializer.serialize_none(),
    }
}

/// 反序列化可选的 Unix 路径
///
/// 从字符串反序列化为 Option<Utf8UnixPathBuf>。
fn deserialize_optional_path<'de, D>(deserializer: D) -> Result<Option<Utf8UnixPathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|s| Ok(Utf8UnixPathBuf::from(s)))
        .transpose()
}

/// 序列化路径 HashMap
///
/// 将 HashMap<String, Utf8UnixPathBuf> 序列化为 JSON 对象，
/// 其中路径被转换为字符串。
fn serialize_path_map<S>(
    map: &HashMap<String, Utf8UnixPathBuf>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::ser::SerializeMap;
    let mut map_ser = serializer.serialize_map(Some(map.len()))?;
    for (k, v) in map {
        map_ser.serialize_entry(k, v.as_str())?;
    }
    map_ser.end()
}

/// 反序列化路径 HashMap
///
/// 从 JSON 对象反序列化为 HashMap<String, Utf8UnixPathBuf>。
/// 先将值反序列化为 String，再转换为 Utf8UnixPathBuf。
fn deserialize_path_map<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Utf8UnixPathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    HashMap::<String, String>::deserialize(deserializer).map(|string_map| {
        string_map
            .into_iter()
            .map(|(k, v)| (k, Utf8UnixPathBuf::from(v)))
            .collect()
    })
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_microsandbox_config_empty_config() {
        let yaml = r#"
            # Empty config with no fields
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();
        assert!(config.meta.is_none());
        assert!(config.modules.is_empty());
        assert!(config.builds.is_empty());
        assert!(config.sandboxes.is_empty());
    }

    #[test]
    fn test_microsandbox_config_default_config() {
        // 测试 Default trait 实现
        let config = Microsandbox::default();
        assert!(config.meta.is_none());
        assert!(config.modules.is_empty());
        assert!(config.builds.is_empty());
        assert!(config.sandboxes.is_empty());

        // 测试空节
        let yaml = r#"
            meta: {}
            modules: {}
            builds: {}
            sandboxes: {}
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();
        assert!(config.meta.unwrap() == Meta::default());
        assert!(config.modules.is_empty());
        assert!(config.builds.is_empty());
        assert!(config.sandboxes.is_empty());
    }

    #[test]
    fn test_microsandbox_config_minimal_sandbox_config() {
        let yaml = r#"
            sandboxes:
              test:
                image: "alpine:latest"
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();
        let sandboxes = &config.sandboxes;
        let sandbox = sandboxes.get("test").unwrap();

        assert!(sandbox.version.is_none());
        assert!(sandbox.memory.is_none());
        assert!(sandbox.cpus.is_none());
        assert!(sandbox.volumes.is_empty());
        assert!(sandbox.ports.is_empty());
        assert!(sandbox.envs.is_empty());
        assert!(sandbox.workdir.is_none());
        assert!(sandbox.shell.is_none());
        assert!(sandbox.scripts.is_empty());
        assert_eq!(sandbox.scope, NetworkScope::Public);
    }

    #[test]
    fn test_microsandbox_config_default_scope() {
        // 测试沙箱的默认 scope 是 Public
        let sandbox = Sandbox::builder()
            .image(ReferenceOrPath::Reference("alpine:latest".parse().unwrap()))
            .shell("/bin/sh")
            .build();
        assert_eq!(sandbox.scope, NetworkScope::Public);

        // 测试 YAML 中的默认 scope
        let yaml = r#"
            sandboxes:
              test:
                image: "alpine:latest"
                shell: "/bin/sh"
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();
        let sandboxes = &config.sandboxes;
        let sandbox = sandboxes.get("test").unwrap();

        assert_eq!(sandbox.scope, NetworkScope::Public);
    }

    #[test]
    fn test_microsandbox_config_basic_microsandbox_config() {
        let yaml = r#"
            meta:
              authors:
                - "John Doe <john@example.com>"
              description: "Test configuration"
              homepage: "https://example.com"
              repository: "https://github.com/example/test"
              readme: "./README.md"
              tags:
                - "test"
                - "example"
              icon: "./icon.png"

            sandboxes:
              test_sandbox:
                version: "1.0.0"
                image: "alpine:latest"
                memory: 1024
                cpus: 2
                volumes:
                  - "./src:/app/src"
                ports:
                  - "8080:80"
                envs:
                  - "DEBUG=true"
                workdir: "/app"
                shell: "/bin/sh"
                scripts:
                  start: "echo 'Hello, World!'"
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();

        // 验证 meta 节
        let meta = config.meta.as_ref().unwrap();
        assert_eq!(
            meta.authors.as_ref().unwrap()[0],
            "John Doe <john@example.com>"
        );
        assert_eq!(meta.description.as_ref().unwrap(), "Test configuration");
        assert_eq!(meta.homepage.as_ref().unwrap(), "https://example.com");
        assert_eq!(
            meta.repository.as_ref().unwrap(),
            "https://github.com/example/test"
        );
        assert_eq!(
            meta.readme.as_ref().unwrap(),
            &Utf8UnixPathBuf::from("./README.md")
        );
        assert_eq!(meta.tags.as_ref().unwrap(), &vec!["test", "example"]);
        assert_eq!(
            meta.icon.as_ref().unwrap(),
            &Utf8UnixPathBuf::from("./icon.png")
        );

        // 验证 sandbox 节
        let sandboxes = &config.sandboxes;
        let sandbox = sandboxes.get("test_sandbox").unwrap();
        assert_eq!(sandbox.version.as_ref().unwrap().to_string(), "1.0.0");
        assert_eq!(sandbox.memory.unwrap(), 1024);
        assert_eq!(sandbox.cpus.unwrap(), 2);
        assert_eq!(sandbox.volumes[0].to_string(), "./src:/app/src");
        assert_eq!(sandbox.ports[0].to_string(), "8080:80");
        assert_eq!(sandbox.envs[0].to_string(), "DEBUG=true");
        assert_eq!(
            sandbox.workdir.as_ref().unwrap(),
            &Utf8UnixPathBuf::from("/app")
        );
        assert_eq!(sandbox.shell, Some("/bin/sh".to_string()));
        assert_eq!(
            sandbox.scripts.get("start").unwrap(),
            "echo 'Hello, World!'"
        );
    }

    #[test]
    fn test_microsandbox_config_full_microsandbox_config() {
        let yaml = r#"
            meta:
              description: "Full test configuration"

            modules:
              "./database.yaml":
                database: {}
              "./redis.yaml":
                redis:
                  as: "cache"

            builds:
              base_build:
                image: "python:3.11-slim"
                memory: 2048
                cpus: 2
                volumes:
                  - "./requirements.txt:/build/requirements.txt"
                envs:
                  - "PYTHON_VERSION=3.11"
                workdir: "/build"
                shell: "/bin/bash"
                steps:
                  - "pip install -r requirements.txt"
                imports:
                  requirements: "./requirements.txt"
                exports:
                  packages: "/build/dist/packages"

            sandboxes:
              api:
                version: "1.0.0"
                image: "python:3.11-slim"
                memory: 1024
                cpus: 1
                volumes:
                  - "./api:/app/src"
                ports:
                  - "8000:8000"
                envs:
                  - "DEBUG=false"
                depends_on:
                  - "database"
                  - "cache"
                workdir: "/app"
                shell: "/bin/bash"
                scripts:
                  start: "python -m uvicorn src.main:app"
                scope: "public"
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();

        // 测试 modules
        let modules = &config.modules;
        assert!(modules.contains_key("./database.yaml"));
        assert!(modules.contains_key("./redis.yaml"));

        // 修复 ComponentMapping.as_() 的访问
        let redis_module = &modules.get("./redis.yaml").unwrap().0;
        let redis_comp = redis_module.get("redis").unwrap().as_ref().unwrap();
        // 直接访问 as_ 字段（作为字段而非方法）
        assert_eq!(redis_comp.as_.as_ref().unwrap(), "cache");

        // 测试 builds
        let builds = &config.builds;
        let base_build = builds.get("base_build").unwrap();
        assert_eq!(base_build.memory.unwrap(), 2048);
        assert_eq!(base_build.cpus.unwrap(), 2);
        assert_eq!(
            base_build.workdir.as_ref().unwrap(),
            &Utf8UnixPathBuf::from("/build")
        );
        assert_eq!(base_build.shell, Some("/bin/bash".to_string()));
        assert_eq!(
            base_build.steps.get(0).unwrap(),
            "pip install -r requirements.txt"
        );
        assert_eq!(
            base_build.imports.get("requirements").unwrap(),
            &Utf8UnixPathBuf::from("./requirements.txt")
        );
        assert_eq!(
            base_build.exports.get("packages").unwrap(),
            &Utf8UnixPathBuf::from("/build/dist/packages")
        );

        // 测试 sandboxes
        let sandboxes = &config.sandboxes;
        let api = sandboxes.get("api").unwrap();
        assert_eq!(api.version.as_ref().unwrap().to_string(), "1.0.0");
        assert_eq!(api.memory.unwrap(), 1024);
        assert_eq!(api.cpus.unwrap(), 1);
        assert_eq!(api.depends_on, vec!["database", "cache"]);
        assert_eq!(api.scope, NetworkScope::Public);
    }

    #[test]
    fn test_microsandbox_config_build_dependencies() {
        let yaml = r#"
            builds:
              base:
                image: "python:3.11-slim"
                depends_on: ["deps"]
              deps:
                image: "python:3.11-slim"
                steps:
                  - "pip install -r requirements.txt"
        "#;

        let config: Microsandbox = serde_yaml::from_str(yaml).unwrap();
        let builds = &config.builds;

        let base = builds.get("base").unwrap();
        assert_eq!(base.depends_on, vec!["deps"]);

        let deps = builds.get("deps").unwrap();
        assert_eq!(
            deps.steps.get(0).unwrap(),
            "pip install -r requirements.txt"
        );
    }

    #[test]
    fn test_microsandbox_config_invalid_configurations() {
        // 测试无效的 scope
        let yaml = r#"
            sandboxes:
              test:
                image: "alpine:latest"
                shell: "/bin/sh"
                scope: "invalid"
        "#;
        assert!(serde_yaml::from_str::<Microsandbox>(yaml).is_err());

        // 测试无效的版本
        let yaml = r#"
            sandboxes:
              test:
                image: "alpine:latest"
                shell: "/bin/sh"
                version: "invalid"
        "#;
        assert!(serde_yaml::from_str::<Microsandbox>(yaml).is_err());
    }
}
