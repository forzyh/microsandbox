# Microsandbox 架构设计文档

> 文档版本：0.2.6
> 最后更新：2026-03-18
> 作者：Microsandbox Team

---

## 目录

- [1. 概述](#1-概述)
  - [1.1 项目简介](#11-项目简介)
  - [1.2 设计目标](#12-设计目标)
  - [1.3 核心特性](#13-核心特性)
- [2. 整体架构](#2-整体架构)
  - [2.1 系统架构图](#21-系统架构图)
  - [2.2 数据流图](#22-数据流图)
- [3. 项目结构](#3-项目结构)
  - [3.1 Workspace 组织](#31-workspace-组织)
  - [3.2 模块依赖关系](#32-模块依赖关系)
- [4. 核心模块详解](#4-核心模块详解)
  - [4.1 microsandbox-core](#41-microsandbox-core)
  - [4.2 microsandbox-server](#42-microsandbox-server)
  - [4.3 microsandbox-portal](#43-microsandbox-portal)
  - [4.4 microsandbox-cli](#44-microsandbox-cli)
  - [4.5 microsandbox-utils](#45-microsandbox-utils)
- [5. 关键技术设计](#5-关键技术设计)
  - [5.1 MicroVM 虚拟化](#51-microvm-虚拟化)
  - [5.2 OCI 镜像处理](#52-oci-镜像处理)
  - [5.3 OverlayFS 联合文件系统](#53-overlayfs-联合文件系统)
  - [5.4 进程监督者模式](#54-进程监督者模式)
  - [5.5 JSON-RPC 通信协议](#55-json-rpc-通信协议)
  - [5.6 MCP 协议集成](#56-mcp-协议集成)
- [6. 数据持久化](#6-数据持久化)
  - [6.1 数据库设计](#61-数据库设计)
  - [6.2 文件系统组织](#62-文件系统组织)
- [7. 安全设计](#7-安全设计)
  - [7.1 隔离机制](#71-隔离机制)
  - [7.2 认证授权](#72-认证授权)
  - [7.3 资源限制](#73-资源限制)
- [8. 性能优化](#8-性能优化)
- [9. 扩展性设计](#9-扩展性设计)
- [10. 总结](#10-总结)

---

## 1. 概述

### 1.1 项目简介

Microsandbox 是一个基于 MicroVM（微虚拟机）技术的沙箱系统，旨在为 AI 工作负载提供安全、隔离的执行环境。项目采用 Rust 语言编写，利用 libkrun 虚拟化库实现轻量级虚拟机，同时保持与 OCI（Open Container Initiative）标准的兼容性。

### 1.2 设计目标

1. **安全隔离**：提供真正的 VM 级别隔离，比传统容器更安全
2. **快速启动**：毫秒级 VM 配置速度，接近容器的启动体验
3. **容器兼容**：支持标准 OCI/Docker 镜像，复用现有生态
4. **资源控制**：细粒度的 CPU、内存、网络资源管理
5. **简单 API**：RESTful 和 JSON-RPC 双接口设计
6. **状态持久**：基于 SQLite 的状态管理，重启后状态不丢失

### 1.3 核心特性

- 基于 libkrun 的轻量级虚拟化
- OCI 镜像拉取和层处理
- OverlayFS 联合文件系统支持
- 多沙箱编排和管理
- HTTP API 和 MCP 协议支持
- Python/Node.js REPL 执行引擎
- 进程监督和日志轮转
- JWT 令牌认证

---

## 2. 整体架构

### 2.1 系统架构图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           用户/API 调用者                                  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         CLI / HTTP API                                  │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
│  │   msb CLI       │  │  msbserver HTTP │  │   MCP Protocol          │ │
│  │   命令行工具     │  │  REST/JSON-RPC  │  │   AI 助手集成            │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      microsandbox-core (核心层)                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐ │
│  │ VM 模块      │  │ OCI 模块    │  │ Management  │  │  Runtime 模块   │ │
│  │ MicroVM     │  │ 镜像拉取    │  │ 编排管理    │  │  进程监督       │ │
│  │ libkrun     │  │ 层处理      │  │ 沙箱生命期  │  │  监控指标       │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────────┘ │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                      │
│  │ Config 模块  │  │ Models 模块  │  │ Utils 模块   │                      │
│  │ 配置构建器  │  │ 数据模型    │  │ 工具函数    │                      │
│  │ 环境变量    │  │ SQLite 模式  │  │ 常量定义    │                      │
│  └─────────────┘  └─────────────┘  └─────────────┘                      │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    microsandbox-portal (执行层)                          │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │                    JSON-RPC Handler                              │    │
│  ├─────────────────┬───────────────────────────────────────────────┤    │
│  │ REPL Engine     │  Command Executor                              │    │
│  │ - Python        │  - PTY/管道模式                                │    │
│  │ - Node.js       │  - 超时控制                                    │    │
│  └─────────────────┴───────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         MicroVM (隔离环境)                               │
│  ┌─────────────────────────────────────────────────────────────────┐    │
│  │  Guest Linux Kernel (libkrun)                                    │    │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │    │
│  │  │ Portal Svc  │  │ User Code   │  │  Resource Limits        │  │    │
│  │  │ :8080       │  │ Python/Node │  │  CPU/Mem/Disk           │  │    │
│  │  └─────────────┘  └─────────────┘  └─────────────────────────┘  │    │
│  └─────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 数据流图

```
用户请求
    │
    ▼
┌──────────────────┐
│  HTTP/JSON-RPC   │
│  请求接收        │
└──────────────────┘
    │
    ▼
┌──────────────────┐
│  Middleware      │───────► 日志记录
│  - Auth          │───────► JWT 验证
│  - Logging       │───────► MCP 认证
└──────────────────┘
    │
    ▼
┌──────────────────┐
│  Handler         │
│  - 路由分发      │
│  - 参数解析      │
└──────────────────┘
    │
    ├──────────────────┬──────────────────┬──────────────────┐
    ▼                  ▼                  ▼                  ▼
┌─────────┐      ┌─────────┐        ┌─────────┐       ┌─────────┐
│ start() │      │  stop() │        │ metrics │       │  repl   │
└─────────┘      └─────────┘        └─────────┘       └─────────┘
    │                  │                  │                 │
    ▼                  ▼                  ▼                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Orchestra 编排层                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ Sandbox.start()│ │ Sandbox.stop()│ │ DB 状态更新            │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────────────────────────────┐
│                      VM 生命周期管理                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │ MicroVm     │  │ Rootfs      │  │  PortManager            │  │
│  │ libkrun_run │  │ OverlayFS   │  │  端口分配               │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
    │
    ▼
┌──────────────────┐
│  Supervisor      │───────► 监控进程状态
│  - spawn()       │───────► 捕获 stdout/stderr
│  - wait()        │───────► 日志轮转
└──────────────────┘
```

---

## 3. 项目结构

### 3.1 Workspace 组织

Microsandbox 采用 Cargo Workspace 组织，包含以下成员：

```
microsandbox/
├── Cargo.toml                    # Workspace 根配置
├── microsandbox-cli/             # 命令行接口模块
│   ├── Cargo.toml
│   ├── lib/                      # 库代码
│   │   ├── lib.rs               # 库入口
│   │   ├── args/                # 参数定义
│   │   │   ├── mod.rs
│   │   │   ├── msb.rs           # msb 命令参数
│   │   │   ├── msbrun.rs        # msbrun 参数
│   │   │   └── msbserver.rs     # 服务器参数
│   │   ├── error.rs             # 错误类型
│   │   └── styles.rs            # 终端样式
│   └── bin/                     # 二进制入口
│       ├── msb/                 # msb 命令
│       │   ├── mod.rs
│       │   └── main.rs
│       ├── msbrun.rs            # msbrun 入口
│       └── msbserver.rs         # 服务器入口
│
├── microsandbox-core/           # 核心功能模块
│   ├── Cargo.toml
│   └── lib/
│       ├── lib.rs               # 库入口
│       ├── error.rs             # 核心错误类型
│       ├── config/              # 配置模块
│       │   ├── mod.rs
│       │   ├── env_pair.rs      # 环境变量对
│       │   ├── path_pair.rs     # 路径映射
│       │   ├── port_pair.rs     # 端口映射
│       │   ├── reference_path.rs# OCI 引用或路径
│       │   ├── microsandbox/    # Microsandbox 配置
│       │   │   ├── mod.rs
│       │   │   └── builder.rs   # 配置构建器
│       ├── vm/                  # 虚拟机模块
│       │   ├── mod.rs
│       │   ├── microvm.rs       # MicroVM 实现
│       │   ├── errors.rs        # VM 错误
│       │   └── ffi.rs           # libkrun FFI 绑定
│       ├── oci/                 # OCI 模块
│       │   ├── mod.rs
│       │   ├── image.rs         # 镜像处理
│       │   ├── layer.rs         # 层处理
│       │   ├── reference.rs     # 引用解析
│       │   ├── registry.rs      # 注册表交互
│       │   └── global_cache.rs  # 全局缓存
│       ├── management/          # 编排管理
│       │   ├── mod.rs
│       │   ├── orchestra.rs     # 沙箱编排
│       │   ├── sandbox.rs       # 沙箱创建
│       │   ├── db.rs            # 数据库操作
│       │   ├── config.rs        # 配置管理
│       │   ├── menv.rs          # 环境管理
│       │   ├── rootfs.rs        # 根文件系统
│       │   ├── home.rs          # 全局目录管理
│       │   └── toolchain.rs     # 工具链管理
│       ├── runtime/             # 运行时模块
│       │   ├── mod.rs
│       │   └── monitor.rs       # 进程监控
│       ├── models.rs            # 数据模型
│       └── utils/               # 工具函数
│           └── mod.rs
│
├── microsandbox-server/         # HTTP 服务器模块
│   ├── Cargo.toml
│   └── lib/
│       ├── lib.rs               # 库入口
│       ├── config.rs            # 服务器配置
│       ├── error.rs             # 错误处理
│       ├── handler.rs           # 请求处理器
│       ├── management.rs        # 生命周期管理
│       ├── mcp.rs               # MCP 协议
│       ├── middleware.rs        # 中间件
│       ├── payload.rs           # 数据结构
│       ├── port.rs              # 端口管理
│       ├── route.rs             # 路由配置
│       └── state.rs             # 状态管理
│
├── microsandbox-portal/         # Portal 执行模块
│   ├── Cargo.toml
│   └── lib/
│       ├── lib.rs               # 库入口
│       ├── error.rs             # 错误类型
│       ├── handler.rs           # 请求处理
│       ├── payload.rs           # JSON-RPC 结构
│       ├── portal/              # 门户功能
│       │   ├── mod.rs
│       │   ├── repl/            # REPL 引擎
│       │   │   ├── engine.rs
│       │   │   ├── types.rs
│       │   │   ├── python.rs
│       │   │   └── nodejs.rs
│       │   ├── command.rs       # 命令执行
│       │   └── fs.rs            # 文件系统
│       ├── route.rs             # 路由
│       └── state.rs             # 状态
│
├── microsandbox-utils/          # 工具函数模块
│   ├── Cargo.toml
│   └── lib/
│       ├── lib.rs
│       ├── log/
│       │   └── rotating.rs      # 日志轮转
│       └── runtime/
│           ├── monitor.rs       # 监控 trait
│           └── supervisor.rs    # 监督者实现
│
└── sdk/rust/                    # Rust SDK（独立）
    └── Cargo.toml
```

### 3.2 模块依赖关系

```
                    ┌─────────────────┐
                    │  microsandbox-cli│
                    └────────┬────────┘
                             │ depends on
              ┌──────────────┼──────────────┐
              │              │              │
              ▼              ▼              ▼
    ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐
    │microsandbox-│  │microsandbox-│  │ microsandbox-   │
    │   server    │  │   portal    │  │   utils         │
    └──────┬──────┘  └──────┬──────┘  └─────────────────┘
           │                │
           └────────┬───────┘
                    │
                    ▼
           ┌─────────────────┐
           │ microsandbox-core│
           └─────────────────┘

依赖方向：cli/server/portal → core → utils
```

---

## 4. 核心模块详解

### 4.1 microsandbox-core

**职责**：提供沙箱系统的核心功能，包括 VM 管理、OCI 镜像处理、编排协调等。

#### 4.1.1 VM 模块设计

**文件**：`microsandbox-core/lib/vm/microvm.rs`

`MicroVm` 结构是虚拟机的核心抽象：

```rust
pub struct MicroVm {
    ctx_id: u32,              // libkrun 上下文 ID
    config: MicroVmConfig,    // VM 配置
}
```

**设计要点**：

1. **Builder 模式**：使用 `MicroVmBuilder` 逐步构建配置
2. **Rootfs 抽象**：支持 `Native` 和 `Overlayfs` 两种根文件系统
3. **配置验证**：构建时验证配置合法性
4. **FFI 封装**：通过 `ffi.rs` 封装 libkrun C API

**启动流程**：

```
MicroVm::builder()
    .rootfs(Rootfs::Overlayfs(layers))
    .memory_mib(1024)
    .num_vcpus(2)
    .mapped_dirs("/app", host_path)
    .port_map(8080, guest_port)
    .exec_path("/start.sh")
    .build()
    └──► MicroVm::start()  ─────► libkrun_run(ctx_id)
```

#### 4.1.2 OCI 模块设计

**文件**：`microsandbox-core/lib/oci/`

OCI 模块负责与容器注册表交互，核心流程：

```
Image::pull(reference)
    │
    ▼
Registry::pull_image()
    │
    ├──► 解析镜像引用 (Reference)
    │
    ├──► 获取 Manifest
    │
    ├──► 获取 Config
    │
    ├──► 并行下载 Layers
    │
    ├──► 解压 Layers (flate2 + tar)
    │
    └──► 提取到本地目录
```

**关键类型**：

- `Image`: 镜像 bundles，包含有序的层列表
- `LayerOps`: 层操作 trait，定义提取和清理接口
- `Registry`: 注册表客户端，处理认证和请求
- `GlobalCache`: 全局缓存，管理下载和提取的层

#### 4.1.3 Management 模块设计

**文件**：`microsandbox-core/lib/management/orchestra.rs`

编排模块提供 `up`、`down`、`apply` 三个核心操作：

| 操作 | 功能 | 实现逻辑 |
|------|------|----------|
| `up()` | 启动所有沙箱 | 读取配置 → 创建沙箱 → 启动 VM |
| `down()` | 停止所有沙箱 | 查询 DB → 发送 SIGTERM → 清理 |
| `apply()` | 应用配置变更 | 对比配置和 DB → 启动新增 → 停止删除 |

**SandboxStatus** 结构记录沙箱状态：

```rust
pub struct SandboxStatus {
    pub name: String,
    pub running: bool,
    pub supervisor_pid: Option<u32>,
    pub microvm_pid: Option<u32>,
    pub cpu_usage: Option<f32>,
    pub memory_usage: Option<u64>,
    pub disk_usage: Option<u64>,
}
```

### 4.2 microsandbox-server

**职责**：提供 HTTP API 服务，管理和控制沙箱环境。

#### 4.2.1 架构设计

```
HTTP 请求
    │
    ▼
┌─────────────────────────────────────────┐
│ Middleware Layer                        │
│ ┌─────────────────────────────────────┐ │
│ │ logging_middleware                  │ │
│ │ - 记录请求方法、路径、耗时          │ │
│ │ - 记录响应状态码                    │ │
│ └─────────────────────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │ auth_middleware                     │ │
│ │ - 提取 Authorization header         │ │
│ │ - 验证 JWT 令牌                      │ │
│ │ - 开发模式跳过认证                  │ │
│ └─────────────────────────────────────┘ │
│ ┌─────────────────────────────────────┐ │
│ │ mcp_smart_auth_middleware           │ │
│ │ - MCP 请求智能认证                   │ │
│ │ - 根据 token 权限控制访问            │ │
│ └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│ Router                                  │
│ /api/v1/health    → health()            │
│ /api/v1/rpc       → json_rpc_handler()  │
│ /mcp              → mcp_handler()       │
│ /proxy/*          → proxy_handler()     │
└─────────────────────────────────────────┘
```

#### 4.2.2 JSON-RPC 方法

服务器支持以下 JSON-RPC 方法：

```json
{
  "jsonrpc": "2.0",
  "method": "sandbox.start",
  "params": {
    "name": "my-sandbox",
    "config": {...}
  },
  "id": 1
}
```

| 方法 | 参数 | 返回值 | 说明 |
|------|------|--------|------|
| `sandbox.start` | `SandboxStartParams` | `SandboxStartResponse` | 启动沙箱 |
| `sandbox.stop` | `SandboxStopParams` | `SandboxStopResponse` | 停止沙箱 |
| `sandbox.metrics.get` | `SandboxMetricsParams` | `SandboxMetricsResponse` | 获取指标 |
| `sandbox.repl.run` | `ReplRunParams` | `ReplRunResponse` | 执行代码 |
| `sandbox.command.run` | `CommandRunParams` | `CommandRunResponse` | 执行命令 |

#### 4.2.3 认证机制

**JWT 令牌结构**：

```rust
pub struct ApiTokenClaims {
    pub sub: String,      // 主题（通常是用户 ID）
    pub exp: usize,       // 过期时间
    pub iat: usize,       // 签发时间
}
```

**令牌格式**：`msb_<jwt_token>`

- 使用 HMAC-SHA256 签名
- 默认 24 小时过期
- 支持 `--dev` 模式跳过认证

### 4.3 microsandbox-portal

**职责**：在沙箱内提供 JSON-RPC 接口，执行代码和命令。

#### 4.3.1 REPL 引擎设计

**文件**：`microsandbox-portal/lib/portal/repl/`

REPL 引擎支持多种语言：

```rust
pub enum Language {
    Python,
    NodeJs,
}

pub trait ReplEngine: Send + Sync {
    async fn eval(
        &self,
        code: &str,
        session_id: &str,
        timeout_secs: Option<u64>,
    ) -> PortalResult<ReplResult>;
}
```

**Python 实现**：

```rust
// 使用 python3 -u -i 启动交互式解释器
// -u: 非缓冲模式（实时输出）
// -i: 交互模式
let child = Command::new("python3")
    .args(["-u", "-i"])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
```

**Node.js 实现**：

```rust
// 使用 node --interactive
let child = Command::new("node")
    .arg("--interactive")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
```

#### 4.3.2 命令执行设计

**PTP/管道模式**：

```rust
// PTY 模式：适用于交互式命令
let pty = Pty::open()?;
let handle = CommandHandle {
    mode: ExecutionMode::Pty { pty, reader },
    // ...
};

// 管道模式：适用于批处理命令
let child = Command::new(cmd)
    .args(args)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
```

### 4.4 microsandbox-cli

**职责**：提供命令行接口，包括 `msb`、`msbrun`、`msbserver` 三个命令。

#### 4.4.1 命令结构

```rust
#[derive(Parser)]
pub enum MicrosandboxArgs {
    /// msb - 主命令行工具
    Msb { subcommand: MsbCommands },

    /// msbrun - 快速运行代码
    Msbrun(MsbrunArgs),

    /// msbserver - 启动服务器
    Msbserver(MsbserverArgs),
}
```

**msb 子命令**：

| 命令 | 功能 |
|------|------|
| `msb up` | 启动所有沙箱 |
| `msb down` | 停止所有沙箱 |
| `msb apply` | 应用配置变更 |
| `msb sandbox create` | 创建沙箱 |
| `msb sandbox stop` | 停止沙箱 |
| `msb image pull` | 拉取镜像 |

#### 4.4.2 参数解析

使用 `clap` crate 的 derive 功能：

```rust
#[derive(Parser)]
#[command(name = "msb")]
pub struct MsbArgs {
    #[command(subcommand)]
    pub subcommand: MsbCommands,
}

#[derive(Subcommand)]
pub enum MsbCommands {
    Up(UpArgs),
    Down(DownArgs),
    Apply(ApplyArgs),
    // ...
}
```

### 4.5 microsandbox-utils

**职责**：提供通用工具函数和辅助代码。

#### 4.5.1 日志轮转

**文件**：`microsandbox-utils/lib/log/rotating.rs`

实现异步日志轮转：

```rust
pub struct RotatingLog {
    max_size_bytes: u64,
    max_files: u32,
    // ...
}

impl RotatingLog {
    pub async fn rotate(&mut self) -> Result<()> {
        // 当日志达到 max_size_bytes 时
        // 重命名为 .1, .2, ...
        // 删除超过 max_files 的旧日志
    }
}
```

#### 4.5.2 进程监督者

**文件**：`microsandbox-utils/lib/runtime/supervisor.rs`

监督者模式管理子进程：

```rust
pub struct Supervisor {
    process: Child,
    output_mode: OutputMode,
    // ...
}

impl Supervisor {
    pub fn spawn(&mut self) -> Result<()> {
        // 启动进程
        // 捕获 stdout/stderr
        // 异步转发到日志
    }

    pub fn wait(&mut self) -> Result<ExitStatus> {
        // 等待进程退出
    }
}
```

---

## 5. 关键技术设计

### 5.1 MicroVM 虚拟化

#### 5.1.1 libkrun 集成

libkrun 是一个轻量级虚拟化库，基于 KVM 实现快速 VM 启动。

**FFI 绑定**（`vm/ffi.rs`）：

```rust
extern "C" {
    pub fn krun_create_ctx() -> i32;
    pub fn krun_start_enter(ctx_id: u32) -> i32;
    pub fn krun_set_vm_config(ctx_id: u32, num_vcpus: u32, memory_mib: u32) -> i32;
    pub fn krun_set_root_dir(ctx_id: u32, path: *const c_char, tag: *const c_char) -> i32;
    pub fn krun_set_port_map(ctx_id: u32, guest_port: u32, host_port: u32) -> i32;
    // ...
}
```

**启动流程**：

```
1. krun_create_ctx()          → 创建上下文，返回 ctx_id
2. krun_set_vm_config()       → 设置 vCPU 和内存
3. krun_set_root_dir()        → 设置根文件系统
4. krun_set_port_map()        → 配置端口转发
5. krun_set_mapped_dirs()     → 配置目录挂载
6. krun_start_enter()         → 启动 VM（阻塞调用）
```

#### 5.1.2 虚拟化隔离

MicroVM 提供以下隔离：

| 隔离级别 | 实现机制 |
|----------|----------|
| 进程隔离 | 独立 PID 命名空间 |
| 文件系统 | 独立根文件系统 |
| 网络 | 独立网络命名空间 + NAT |
| 资源 | cgroups v2 限制 |

### 5.2 OCI 镜像处理

#### 5.2.1 镜像引用解析

```rust
// docker.io/library/nginx:stable
// ├── registry: docker.io
// ├── namespace: library
// ├── name: nginx
// └── tag: stable

// us-docker.pkg.dev/google-samples/containers/gke/hello-app:1.0
// ├── registry: us-docker.pkg.dev
// ├── namespace: google-samples/containers/gke
// ├── name: hello-app
// └── tag: 1.0
```

**解析逻辑**：

```rust
pub fn parse(image_ref: &str) -> Result<Reference> {
    // 1. 检查是否有 registry（包含.或:）
    // 2. 提取 namespace（最后一个/之前的部分）
    // 3. 提取 name:tag 或 name@digest
    // 4. 验证格式合法性
}
```

#### 5.2.2 镜像层处理

OCI 镜像由多层组成，每层是一个 gzip 压缩的 tar 包：

```
Image Manifest
├── Layer 0: base.tar.gz (基础系统)
├── Layer 1: runtime.tar.gz (运行时环境)
├── Layer 2: app.tar.gz (应用程序)
└── Config: config.json (运行配置)
```

**提取流程**：

```rust
async fn extract(layer: &Layer, parents: LayerDependencies) -> Result<()> {
    // 1. 创建临时目录
    // 2. 解压 gzip
    // 3. 解包 tar
    // 4. 处理白色文件（overlayfs 删除标记）
    // 5. 移动到最终目录
}
```

### 5.3 OverlayFS 联合文件系统

#### 5.3.1 层叠加原理

OverlayFS 将多个目录合并成一个统一的文件系统视图：

```
┌─────────────────────────────────────┐
│          Upper (RW Layer)           │  ← 可写层（沙箱运行时创建的文件）
├─────────────────────────────────────┤
│          Lower1 (Layer N)           │  ← 只读层（应用层）
├─────────────────────────────────────┤
│          Lower2 (Layer N-1)         │  ← 只读层（运行时层）
├─────────────────────────────────────┤
│          LowerN (Base Layer)        │  ← 只读层（基础系统层）
└─────────────────────────────────────┘
              │
              ▼
        ┌─────────────┐
        │   Merged    │  ← 统一视图（/）
        │   View      │
        └─────────────┘
```

#### 5.3.2 libkrun overlayfs 配置

libkrun 通过 virtio-fs 支持 overlayfs：

```rust
// 配置 overlayfs 根文件系统
let lower_dirs: Vec<String> = layers
    .iter()
    .map(|l| l.extract_path().to_string())
    .collect();

let upper_dir = rw_layer_path.to_string();
let work_dir = work_path.to_string();

// libkrun 要求特殊的 overlay 格式
// overlay:<lower1>:<lower2>:...:<upper>:<work>
let overlay_spec = format!(
    "overlay:{}::{}",
    lower_dirs.join(":"),
    work_dir
);
```

### 5.4 进程监督者模式

#### 5.4.1 Supervisor 设计

监督者负责管理子进程的生命周期：

```rust
pub struct Supervisor {
    process: Child,
    output_mode: OutputMode,
    log_dir: Option<PathBuf>,
    max_log_size: u64,
    max_log_files: u32,
}
```

**状态流转**：

```
Created
    │
    ▼
Spawning ────► 启动子进程
    │
    ▼
Running  ────► 转发输出到日志
    │
    ├────────► 正常退出 (exit_code)
    │
    ▼
Exited   ────► 清理资源
```

#### 5.4.2 日志轮转集成

```rust
// 异步日志转发
async fn forward_output(
    reader: BufReader<ChildStdout>,
    log_file: Arc<Mutex<RotatingLog>>,
) {
    let mut lines = reader.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut log = log_file.lock().await;
        log.write(&line).await?;

        // 检查是否需要轮转
        if log.size() > max_size {
            log.rotate().await?;
        }
    }
}
```

### 5.5 JSON-RPC 通信协议

#### 5.5.1 消息结构

```rust
// 请求
#[derive(Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,  // "2.0"
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

// 响应
#[derive(Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,  // "2.0"
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
    pub id: Option<Value>,
}

// 错误
#[derive(Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}
```

#### 5.5.2 错误码定义

| 错误码 | 含义 |
|--------|------|
| -32700 | Parse error |
| -32600 | Invalid Request |
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |
| 1001   | Authentication failed |
| 1002   | Sandbox not found |
| 1003   | Resource exhausted |

### 5.6 MCP 协议集成

#### 5.6.1 MCP 概述

Model Context Protocol (MCP) 是 Anthropic 定义的协议，允许 AI 助手调用外部工具。

#### 5.6.2 实现的方法

```rust
// MCP initialize 响应
{
  "protocolVersion": "2024-11-05",
  "capabilities": {
    "tools": {},
    "prompts": {}
  },
  "serverInfo": {
    "name": "microsandbox",
    "version": "0.2.6"
  }
}

// 可用的工具
{
  "tools": [
    {"name": "sandbox_start", "description": "启动沙箱"},
    {"name": "sandbox_stop", "description": "停止沙箱"},
    {"name": "sandbox_run_code", "description": "执行代码"},
    {"name": "sandbox_run_command", "description": "执行命令"},
    {"name": "sandbox_get_metrics", "description": "获取指标"}
  ]
}
```

#### 5.6.3 智能认证

MCP 中间件实现智能认证：

```rust
pub async fn mcp_smart_auth_middleware(
    state: State<AppState>,
    request: Request<Bytes>,
    next: Next,
) -> Result<Response> {
    // 1. 检查是否有 Authorization header
    // 2. 验证 JWT 令牌
    // 3. 根据 token 权限决定允许的操作
    // 4. 开发模式跳过认证
}
```

---

## 6. 数据持久化

### 6.1 数据库设计

#### 6.1.1 SQLite Schema

**Sandbox 表**：

```sql
CREATE TABLE IF NOT EXISTS sandboxes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    config_file TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    supervisor_pid INTEGER,
    microvm_pid INTEGER
);

CREATE INDEX idx_sandboxes_name ON sandboxes(name);
CREATE INDEX idx_sandboxes_status ON sandboxes(status);
```

**Image 表**：

```sql
CREATE TABLE IF NOT EXISTS images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    reference TEXT UNIQUE NOT NULL,
    config TEXT NOT NULL,
    manifest TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

**Layer 表**：

```sql
CREATE TABLE IF NOT EXISTS layers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    digest TEXT NOT NULL,
    media_type TEXT NOT NULL,
    size INTEGER NOT NULL,
    urls TEXT,
    extracted_path TEXT,
    UNIQUE(digest)
);
```

#### 6.1.2 数据库迁移

使用 `sqlx` 的 migrator 功能：

```rust
pub static OCI_DB_MIGRATOR: Migrator = sqlx::migrate!("migrations/oci");
pub static SANDBOX_DB_MIGRATOR: Migrator = sqlx::migrate!("migrations/sandbox");

pub async fn get_or_create_pool(
    db_path: &Path,
    migrator: &Migrator,
) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_path.to_str().unwrap())
        .await?;

    migrator.run(&pool).await?;
    Ok(pool)
}
```

### 6.2 文件系统组织

#### 6.2.1 目录结构

```
~/.microsandbox/
├── layers/                 # OCI 镜像层
│   ├── download/           # 下载的压缩层
│   └── extracted/          # 解压后的层
├── sandboxes/              # 沙箱数据
│   ├── <sandbox-name>/
│   │   ├── config.yaml     # 沙箱配置
│   │   ├── rw/             # 可写层
│   │   └── logs/           # 日志文件
│   └── ...
├── dbs/                    # SQLite 数据库
│   ├── oci.db
│   └── sandbox.db
└── ports.json              # 端口分配记录
```

#### 6.2.2 路径常量

```rust
// microsandbox-utils/lib/constants.rs
pub const MICROSANDBOX_HOME: &str = ".microsandbox";
pub const LAYERS_SUBDIR: &str = "layers";
pub const EXTRACTED_LAYER_SUFFIX: &str = "_extracted";
pub const OCI_DB_FILENAME: &str = "oci.db";
pub const SANDBOX_DB_FILENAME: &str = "sandbox.db";
pub const MICROSANDBOX_ENV_DIR: &str = ".microsandbox";
```

---

## 7. 安全设计

### 7.1 隔离机制

#### 7.1.1 VM 级别隔离

与容器相比，MicroVM 提供更强的隔离：

| 特性 | 容器 | MicroVM |
|------|------|---------|
| 内核 | 共享主机内核 | 独立 Guest 内核 |
| 文件系统 | Namespace 隔离 | 完全独立 |
| 进程 | PID Namespace | 完全隔离 |
| 网络 | Network Namespace | 独立 NIC |
| 逃逸风险 | 较高 | 极低 |

#### 7.1.2 资源限制

```rust
// 配置资源限制
let config = MicroVmConfig::builder()
    .num_vcpus(2)           // vCPU 数量
    .memory_mib(1024)       // 内存限制
    .rlimits(vec![
        (RLIMIT_NOFILE, 1024),      // 文件描述符
        (RLIMIT_NPROC, 100),        // 进程数
        (RLIMIT_AS, 1 << 30),       // 地址空间
    ])
    .build();
```

### 7.2 认证授权

#### 7.2.1 JWT 验证流程

```rust
pub fn verify_token(token: &str, secret: &str) -> Result<ApiTokenClaims> {
    let validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.required_spec = RequiredSpec {
        exp: true,
        iat: true,
        sub: true,
    };

    let token_data = decode::<ApiTokenClaims>(token, secret, &validation)?;
    Ok(token_data.claims)
}
```

#### 7.2.2 中间件认证链

```
Request
    │
    ▼
┌─────────────────────────┐
│ logging_middleware      │  ← 记录日志
└───────────┬─────────────┘
            │
            ▼
┌─────────────────────────┐
│ auth_middleware         │  ← 验证 JWT
└───────────┬─────────────┘
            │
            ├──────► 401 Unauthorized (验证失败)
            │
            ▼ (验证通过)
┌─────────────────────────┐
│ mcp_smart_auth          │  ← MCP 智能认证
└───────────┬─────────────┘
            │
            ▼
Handler
```

### 7.3 资源限制

#### 7.3.1 cgroups v2 集成

libkrun 内部使用 cgroups v2 进行资源限制：

```
/sys/fs/cgroup/microsandbox/
├── cpu.max          # CPU 限制
├── memory.max       # 内存限制
├── pids.max         # 进程数限制
└── io.max           # IO 限制
```

#### 7.3.2 磁盘配额

```rust
// 监控磁盘使用
pub fn get_disk_usage(path: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in walkdir::WalkDir::new(path) {
        let entry = entry?;
        if let Ok(meta) = entry.metadata() {
            total += meta.len();
        }
    }
    Ok(total)
}
```

---

## 8. 性能优化

### 8.1 并行镜像拉取

```rust
// 并行下载层
let download_futures = layer_digests.iter().map(|digest| {
    async move {
        registry.download_layer(digest).await
    }
});
let results = future::join_all(download_futures).await;

// 并行提取层
let extraction_futures = layers.iter().map(|layer| {
    async move {
        layer.extract(parent_layers).await
    }
});
future::join_all(extraction_futures).await;
```

### 8.2 层缓存

```rust
// 全局缓存结构
pub struct GlobalCache {
    download_dir: PathBuf,      // 下载缓存
    extract_dir: PathBuf,       // 提取缓存
    db: SqlitePool,             // 元数据缓存
}

// 检查缓存命中
if let Some(cached_path) = cache.get_extracted(digest).await? {
    return Ok(cached_path);  // 直接返回缓存
}
```

### 8.3 磁盘大小缓存

```rust
// TTL 缓存，避免重复计算
static DISK_SIZE_CACHE: Lazy<RwLock<HashMap<String, (u64, Instant)>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const DISK_SIZE_TTL: Duration = Duration::from_secs(30);

pub fn get_cached_size(path: &str) -> Option<u64> {
    let cache = DISK_SIZE_CACHE.read().unwrap();
    if let Some((size, timestamp)) = cache.get(path) {
        if timestamp.elapsed() < DISK_SIZE_TTL {
            return Some(*size);
        }
    }
    None
}
```

---

## 9. 扩展性设计

### 9.1 插件化架构

各模块通过 trait 定义接口，支持自定义实现：

```rust
// 层操作接口
pub trait LayerOps: Send + Sync {
    fn digest(&self) -> &Digest;
    fn media_type(&self) -> &str;
    async fn extract(&self, parents: LayerDependencies) -> Result<()>;
    async fn cleanup_extracted(&self) -> Result<()>;
}

// 运行时监控接口
pub trait RuntimeMonitor: Send + Sync {
    fn get_cpu_usage(&self, pid: u32) -> Result<f32>;
    fn get_memory_usage(&self, pid: u32) -> Result<u64>;
}
```

### 9.2 多语言支持

当前支持 Python 和 Node.js，可通过添加新的 `ReplEngine` 实现扩展：

```rust
// 添加 Go 支持
impl ReplEngine for GoEngine {
    async fn eval(&self, code: &str, ...) -> Result<ReplResult> {
        // 使用 go run 执行代码
    }
}
```

### 9.3 配置扩展

配置系统支持 YAML 格式，易于扩展：

```yaml
# microsandbox.yaml
version: "1.0"

sandboxes:
  python-sandbox:
    image: docker.io/library/python:3.12
    memory: 1024
    vcpus: 2
    ports:
      - "8080:8080"
    volumes:
      - ./code:/app

  node-sandbox:
    image: docker.io/library/node:20
    memory: 512
    vcpus: 1
```

---

## 10. 总结

Microsandbox 是一个功能完整的微虚拟机沙箱系统，具有以下特点：

### 10.1 架构优势

1. **模块化设计**：清晰的模块划分，各司其职
2. **层次分明**：核心层、服务层、执行层职责明确
3. **trait 抽象**：良好的接口设计，支持扩展

### 10.2 技术亮点

1. **libkrun 虚拟化**：快速启动，真正隔离
2. **OCI 兼容**：复用现有容器生态
3. **OverlayFS**：高效的层叠加
4. **监督者模式**：可靠的进程管理
5. **JSON-RPC + MCP**：标准协议，易于集成

### 10.3 安全特性

1. **VM 级别隔离**：比容器更安全
2. **资源限制**：防止资源滥用
3. **JWT 认证**：API 访问控制
4. **日志审计**：完整的操作记录

### 10.4 性能优化

1. **并行处理**：镜像拉取和提取并行
2. **缓存机制**：减少重复操作
3. **异步 IO**：非阻塞设计

---

*本文档涵盖了 Microsandbox 的整体架构设计和核心模块实现细节，旨在帮助开发者理解系统的内部工作原理。*
