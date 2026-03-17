//! # 共享状态模块 (Shared State)
//!
//! 本模块实现了 microsandbox portal 服务器的共享状态管理。
//!
//! ## 什么是共享状态？
//!
//! 在 Web 服务器中，共享状态是指在多个请求之间共享的数据。由于 HTTP 是无状态协议，
//! 每个请求默认是独立的。但在某些场景下，我们需要在请求间共享数据，例如：
//! - 数据库连接池
//! - 缓存
//! - 服务器配置
//! - 长生命周期资源的句柄（如本例中的引擎句柄）
//!
//! ## 线程安全设计
//!
//! 由于 Web 服务器并发处理多个请求，共享状态必须是线程安全的。本模块使用以下技术：
//!
//! ### Arc (Atomic Reference Counting)
//! - **作用**: 允许多个线程安全地共享所有权
//! - **原理**: 使用原子操作管理引用计数
//! - **特点**: 克隆 Arc 只是增加引用计数，不复制数据
//!
//! ### AtomicBool
//! - **作用**: 提供原子布尔操作
//! - **优势**: 无需锁即可安全地进行读写
//! - **使用场景**: `ready` 标志，表示服务器是否就绪
//!
//! ### tokio::sync::Mutex
//! - **作用**: 异步互斥锁，保护共享数据
//! - **与 std::sync::Mutex 的区别**:
//!   - `tokio::sync::Mutex`: 可以在 `.await` 时持有锁
//!   - `std::sync::Mutex`: 在 `.await` 时持有锁会导致问题
//! - **使用场景**: 引擎句柄和命令执行器句柄
//!
//! ## SharedState 结构
//!
//! ```text
//! SharedState
//! ├── ready: Arc<AtomicBool>     # 服务器就绪标志
//! ├── engine_handle: Arc<Mutex<Option<EngineHandle>>>  # REPL 引擎句柄
//! └── command_handle: Arc<Mutex<Option<CommandHandle>>> # 命令执行器句柄
//! ```

use std::sync::{Arc, atomic::AtomicBool};
use tokio::sync::Mutex;

use crate::portal::{command::CommandHandle, repl::EngineHandle};

//--------------------------------------------------------------------------------------------------
// 类型 (Types)
//--------------------------------------------------------------------------------------------------

/// # 共享状态结构
///
/// 此结构包含服务器的全局状态，在多个请求间共享。
///
/// ## 字段说明
///
/// ### ready: Arc<AtomicBool>
/// 服务器就绪标志：
/// - `true`: 服务器已初始化完成，可以处理请求
/// - `false`: 服务器正在初始化，尚未就绪
/// - 使用 `AtomicBool` 实现无锁线程安全访问
///
/// ### engine_handle: Arc<Mutex<Option<EngineHandle>>>
/// REPL 引擎句柄的可选包装：
/// - `None`: 引擎尚未初始化
/// - `Some(EngineHandle)`: 引擎已初始化，可以执行代码
/// - 使用 `Mutex` 保护，因为需要可变访问来初始化
/// - 使用 `Arc` 实现多请求共享
///
/// ### command_handle: Arc<Mutex<Option<CommandHandle>>>
/// 命令执行器句柄的可选包装：
/// - 同 `engine_handle`，但用于命令执行
///
/// ## Clone 实现
///
/// 派生的 `Clone` 实现是浅克隆：
/// - 克隆 `Arc` 只是增加引用计数
/// - 所有克隆共享相同的基础数据
/// - 这是期望的行为，符合共享状态的设计目的
///
/// ## Debug 实现
///
/// 派生的 `Debug` 实现用于调试输出。
/// 注意：实际使用时可能因为互斥锁而输出简化信息。
#[derive(Clone, Debug)]
pub struct SharedState {
    /// 服务器就绪标志
    ///
    /// 此标志在服务器完成初始化后设置为 true
    /// 健康检查端点使用此标志判断服务器状态
    pub ready: Arc<AtomicBool>,

    /// REPL 引擎句柄
    ///
    /// 使用 Mutex 保护，因为：
    /// 1. 需要在首次请求时初始化（可变操作）
    /// 2. 多个请求可能同时访问
    ///
    /// Option 表示引擎可能尚未初始化（懒加载模式）
    pub engine_handle: Arc<Mutex<Option<EngineHandle>>>,

    /// 命令执行器句柄
    ///
    /// 同 engine_handle，但用于命令执行引擎
    pub command_handle: Arc<Mutex<Option<CommandHandle>>>,
}

/// # Default trait 实现
///
/// 提供创建默认 `SharedState` 的方法。
///
/// ## 默认值
///
/// - `ready`: `false` - 服务器初始状态为未就绪
/// - `engine_handle`: `None` - 引擎尚未初始化
/// - `command_handle`: `None` - 命令执行器尚未初始化
///
/// ## 使用场景
///
/// ```rust
/// // 创建默认状态
/// let state = SharedState::default();
///
/// // 或者使用 turbofish 语法
/// let state = SharedState::Default::default();
/// ```
impl Default for SharedState {
    /// 创建默认的 SharedState 实例
    ///
    /// 所有字段初始化为"未初始化"状态：
    /// - ready = false
    /// - engine_handle = None
    /// - command_handle = None
    fn default() -> Self {
        Self {
            // 创建新的 AtomicBool，初始值为 false
            // Arc::new 将其包装在 Arc 中
            ready: Arc::new(AtomicBool::new(false)),

            // 创建新的 Mutex，初始值为 None
            // 表示引擎尚未初始化
            engine_handle: Arc::new(Mutex::new(None)),

            // 创建新的 Mutex，初始值为 None
            // 表示命令执行器尚未初始化
            command_handle: Arc::new(Mutex::new(None)),
        }
    }
}
