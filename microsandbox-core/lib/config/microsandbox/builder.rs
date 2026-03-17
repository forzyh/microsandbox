//! Microsandbox 配置构建器
//!
//! 本模块提供了用于构建 Microsandbox 和 Sandbox 配置的构建器类型。
//! 使用构建器模式可以更方便地创建复杂的配置。

use std::collections::HashMap;

use microsandbox_utils::DEFAULT_SHELL;
use semver::Version;
use typed_path::Utf8UnixPathBuf;

use crate::{
    MicrosandboxResult,
    config::{EnvPair, PathPair, PortPair, ReferenceOrPath},
};

use super::{Build, Meta, Microsandbox, Module, NetworkScope, Sandbox};

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// Microsandbox 配置构建器
///
/// 使用构建器模式可以方便地创建 Microsandbox 配置，
/// 支持链式调用和渐进式配置。
///
/// ## 可选字段
/// * `meta` - 配置元数据
/// * `modules` - 要导入的模块
/// * `builds` - 要运行的构建任务
/// * `sandboxes` - 要运行的沙箱
///
/// ## 使用示例
/// ```
/// use microsandbox_core::config::{MicrosandboxBuilder, Sandbox, Meta};
///
/// let config = MicrosandboxBuilder::default()
///     .meta(Meta::builder().description("示例配置".to_string()).build())
///     .sandboxes(vec![("web".to_string(), sandbox)])
///     .build()
///     .unwrap();
/// ```
#[derive(Default)]
pub struct MicrosandboxBuilder {
    /// 配置元数据
    meta: Option<Meta>,
    /// 要导入的模块
    modules: HashMap<String, Module>,
    /// 要运行的构建任务
    builds: HashMap<String, Build>,
    /// 要运行的沙箱
    sandboxes: HashMap<String, Sandbox>,
}

/// Sandbox 配置构建器
///
/// 用于构建 Sandbox 配置的构建器类型。
/// 使用泛型参数 `I` 来表示镜像类型的状态，
/// 确保在构建前必须设置镜像。
///
/// ## 必填字段
/// * `image` - 要使用的镜像（OCI 引用或本地 rootfs 路径）
///
/// ## 可选字段
/// * `version` - 沙箱版本
/// * `meta` - 沙箱元数据
/// * `memory` - 沙箱最大内存（MiB）
/// * `cpus` - 沙箱最大 CPU 核心数
/// * `volumes` - 要挂载的卷
/// * `ports` - 要暴露的端口
/// * `envs` - 环境变量
/// * `env_file` - 环境变量文件
/// * `depends_on` - 依赖的沙箱列表
/// * `workdir` - 工作目录
/// * `shell` - 使用的 shell
/// * `scripts` - 沙箱中可用的脚本
/// * `imports` - 要导入的文件
/// * `exports` - 要导出的文件
/// * `scope` - 网络范围
/// * `proxy` - 使用的代理
pub struct SandboxBuilder<I> {
    /// 沙箱版本
    version: Option<Version>,
    /// 沙箱元数据
    meta: Option<Meta>,
    /// 要使用的镜像
    image: I,
    /// 最大内存（MiB）
    memory: Option<u32>,
    /// 最大 CPU 核心数
    cpus: Option<u8>,
    /// 要挂载的卷
    volumes: Vec<PathPair>,
    /// 要暴露的端口
    ports: Vec<PortPair>,
    /// 环境变量
    envs: Vec<EnvPair>,
    /// 环境变量文件路径
    env_file: Option<Utf8UnixPathBuf>,
    /// 依赖的沙箱列表
    depends_on: Vec<String>,
    /// 工作目录
    workdir: Option<Utf8UnixPathBuf>,
    /// 使用的 shell
    shell: Option<String>,
    /// 沙箱中可用的脚本
    scripts: HashMap<String, String>,
    /// 默认命令
    command: Vec<String>,
    /// 要导入的文件
    imports: HashMap<String, Utf8UnixPathBuf>,
    /// 要导出的文件
    exports: HashMap<String, Utf8UnixPathBuf>,
    /// 网络范围
    scope: NetworkScope,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl MicrosandboxBuilder {
    /// 设置配置元数据
    ///
    /// ## 参数
    /// * `meta` - 配置元数据
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn meta(mut self, meta: Meta) -> Self {
        self.meta = Some(meta);
        self
    }

    /// 设置要导入的模块
    ///
    /// ## 参数
    /// * `modules` - 模块映射（路径 -> 模块配置）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn modules(mut self, modules: impl IntoIterator<Item = (String, Module)>) -> Self {
        self.modules = modules.into_iter().collect();
        self
    }

    /// 设置要运行的构建任务
    ///
    /// ## 参数
    /// * `builds` - 构建任务映射（名称 -> 构建配置）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn builds(mut self, builds: impl IntoIterator<Item = (String, Build)>) -> Self {
        self.builds = builds.into_iter().collect();
        self
    }

    /// 设置要运行的沙箱
    ///
    /// ## 参数
    /// * `sandboxes` - 沙箱映射（名称 -> 沙箱配置）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn sandboxes(mut self, sandboxes: impl IntoIterator<Item = (String, Sandbox)>) -> Self {
        self.sandboxes = sandboxes.into_iter().collect();
        self
    }

    /// 构建并验证 Microsandbox 配置
    ///
    /// 此方法会先构建配置，然后调用 `validate()` 进行验证。
    ///
    /// ## 返回值
    /// * `Ok(Microsandbox)` - 构建和验证成功
    /// * `Err(MicrosandboxError)` - 验证失败
    pub fn build(self) -> MicrosandboxResult<Microsandbox> {
        let microsandbox = self.build_unchecked();
        microsandbox.validate()?;
        Ok(microsandbox)
    }

    /// 构建 Microsandbox 配置（不进行验证）
    ///
    /// 此方法直接构建配置，不进行验证。
    /// 适用于已经在其他地方验证过的配置。
    ///
    /// ## 返回值
    /// 构建的 Microsandbox 配置
    pub fn build_unchecked(self) -> Microsandbox {
        Microsandbox {
            meta: self.meta,
            modules: self.modules,
            builds: self.builds,
            sandboxes: self.sandboxes,
        }
    }
}

impl<I> SandboxBuilder<I> {
    /// 设置沙箱版本
    ///
    /// ## 参数
    /// * `version` - 语义化版本号
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn version(mut self, version: impl Into<Version>) -> SandboxBuilder<I> {
        self.version = Some(version.into());
        self
    }

    /// 设置沙箱元数据
    ///
    /// ## 参数
    /// * `meta` - 沙箱元数据
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn meta(mut self, meta: Meta) -> SandboxBuilder<I> {
        self.meta = Some(meta);
        self
    }

    /// 设置沙箱镜像
    ///
    /// 此方法会消耗当前的构建器并返回一个新的 `SandboxBuilder<ReferenceOrPath>`，
    /// 因为镜像是必填字段，设置后类型状态发生变化。
    ///
    /// ## 参数
    /// * `image` - 镜像（OCI 引用或本地 rootfs 路径）
    ///
    /// ## 返回值
    /// 返回设置了镜像的新构建器
    pub fn image(self, image: impl Into<ReferenceOrPath>) -> SandboxBuilder<ReferenceOrPath> {
        SandboxBuilder {
            version: self.version,
            meta: self.meta,
            image: image.into(),
            memory: self.memory,
            cpus: self.cpus,
            volumes: self.volumes,
            ports: self.ports,
            envs: self.envs,
            env_file: self.env_file,
            depends_on: self.depends_on,
            workdir: self.workdir,
            shell: self.shell,
            scripts: self.scripts,
            command: self.command,
            imports: self.imports,
            exports: self.exports,
            scope: self.scope,
        }
    }

    /// 设置沙箱最大内存
    ///
    /// ## 参数
    /// * `memory` - 内存大小（MiB）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn memory(mut self, memory: u32) -> SandboxBuilder<I> {
        self.memory = Some(memory);
        self
    }

    /// 设置沙箱最大 CPU 核心数
    ///
    /// ## 参数
    /// * `cpus` - CPU 核心数
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn cpus(mut self, cpus: u8) -> SandboxBuilder<I> {
        self.cpus = Some(cpus);
        self
    }

    /// 设置要挂载的卷
    ///
    /// ## 参数
    /// * `volumes` - 卷列表（PathPair 集合）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn volumes(mut self, volumes: impl IntoIterator<Item = PathPair>) -> SandboxBuilder<I> {
        self.volumes = volumes.into_iter().collect();
        self
    }

    /// 设置要暴露的端口
    ///
    /// ## 参数
    /// * `ports` - 端口列表（PortPair 集合）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn ports(mut self, ports: impl IntoIterator<Item = PortPair>) -> SandboxBuilder<I> {
        self.ports = ports.into_iter().collect();
        self
    }

    /// 设置环境变量
    ///
    /// ## 参数
    /// * `envs` - 环境变量列表（EnvPair 集合）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn envs(mut self, envs: impl IntoIterator<Item = EnvPair>) -> SandboxBuilder<I> {
        self.envs = envs.into_iter().collect();
        self
    }

    /// 设置环境变量文件
    ///
    /// ## 参数
    /// * `env_file` - 环境变量文件路径
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn env_file(mut self, env_file: impl Into<Utf8UnixPathBuf>) -> SandboxBuilder<I> {
        self.env_file = Some(env_file.into());
        self
    }

    /// 设置依赖的沙箱
    ///
    /// ## 参数
    /// * `depends_on` - 依赖的沙箱名称列表
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn depends_on(mut self, depends_on: impl IntoIterator<Item = String>) -> SandboxBuilder<I> {
        self.depends_on = depends_on.into_iter().collect();
        self
    }

    /// 设置工作目录
    ///
    /// ## 参数
    /// * `workdir` - 工作目录路径
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn workdir(mut self, workdir: impl Into<Utf8UnixPathBuf>) -> SandboxBuilder<I> {
        self.workdir = Some(workdir.into());
        self
    }

    /// 设置 shell
    ///
    /// ## 参数
    /// * `shell` - shell 路径（如 "/bin/bash" 或 "/bin/sh"）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn shell(mut self, shell: impl AsRef<str>) -> SandboxBuilder<I> {
        self.shell = Some(shell.as_ref().to_string());
        self
    }

    /// 设置脚本
    ///
    /// ## 参数
    /// * `scripts` - 脚本映射（名称 -> 脚本内容）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn scripts(
        mut self,
        scripts: impl IntoIterator<Item = (String, String)>,
    ) -> SandboxBuilder<I> {
        self.scripts = scripts.into_iter().collect();
        self
    }

    /// 设置默认命令
    ///
    /// ## 参数
    /// * `command` - 命令参数列表
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn command(mut self, command: impl IntoIterator<Item = String>) -> SandboxBuilder<I> {
        self.command = command.into_iter().collect();
        self
    }

    /// 设置要导入的文件
    ///
    /// ## 参数
    /// * `imports` - 文件映射（名称 -> 路径）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn imports(
        mut self,
        imports: impl IntoIterator<Item = (String, Utf8UnixPathBuf)>,
    ) -> SandboxBuilder<I> {
        self.imports = imports.into_iter().collect();
        self
    }

    /// 设置要导出的文件
    ///
    /// ## 参数
    /// * `exports` - 文件映射（名称 -> 路径）
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn exports(
        mut self,
        exports: impl IntoIterator<Item = (String, Utf8UnixPathBuf)>,
    ) -> SandboxBuilder<I> {
        self.exports = exports.into_iter().collect();
        self
    }

    /// 设置网络范围
    ///
    /// ## 参数
    /// * `scope` - 网络范围配置
    ///
    /// ## 返回值
    /// 返回自引用以支持链式调用
    pub fn scope(mut self, scope: NetworkScope) -> SandboxBuilder<I> {
        self.scope = scope;
        self
    }
}

impl SandboxBuilder<ReferenceOrPath> {
    /// 构建 Sandbox 实例
    ///
    /// 此方法只在已经设置镜像后可用（通过类型状态保证）。
    ///
    /// ## 返回值
    /// 构建的 Sandbox 实例
    pub fn build(self) -> Sandbox {
        Sandbox {
            version: self.version,
            meta: self.meta,
            image: self.image,
            memory: self.memory,
            cpus: self.cpus,
            volumes: self.volumes,
            ports: self.ports,
            envs: self.envs,
            depends_on: self.depends_on,
            workdir: self.workdir,
            shell: self.shell,
            scripts: self.scripts,
            command: self.command,
            imports: self.imports,
            exports: self.exports,
            scope: self.scope,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl Default for SandboxBuilder<()> {
    /// 创建默认的 SandboxBuilder
    ///
    /// 默认值：
    /// - `shell` - 使用默认 shell（通常是 "/bin/sh"）
    /// - `scope` - 使用默认网络范围（Public）
    /// - 所有集合字段为空
    /// - 所有可选字段为 None
    fn default() -> Self {
        Self {
            version: None,
            meta: None,
            image: (),
            memory: None,
            cpus: None,
            volumes: Vec::new(),
            ports: Vec::new(),
            envs: Vec::new(),
            env_file: None,
            depends_on: Vec::new(),
            workdir: None,
            shell: Some(DEFAULT_SHELL.to_string()),
            scripts: HashMap::new(),
            command: Vec::new(),
            imports: HashMap::new(),
            exports: HashMap::new(),
            scope: NetworkScope::default(),
        }
    }
}
