//! # 可定位（Seekable）读写工具模块
//!
//! 本模块提供用于异步可定位读写（Async Seekable Read/Write）的 trait 和工具类型。
//!
//! ## 核心概念
//!
//! ### 什么是 Seekable？
//!
//! "Seekable" 指的是可以随机访问（定位）的数据流。与顺序读写不同，
//! seekable 允许你跳转到数据的任意位置进行读写。
//!
//! 类比：
//! - **顺序读写**：像磁带，只能从头到尾依次读写
//! - **随机读写（Seekable）**：像书籍，可以直接翻到任意页
//!
//! ### AsyncRead + AsyncSeek
//!
//! Rust 的 tokio 库提供了异步 IO trait：
//! - `AsyncRead`: 异步读取数据
//! - `AsyncWrite`: 异步写入数据
//! - `AsyncSeek`: 异步定位到指定位置
//!
//! 本模块的 `SeekableReader` 和 `SeekableWriter` trait 组合了这些 trait，
//! 提供更清晰的语义。
//!
//! ## 主要内容
//!
//! ### Trait
//! - [`SeekableReader`]: 可读 + 可定位的 trait
//! - [`SeekableWriter`]: 可写 + 可定位的 trait
//!
//! ### 类型
//! - [`EmptySeekableReader`]: 空阅读器，总是读取 0 字节
//! - [`EmptySeekableWriter`]: 空写入器，总是写入 0 字节
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::seekable::{SeekableReader, SeekableWriter};
//! use tokio::io::{AsyncRead, AsyncSeek};
//!
//! // 任何实现了 AsyncRead + AsyncSeek 的类型自动实现 SeekableReader
//! async fn process_reader<R: SeekableReader>(reader: R) {
//!     // 处理可定位的阅读器
//! }
//!
//! // 任何实现了 AsyncWrite + AsyncSeek 的类型自动实现 SeekableWriter
//! async fn process_writer<W: SeekableWriter>(writer: W) {
//!     // 处理可定位的写入器
//! }
//! ```
//!
//! ## 自动实现（Blanket Implementation）
//!
//! 本模块使用了 Rust 的 "blanket implementation" 特性：
//!
//! ```rust,ignore
//! impl<T> SeekableReader for T where T: AsyncRead + AsyncSeek {}
//! ```
//!
//! 这意味着：**任何**同时实现了 `AsyncRead` 和 `AsyncSeek` 的类型，
//! 都会**自动**实现 `SeekableReader` trait，无需手动实现。
//!
//! 这种设计模式：
//! - 减少了样板代码
//! - 提供了更清晰的语义（看到 `SeekableReader` 就知道需要 `AsyncRead + AsyncSeek`）
//! - 便于未来的扩展和修改

use std::{
    io::{self, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// ### 空的可定位阅读器
///
/// `EmptySeekableReader` 是一个特殊的阅读器，它的行为如下：
/// - **读取**: 总是读取 0 字节（相当于 EOF，文件结束）
/// - **定位**: 总是报告位置为 0
///
/// ## 用途
///
/// 这个类型主要用于：
/// 1. **测试**: 作为 mock 对象用于单元测试
/// 2. **占位符**: 当需要一个 "什么都不做" 的阅读器时
/// 3. **默认值**: 作为可选阅读器的默认实现
///
/// ## 示例
///
/// ```rust
/// use microsandbox_utils::seekable::EmptySeekableReader;
/// use tokio::io::AsyncReadExt;
///
/// #[tokio::main]
/// async fn main() {
///     let mut reader = EmptySeekableReader;
///     let mut buf = [0u8; 100];
///
///     // 总是读取 0 字节
///     let n = reader.read(&mut buf).await.unwrap();
///     assert_eq!(n, 0);  // 返回 0 表示 EOF
///
///     // 定位总是成功，位置总是 0
///     let pos = reader.seek(std::io::SeekFrom::Start(100)).await.unwrap();
///     assert_eq!(pos, 0);
/// }
/// ```
///
/// ## 派生 trait
///
/// `#[derive(Debug)]` 使得这个类型可以使用 `{:?}` 格式化输出，
/// 便于调试和日志记录。
#[derive(Debug)]
pub struct EmptySeekableReader;

/// ### 空的可定位写入器
///
/// `EmptySeekableWriter` 是一个特殊的写入器，它的行为如下：
/// - **写入**: 总是报告成功，但实际不写入任何数据（/dev/null 行为）
/// - **定位**: 总是报告位置为 0
///
/// ## 用途
///
/// 这个类型主要用于：
/// 1. **测试**: 当需要测试代码逻辑但不关心实际输出时
/// 2. **丢弃数据**: 类似 Unix 的 `/dev/null`，写入的数据被丢弃
/// 3. **性能基准**: 测量纯写入逻辑的性能，排除实际 IO 的影响
///
/// ## 示例
///
/// ```rust
/// use microsandbox_utils::seekable::EmptySeekableWriter;
/// use tokio::io::AsyncWriteExt;
///
/// #[tokio::main]
/// async fn main() {
///     let mut writer = EmptySeekableWriter;
///
///     // 写入数据，但实际被丢弃
///     let n = writer.write(b"hello").await.unwrap();
///     assert_eq!(n, 5);  // 报告写入了 5 字节
///
///     // 定位总是成功
///     let pos = writer.seek(std::io::SeekFrom::Start(100)).await.unwrap();
///     assert_eq!(pos, 0);
/// }
/// ```
#[derive(Debug)]
pub struct EmptySeekableWriter;

//--------------------------------------------------------------------------------------------------
// Trait 定义
//--------------------------------------------------------------------------------------------------

/// ### 可定位阅读器 trait
///
/// `SeekableReader` 是一个组合 trait，表示一个可以异步读取和定位的数据源。
///
/// ## Trait 边界
///
/// ```rust,ignore
/// pub trait SeekableReader: AsyncRead + AsyncSeek {}
/// ```
///
/// 这意味着 `SeekableReader` 要求实现者同时实现：
/// - `AsyncRead`: 可以异步读取数据
/// - `AsyncSeek`: 可以异步定位到任意位置
///
/// ## 设计目的
///
/// 1. **语义清晰**: 看到 `SeekableReader` 就知道是一个可读且可定位的流
/// 2. **约束简化**: 在泛型约束中更简洁
/// 3. **未来扩展**: 可以在不破坏 API 的情况下添加默认方法
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_utils::seekable::SeekableReader;
/// use tokio::io::AsyncReadExt;
///
/// // 泛型函数，要求参数实现 SeekableReader
/// async fn read_from_seekable<R: SeekableReader + Unpin>(mut reader: R) {
///     // 读取一些数据
///     let mut buf = Vec::new();
///     reader.read_to_end(&mut buf).await.unwrap();
///
///     // 定位到开头
///     reader.seek(std::io::SeekFrom::Start(0)).await.unwrap();
///
///     // 再次读取
///     let mut buf2 = Vec::new();
///     reader.read_to_end(&mut buf2).await.unwrap();
/// }
/// ```
///
/// ## 与标准库的关系
///
/// Rust 标准库中的 `std::io::Read` 和 `std::io::Seek` 是同步版本。
/// 本 trait 是它们的异步版本，用于 tokio 异步运行时。
pub trait SeekableReader: AsyncRead + AsyncSeek {}

/// ### 可定位写入器 trait
///
/// `SeekableWriter` 是一个组合 trait，表示一个可以异步写入和定位的数据目的地。
///
/// ## Trait 边界
///
/// ```rust,ignore
/// pub trait SeekableWriter: AsyncWrite + AsyncSeek {}
/// ```
///
/// 这意味着 `SeekableWriter` 要求实现者同时实现：
/// - `AsyncWrite`: 可以异步写入数据
/// - `AsyncSeek`: 可以异步定位到任意位置
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_utils::seekable::SeekableWriter;
/// use tokio::io::AsyncWriteExt;
///
/// // 泛型函数，要求参数实现 SeekableWriter
/// async fn write_to_seekable<W: SeekableWriter + Unpin>(mut writer: W) {
///     // 写入数据
///     writer.write_all(b"hello").await.unwrap();
///
///     // 定位到开头
///     writer.seek(std::io::SeekFrom::Start(0)).await.unwrap();
///
///     // 覆盖写入
///     writer.write_all(b"world").await.unwrap();
/// }
/// ```
pub trait SeekableWriter: AsyncWrite + AsyncSeek {}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

/// ### SeekableReader 的自动实现
///
/// 这是一个 "blanket implementation"（覆盖实现）：
/// **任何**同时实现了 `AsyncRead` 和 `AsyncSeek` 的类型，
/// 都会**自动**实现 `SeekableReader` trait。
///
/// ## 什么是 Blanket Implementation？
///
/// Blanket implementation 是 Rust 中的一种设计模式，允许为一个泛型类型 `T`
/// 实现 trait，只要 `T` 满足某些条件。
///
/// 语法：
/// ```rust,ignore
/// impl<T> Trait for T where T: OtherTrait {}
/// ```
///
/// ## 示例
///
/// ```rust
/// use tokio::fs::File;
/// use microsandbox_utils::seekable::SeekableReader;
///
/// // tokio::fs::File 实现了 AsyncRead + AsyncSeek
/// // 因此自动实现了 SeekableReader
/// async fn read_file(file: File) {
///     process_reader(file).await;  // 可以直接传递
/// }
///
/// async fn process_reader<R: SeekableReader>(reader: R) {
///     // 处理...
/// }
/// ```
impl<T> SeekableReader for T where T: AsyncRead + AsyncSeek {}

/// ### SeekableWriter 的自动实现
///
/// 与 [`SeekableReader`] 类似，任何同时实现了 `AsyncWrite` 和 `AsyncSeek`
/// 的类型都会自动实现 `SeekableWriter` trait。
impl<T> SeekableWriter for T where T: AsyncWrite + AsyncSeek {}

// ============================================================================
// EmptySeekableReader 的 AsyncRead 实现
// ============================================================================

/// ### AsyncRead trait 实现
///
/// 实现 `AsyncRead` 使得 `EmptySeekableReader` 可以作为异步阅读器使用。
///
/// ## 行为说明
///
/// `poll_read` 方法总是立即返回 `Ok(())`，但不会向缓冲区写入任何数据。
/// 这相当于文件结束（EOF）的状态。
///
/// ## 参数说明
///
/// - `self: Pin<&mut Self>`: 自引用类型，用于异步操作
/// - `_cx: &mut Context<'_>`: 异步上下文，用于注册唤醒器（这里未使用）
/// - `_buf: &mut ReadBuf<'_>`: 读取缓冲区（这里不会被修改）
///
/// ## 返回值
///
/// `Poll::Ready(Ok(()))` 表示操作立即完成，没有错误。
/// 但由于缓冲区没有被填充，调用者会认为已经到达 EOF。
///
/// ## Poll 类型解释
///
/// `Poll<T>` 是 Rust 异步 IO 的返回类型：
/// - `Poll::Ready(value)`: 操作完成，返回结果
/// - `Poll::Pending`: 操作尚未完成，稍后会有唤醒通知
///
/// 对于 `EmptySeekableReader`，由于没有实际的 IO 操作，总是立即返回 `Ready`。
impl AsyncRead for EmptySeekableReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // 立即返回，不修改缓冲区（相当于读取 0 字节）
        // 在 Rust 的 Read trait 中，返回 0 字节表示 EOF
        Poll::Ready(Ok(()))
    }
}

// ============================================================================
// EmptySeekableReader 的 AsyncSeek 实现
// ============================================================================

/// ### AsyncSeek trait 实现
///
/// 实现 `AsyncSeek` 使得 `EmptySeekableReader` 可以执行定位操作。
///
/// ## 行为说明
///
/// - `start_seek`: 总是立即成功，忽略目标位置
/// - `poll_complete`: 总是返回位置 0
///
/// ## 为什么需要两个方法？
///
/// `AsyncSeek` 分为两个阶段：
/// 1. `start_seek`: 开始定位操作（可能触发系统调用）
/// 2. `poll_complete`: 检查定位是否完成并获取新位置
///
/// 这种设计允许异步地执行可能耗时的定位操作。
/// 对于 `EmptySeekableReader`，由于没有实际的数据源，两个操作都是瞬间完成的。
impl AsyncSeek for EmptySeekableReader {
    /// ### 开始定位操作
    ///
    /// ## 参数
    /// - `_position: SeekFrom`: 目标位置（被忽略）
    ///
    /// `SeekFrom` 有三种模式：
    /// - `SeekFrom::Start(n)`: 从开头偏移 n 字节
    /// - `SeekFrom::End(n)`: 从结尾偏移 n 字节
    /// - `SeekFrom::Current(n)`: 从当前位置偏移 n 字节
    ///
    /// ## 返回值
    /// - `Ok(())`: 总是成功（因为没有实际的定位操作）
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        // 忽略目标位置，直接返回成功
        Ok(())
    }

    /// ### 完成定位操作
    ///
    /// ## 返回值
    /// - `Poll::Ready(Ok(0))`: 总是返回位置 0
    ///
    /// 由于 `EmptySeekableReader` 是一个空的阅读器，
    /// 它的位置始终是 0（开头）。
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        // 总是报告位置为 0
        Poll::Ready(Ok(0))
    }
}
