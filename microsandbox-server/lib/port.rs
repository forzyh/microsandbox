//! # 端口管理模块 - 沙箱端口分配和管理
//!
//! 本模块负责为沙箱分配和管理网络端口，确保每个沙箱都有唯一的可用端口。
//!
//! ## 端口管理架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        PortManager                              │
//! │  ┌─────────────────────────────────────────────────────────┐    │
//! │  │                   BiPortMapping                          │    │
//! │  │  ┌─────────────────────┐  ┌─────────────────────┐       │    │
//! │  │  │  sandbox_to_port    │  │   port_to_sandbox   │       │    │
//! │  │  │  (沙箱 -> 端口)      │  │   (端口 -> 沙箱)     │       │    │
//! │  │  │  -----------------  │  │  -----------------  │       │    │
//! │  │  │  "sandbox-a" -> 5001│  │  5001 -> "sandbox-a"│       │    │
//! │  │  │  "sandbox-b" -> 5002│  │  5002 -> "sandbox-b"│       │    │
//! │  │  └─────────────────────┘  └─────────────────────┘       │    │
//! │  └─────────────────────────────────────────────────────────┘    │
//! │                              │                                   │
//! │                              ▼                                   │
//! │  ┌─────────────────────────────────────────────────────────┐    │
//! │  │                  portal_ports.json                       │    │
//! │  │  {                                                       │    │
//! │  │    "mappings": {                                         │    │
//! │  │      "sandbox-a": 5001,                                  │    │
//! │  │      "sandbox-b": 5002                                   │    │
//! │  │    }                                                     │    │
//! │  │  }                                                       │    │
//! │  └─────────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 核心特性
//!
//! ### 1. 双向映射 (BiPortMapping)
//! 为了高效查找，使用两个 HashMap 实现双向映射：
//! - `sandbox_to_port`: 通过沙箱名快速获取端口
//! - `port_to_sandbox`: 通过端口快速查找沙箱（用于反向查询）
//!
//! ### 2. OS 端口分配
//! 使用 OS 的端口分配机制确保端口真正可用：
//! ```rust,ignore
//! // 绑定到端口 0，让 OS 分配一个可用端口
//! let addr = SocketAddr::new(LOCALHOST_IP, 0);
//! let listener = TcpListener::bind(addr)?;
//! let port = listener.local_addr()?.port();
//! // listener 被丢弃后，端口释放，但我们可以使用这个端口号
//! ```
//!
//! ### 3. 并发安全
//! 使用互斥锁确保端口分配的原子性：
//! ```rust,ignore
//! static PORT_ASSIGNMENT_LOCK: Lazy<Mutex<()>> = ...;
//! let _lock = PORT_ASSIGNMENT_LOCK.lock().await;
//! // 临界区：获取可用端口并保存映射
//! ```
//!
//! ### 4. 持久化存储
//! 端口映射保存到文件，服务器重启后恢复：
//! - 文件位置：`~/.microsandbox/projects/portal_ports.json`
//! - 启动时加载现有映射
//! - 分配/释放端口后立即保存
//!
//! ## 端口分配流程
//!
//! ```text
//! 1. 检查是否已分配端口
//!    │
//!    ├─ 是 → 验证端口是否仍可用
//!    │        │
//!    │        ├─ 可用 → 返回已分配端口 ✓
//!    │        │
//!    │        └─ 不可用 → 删除旧映射，继续分配新端口
//!    │
//!    └─ 否 → 继续分配新端口
//!              │
//!              ▼
//! 2. 获取端口分配锁 (确保并发安全)
//!              │
//!              ▼
//! 3. 从 OS 获取可用端口 (绑定到端口 0)
//!              │
//!              ▼
//! 4. 保存映射到内存和文件
//!              │
//!              ▼
//! 5. 返回分配的端口号 ✓
//! ```
//!
//! ## 使用示例
//!
//! ```rust,no_run
//! use microsandbox_server::PortManager;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 创建端口管理器
//! let mut manager = PortManager::new("/path/to/project").await?;
//!
//! // 为沙箱分配端口
//! let port = manager.assign_port("my-sandbox").await?;
//! println!("Assigned port {} to my-sandbox", port);
//!
//! // 获取已分配的端口
//! if let Some(port) = manager.get_port("my-sandbox") {
//!     println!("Port for my-sandbox: {}", port);
//! }
//!
//! // 释放端口
//! manager.release_port("my-sandbox").await?;
//! # Ok(())
//! # }
//! ```

use microsandbox_utils::PORTAL_PORTS_FILE;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
};
use tokio::{fs, sync::Mutex};
use tracing::{debug, info, warn};

use crate::{MicrosandboxServerError, MicrosandboxServerResult};

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// 本地回环 IP 地址 (127.0.0.1)
///
/// 所有沙箱的 Portal 服务都绑定到此地址，仅允许本地访问。
/// 这是出于安全考虑，防止外部网络直接访问沙箱。
pub const LOCALHOST_IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// 端口分配锁
///
/// 这是一个全局静态锁，确保同一时间只有一个线程可以分配端口。
/// 使用 `once_cell::sync::Lazy` 延迟初始化，第一次使用时创建。
///
/// ## 为什么需要锁？
///
/// 考虑以下竞态条件：
/// ```text
/// 线程 A: 获取可用端口 → OS 分配 5001
/// 线程 B: 获取可用端口 → OS 分配 5001 (同时)
/// 线程 A: 保存映射 sandbox-a -> 5001
/// 线程 B: 保存映射 sandbox-b -> 5001 (冲突！)
/// ```
///
/// 使用锁后：
/// ```text
/// 线程 A: 获取锁 → 获取可用端口 5001 → 保存映射 → 释放锁
/// 线程 B: 等待锁 → 获取锁 → 获取可用端口 5002 → 保存映射 → 释放锁
/// ```
static PORT_ASSIGNMENT_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// # 双向端口映射
///
/// 此结构体实现了沙箱名和端口号之间的双向映射。
///
/// ## 为什么需要双向映射？
///
/// - **正向查询** (sandbox → port): 获取沙箱的端口，用于构建 URL
/// - **反向查询** (port → sandbox): 通过端口查找沙箱，用于清理和验证
///
/// ## 数据结构
///
/// ```text
/// BiPortMapping {
///     sandbox_to_port: {
///         "sandbox-a": 5001,
///         "sandbox-b": 5002,
///     },
///     port_to_sandbox: {
///         5001: "sandbox-a",
///         5002: "sandbox-b",
///     },
/// }
/// ```
///
/// ## 一致性保证
///
/// 两个 HashMap 始终保持同步：
/// - 插入时同时更新两个映射
/// - 删除时同时删除两个映射
/// - 更新时先删除旧映射再插入新映射
#[derive(Debug, Clone, Default)]
pub struct BiPortMapping {
    /// 沙箱名到端口号的映射
    ///
    /// 用于快速查找沙箱的端口
    sandbox_to_port: HashMap<String, u16>,

    /// 端口号到沙箱名的映射
    ///
    /// 用于快速查找端口的沙箱
    port_to_sandbox: HashMap<u16, String>,
}

/// # 可序列化的端口映射
///
/// 这是 `BiPortMapping` 的简化版本，用于文件存储。
/// 只保存单向映射（沙箱→端口），因为反向映射可以从正向映射重建。
///
/// ## JSON 格式
///
/// ```json
/// {
///     "mappings": {
///         "sandbox-a": 5001,
///         "sandbox-b": 5002
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PortMapping {
    /// 沙箱名到端口号的映射
    pub mappings: HashMap<String, u16>,
}

/// # 端口管理器
///
/// 负责沙箱端口的分配、释放和持久化。
///
/// ## 字段说明
///
/// ### `mappings: BiPortMapping`
/// 内存中的端口映射，用于快速查找。
///
/// ### `file_path: PathBuf`
/// 持久化文件的路径，通常是 `~/.microsandbox/projects/portal_ports.json`。
///
/// ## 线程安全
///
/// `PortManager` 通常包装在 `Arc<RwLock<PortManager>>` 中使用：
/// - `Arc`: 多线程共享
/// - `RwLock`: 读写锁，支持并发读取和独占写入
#[derive(Debug)]
pub struct PortManager {
    /// 端口映射数据
    mappings: BiPortMapping,

    /// 端口映射文件的路径
    file_path: PathBuf,
}

//--------------------------------------------------------------------------------------------------
// BiPortMapping 方法实现
//--------------------------------------------------------------------------------------------------

impl BiPortMapping {
    /// # 创建新的双向映射
    ///
    /// 创建一个空的映射，两个 HashMap 都是空的。
    pub fn new() -> Self {
        Self {
            sandbox_to_port: HashMap::new(),
            port_to_sandbox: HashMap::new(),
        }
    }

    /// # 插入映射关系
    ///
    /// 添加或更新沙箱和端口之间的映射关系。
    ///
    /// ## 参数
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `sandbox_key` | `String` | 沙箱名称 |
    /// | `port` | `u16` | 端口号 |
    ///
    /// ## 实现细节
    ///
    /// 1. 检查端口是否已分配给其他沙箱，如果是则删除旧映射
    /// 2. 检查沙箱是否已有其他端口，如果是则删除旧映射
    /// 3. 插入新的双向映射
    ///
    /// ## 示例
    ///
    /// ```rust,ignore
    /// let mut mapping = BiPortMapping::new();
    ///
    /// // 插入新映射
    /// mapping.insert("sandbox-a".to_string(), 5001);
    ///
    /// // 更新沙箱的端口（自动删除旧端口映射）
    /// mapping.insert("sandbox-a".to_string(), 5002);
    ///
    /// // 更新端口的沙箱（自动删除旧沙箱映射）
    /// mapping.insert("sandbox-b".to_string(), 5001);
    /// ```
    pub fn insert(&mut self, sandbox_key: String, port: u16) {
        // 检查此端口是否已分配给其他沙箱
        if let Some(existing_sandbox) = self.port_to_sandbox.get(&port)
            && existing_sandbox != &sandbox_key
        {
            // 端口已分配给其他沙箱，删除旧映射
            warn!(
                "Port {} was already assigned to sandbox {}, reassigning to {}",
                port, existing_sandbox, sandbox_key
            );
            self.sandbox_to_port.remove(existing_sandbox);
        }

        // 检查此沙箱是否已有其他端口
        if let Some(existing_port) = self.sandbox_to_port.get(&sandbox_key)
            && *existing_port != port
        {
            // 沙箱已有其他端口，删除旧映射
            self.port_to_sandbox.remove(existing_port);
        }

        // 插入新的双向映射
        self.sandbox_to_port.insert(sandbox_key.clone(), port);
        self.port_to_sandbox.insert(port, sandbox_key);
    }

    /// # 通过沙箱名删除映射
    ///
    /// 删除指定沙箱的端口映射，并返回被删除的端口号。
    ///
    /// ## 返回值
    ///
    /// - `Some(port)`: 成功删除，返回被删除的端口号
    /// - `None`: 沙箱不存在于映射中
    pub fn remove_by_sandbox(&mut self, sandbox_key: &str) -> Option<u16> {
        // 先从 sandbox_to_port 中删除，获取端口号
        if let Some(port) = self.sandbox_to_port.remove(sandbox_key) {
            // 然后从 port_to_sandbox 中删除反向映射
            self.port_to_sandbox.remove(&port);
            Some(port)
        } else {
            None
        }
    }

    /// # 通过端口号删除映射
    ///
    /// 删除指定端口的映射，并返回被删除的沙箱名。
    ///
    /// ## 返回值
    ///
    /// - `Some(sandbox_key)`: 成功删除，返回被删除的沙箱名
    /// - `None`: 端口不存在于映射中
    pub fn remove_by_port(&mut self, port: u16) -> Option<String> {
        // 先从 port_to_sandbox 中删除，获取沙箱名
        if let Some(sandbox_key) = self.port_to_sandbox.remove(&port) {
            // 然后从 sandbox_to_port 中删除反向映射
            self.sandbox_to_port.remove(&sandbox_key);
            Some(sandbox_key)
        } else {
            None
        }
    }

    /// # 通过沙箱名获取端口
    ///
    /// ## 返回值
    ///
    /// - `Some(port)`: 找到映射，返回端口号
    /// - `None`: 沙箱不存在于映射中
    pub fn get_port(&self, sandbox_key: &str) -> Option<u16> {
        // .copied() 将 Option<&u16> 转换为 Option<u16>
        self.sandbox_to_port.get(sandbox_key).copied()
    }

    /// # 通过端口号获取沙箱名
    ///
    /// ## 返回值
    ///
    /// - `Some(&String)`: 找到映射，返回沙箱名的引用
    /// - `None`: 端口不存在于映射中
    pub fn get_sandbox(&self, port: u16) -> Option<&String> {
        self.port_to_sandbox.get(&port)
    }

    /// # 转换为可序列化格式
    ///
    /// 将 `BiPortMapping` 转换为 `PortMapping` 以便保存到文件。
    pub fn to_port_mapping(&self) -> PortMapping {
        PortMapping {
            mappings: self.sandbox_to_port.clone(),
        }
    }

    /// # 从可序列化格式加载
    ///
    /// 从 `PortMapping` 重建 `BiPortMapping`，恢复双向映射。
    pub fn from_port_mapping(mapping: PortMapping) -> Self {
        let mut result = Self::new();

        // 遍历单向映射，重建双向映射
        for (sandbox_key, port) in mapping.mappings {
            result.insert(sandbox_key, port);
        }

        result
    }
}

//--------------------------------------------------------------------------------------------------
// PortManager 方法实现
//--------------------------------------------------------------------------------------------------

impl PortManager {
    /// # 创建新的端口管理器
    ///
    /// 初始化端口管理器，从文件加载现有的端口映射。
    ///
    /// ## 参数
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `project_dir` | `impl AsRef<Path>` | 项目目录路径 |
    ///
    /// ## 返回值
    ///
    /// - `Ok(PortManager)`: 创建成功
    /// - `Err(MicrosandboxServerError)`: 加载失败
    ///
    /// ## 实现细节
    ///
    /// 1. 构建端口映射文件路径：`project_dir/portal_ports.json`
    /// 2. 调用 `load_mappings` 从文件加载
    /// 3. 返回初始化的管理器
    pub async fn new(project_dir: impl AsRef<Path>) -> MicrosandboxServerResult<Self> {
        // 构建端口映射文件路径
        let file_path = project_dir.as_ref().join(PORTAL_PORTS_FILE);
        // 加载现有映射（如果文件不存在则创建空映射）
        let mappings = Self::load_mappings(&file_path).await?;

        Ok(Self {
            mappings,
            file_path,
        })
    }

    /// # 从文件加载端口映射
    ///
    /// 从 JSON 文件中读取端口映射数据。
    ///
    /// ## 参数
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `file_path` | `&Path` | 文件路径 |
    ///
    /// ## 返回值
    ///
    /// - `Ok(BiPortMapping)`: 加载成功
    /// - `Err(MicrosandboxServerError)`: 读取或解析失败
    ///
    /// ## 处理逻辑
    ///
    /// 1. 检查文件是否存在
    /// 2. 如果存在：读取内容并解析 JSON
    /// 3. 如果不存在：返回空映射
    async fn load_mappings(file_path: &Path) -> MicrosandboxServerResult<BiPortMapping> {
        if file_path.exists() {
            // 读取文件内容
            let contents = fs::read_to_string(file_path).await.map_err(|e| {
                MicrosandboxServerError::ConfigError(format!(
                    "Failed to read port mappings file: {}",
                    e
                ))
            })?;

            // 解析 JSON
            let port_mapping: PortMapping = serde_json::from_str(&contents).map_err(|e| {
                MicrosandboxServerError::ConfigError(format!(
                    "Failed to parse port mappings file: {}",
                    e
                ))
            })?;

            // 转换为双向映射
            Ok(BiPortMapping::from_port_mapping(port_mapping))
        } else {
            // 文件不存在，返回空映射
            debug!("No port mappings file found, creating a new one");
            Ok(BiPortMapping::new())
        }
    }

    /// # 保存端口映射到文件
    ///
    /// 将当前的端口映射持久化到 JSON 文件。
    ///
    /// ## 实现细节
    ///
    /// 1. 将 `BiPortMapping` 转换为可序列化的 `PortMapping`
    /// 2. 序列化为格式化的 JSON 字符串
    /// 3. 创建父目录（如果不存在）
    /// 4. 写入文件
    async fn save_mappings(&self) -> MicrosandboxServerResult<()> {
        // 转换为可序列化格式
        let port_mapping = self.mappings.to_port_mapping();
        // 序列化为格式化的 JSON（pretty 表示带缩进）
        let contents = serde_json::to_string_pretty(&port_mapping).map_err(|e| {
            MicrosandboxServerError::ConfigError(format!(
                "Failed to serialize port mappings: {}",
                e
            ))
        })?;

        // 确保父目录存在
        if let Some(parent) = self.file_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent).await.map_err(|e| {
                MicrosandboxServerError::ConfigError(format!(
                    "Failed to create directory for port mappings file: {}",
                    e
                ))
            })?;
        }

        // 写入文件
        fs::write(&self.file_path, contents).await.map_err(|e| {
            MicrosandboxServerError::ConfigError(format!(
                "Failed to write port mappings file: {}",
                e
            ))
        })
    }

    /// # 为沙箱分配端口
    ///
    /// 这是端口管理器的核心功能，为指定沙箱分配一个可用的端口。
    ///
    /// ## 参数
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `key` | `&str` | 沙箱名称 |
    ///
    /// ## 返回值
    ///
    /// - `Ok(u16)`: 分配成功，返回端口号
    /// - `Err(MicrosandboxServerError)`: 分配失败
    ///
    /// ## 分配流程
    ///
    /// 1. **检查已有分配**: 如果沙箱已有端口，验证该端口是否仍可用
    /// 2. **获取锁**: 确保并发安全
    /// 3. **获取可用端口**: 绑定到端口 0，让 OS 分配
    /// 4. **保存映射**: 更新内存和文件
    /// 5. **返回端口**: 返回分配的端口号
    ///
    /// ## 端口可用性验证
    ///
    /// 使用 `TcpListener::bind()` 验证端口是否可绑定：
    /// - 可绑定 = 端口可用
    /// - 绑定失败 = 端口已被占用
    pub async fn assign_port(&mut self, key: &str) -> MicrosandboxServerResult<u16> {
        // 检查是否已分配端口
        if let Some(port) = self.mappings.get_port(key) {
            // 验证此端口是否仍可用
            if self.verify_port_availability(port) {
                // 端口可用，直接返回
                return Ok(port);
            } else {
                // 端口不可用（可能被其他进程占用）
                warn!(
                    "Previously assigned port {port} for sandbox {key} is no longer available, reassigning",
                );
                // 删除旧映射
                self.mappings.remove_by_sandbox(key);
            }
        }

        // 获取锁，确保同一时间只有一个线程分配端口
        let _lock = PORT_ASSIGNMENT_LOCK.lock().await;

        // 从 OS 获取可用端口
        let port = self.get_available_port_from_os()?;

        // 保存映射到内存和文件
        self.mappings.insert(key.to_string(), port);
        self.save_mappings().await?;

        info!("Assigned port {} to sandbox {}", port, key);
        Ok(port)
    }

    /// # 释放沙箱端口
    ///
    /// 释放指定沙箱的端口分配，使其可以被重新使用。
    ///
    /// ## 参数
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `key` | `&str` | 沙箱名称 |
    ///
    /// ## 返回值
    ///
    /// - `Ok(())`: 释放成功
    /// - `Err(MicrosandboxServerError)`: 保存失败
    pub async fn release_port(&mut self, key: &str) -> MicrosandboxServerResult<()> {
        // 从映射中删除
        if self.mappings.remove_by_sandbox(key).is_some() {
            // 保存到文件
            self.save_mappings().await?;
            info!("Released port for sandbox {}", key);
        }

        Ok(())
    }

    /// # 获取沙箱的已分配端口
    ///
    /// 查询沙箱的端口分配，不修改状态。
    ///
    /// ## 参数
    ///
    /// | 参数 | 类型 | 说明 |
    /// |------|------|------|
    /// | `key` | `&str` | 沙箱名称 |
    ///
    /// ## 返回值
    ///
    /// - `Some(u16)`: 已分配端口
    /// - `None`: 未分配端口
    pub fn get_port(&self, key: &str) -> Option<u16> {
        self.mappings.get_port(key)
    }

    /// # 验证端口可用性
    ///
    /// 检查指定端口是否可以绑定（未被其他进程占用）。
    ///
    /// ## 实现原理
    ///
    /// 尝试绑定到指定端口：
    /// - 成功 = 端口可用
    /// - 失败 = 端口已被占用
    ///
    /// ## 注意
    ///
    /// 这是一个竞态条件检查，两次检查之间端口可能被其他进程占用。
    /// 因此分配端口时需要使用锁。
    fn verify_port_availability(&self, port: u16) -> bool {
        let addr = SocketAddr::new(LOCALHOST_IP, port);
        // 尝试绑定，成功表示端口可用
        TcpListener::bind(addr).is_ok()
    }

    /// # 从 OS 获取可用端口
    ///
    /// 使用操作系统的端口分配机制获取一个真正可用的端口。
    ///
    /// ## 实现原理
    ///
    /// 1. 绑定到 `127.0.0.1:0`（端口 0 是特殊值）
    /// 2. OS 会自动分配一个可用的端口
    /// 3. 通过 `local_addr()` 获取分配的端口号
    /// 4. `TcpListener` 被丢弃后，端口释放
    /// 5. 返回端口号供后续使用
    ///
    /// ## 为什么使用此方法？
    ///
    /// 直接检查端口范围（如 8000-9000）可能会遇到：
    /// - 端口在检查和分配之间被占用（竞态条件）
    /// - 某些端口被防火墙或安全软件阻止
    ///
    /// 使用 OS 分配可以确保端口真正可用。
    fn get_available_port_from_os(&self) -> MicrosandboxServerResult<u16> {
        // 绑定到端口 0，让 OS 分配可用端口
        let addr = SocketAddr::new(LOCALHOST_IP, 0);
        let listener = TcpListener::bind(addr).map_err(|e| {
            MicrosandboxServerError::ConfigError(format!(
                "Failed to bind to address to get available port: {}",
                e
            ))
        })?;

        // 获取 OS 分配的端口号
        let port = listener
            .local_addr()
            .map_err(|e| {
                MicrosandboxServerError::ConfigError(format!(
                    "Failed to get local address from socket: {}",
                    e
                ))
            })?
            .port();

        debug!("OS assigned port {}", port);

        // listener 在此处被丢弃，释放端口
        // 返回端口号供调用者使用

        Ok(port)
    }
}
