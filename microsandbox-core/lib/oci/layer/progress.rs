//! OCI 层下载和提取进度条模块
//!
//! 本模块提供了在 CLI 模式下显示进度条的功能。
//!
//! ## 模块组成
//!
//! - **build_progress_bar()**: 创建进度条
//! - **ProgressReader**: 包装 AsyncRead，在读取时更新进度
//!
//! ## 功能条件编译
//!
//! 此模块的所有功能仅在 `cli` 特性启用时可用：
//! ```toml
//! [features]
//! cli = ["indicatif", "pin-project-lite"]
//! ```
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! // 创建进度条
//! let pb = build_progress_bar(total_bytes, "Downloading");
//!
//! // 包装读取器
//! let reader = ProgressReader {
//!     inner: file,
//!     bar: pb.clone(),
//! };
//!
//! // 读取时自动更新进度
//! let mut archive = Archive::new(GzipDecoder::new(BufReader::new(reader)));
//! ```

#[cfg(feature = "cli")]
use indicatif::{ProgressBar, ProgressStyle};
#[cfg(feature = "cli")]
use microsandbox_utils::MULTI_PROGRESS;
#[cfg(feature = "cli")]
use pin_project_lite::pin_project;
#[cfg(feature = "cli")]
use std::task::Poll;
#[cfg(feature = "cli")]
use tokio::io::{AsyncRead, ReadBuf};

//--------------------------------------------------------------------------------------------------
// 进度条构建器
//--------------------------------------------------------------------------------------------------

/// 构建进度条
///
/// 此函数创建一个 indicatif 进度条，用于显示下载或提取进度。
///
/// ## 参数
///
/// * `total_bytes` - 总字节数
/// * `prefix` - 进度条前缀文本（通常是层 digest 的前 8 位）
///
/// ## 返回值
///
/// 返回一个配置好的 `ProgressBar` 实例
///
/// ## 进度条样式
///
/// ```text
/// {prefix} {bar:40.green/green.dim} {bytes}/{total_bytes}
/// ```
///
/// 示例输出：
/// ```text
/// sha256.abc12345 ████████████████████████████░░░░░░░░ 1.5MB/5.0MB
/// ```
///
/// ## 进度条字符
///
/// | 字符 | 含义 |
/// |------|------|
/// | `█` (`=`) | 已完成部分 |
/// | `░` (`+`) | 进行中位置 |
/// | `░` (`-`) | 未完成部分 |
///
/// ## 使用示例
///
/// ```rust,ignore
/// let pb = build_progress_bar(5_000_000, "sha256.abc12345");
/// // 读取数据时更新进度
/// pb.inc(bytes_read as u64);
/// // 完成后清除
/// pb.finish_and_clear();
/// ```
#[cfg(feature = "cli")]
pub(super) fn build_progress_bar(total_bytes: u64, prefix: &str) -> ProgressBar {
    // 从全局 MULTI_PROGRESS 获取一个新的进度条
    let pb = MULTI_PROGRESS.add(ProgressBar::new(total_bytes));
    pb.set_style(
        ProgressStyle::with_template(
            "{prefix:.bold.dim} {bar:40.green/green.dim} {bytes:.bold}/{total_bytes:.dim}",
        )
        .unwrap()
        .progress_chars("=+-"),
    );
    pb.set_prefix(prefix.to_string());
    pb
}

//--------------------------------------------------------------------------------------------------
// ProgressReader - 进度追踪读取器
//--------------------------------------------------------------------------------------------------

/// 进度追踪读取器
///
/// 这是一个包装器，包装任何 `AsyncRead` 实现，在读取数据时自动更新进度条。
///
/// ## 泛型参数
///
/// - `R`: 被包装的 `AsyncRead` 类型
///
/// ## 字段说明
///
/// - `inner`: 被包装的内部读取器
/// - `bar`: 进度条，每次读取时更新
///
/// ## pin_project 宏
///
/// 使用 `#[pin_project]` 宏为此结构体生成必要的代码，
/// 以便在 `AsyncRead` 实现中进行自引用（self-referential）操作。
///
/// ## 使用示例
///
/// ```rust,ignore
/// let reader = ProgressReader {
///     inner: file,
///     bar: progress_bar,
/// };
///
/// // 读取时自动更新进度
/// let mut archive = Archive::new(BufReader::new(reader));
/// ```
#[cfg(feature = "cli")]
pin_project! {
    pub(super) struct ProgressReader<R> {
        #[pin]
        pub(super) inner: R,
        pub(super) bar: ProgressBar,
    }
}

/// 为 ProgressReader 实现 AsyncRead trait
///
/// 此实现在每次读取时更新进度条。
///
/// ## 实现细节
///
/// 1. 使用 `self.project()` 获取字段的投影
/// 2. 调用内部读取器的 `poll_read` 方法
/// 3. 如果读取完成（`Poll::Ready`），获取读取的字节数
/// 4. 更新进度条（`bar.inc(n)`）
/// 5. 返回 `Poll::Ready(Ok(()))`
///
/// ## 为什么需要 `buf.filled().len()`？
///
/// `ReadBuf::filled()` 返回已填充数据的切片，
/// 其长度就是本次读取的字节数。
///
/// ## 异步读取状态
///
/// | 状态 | 行为 |
/// |------|------|
/// | `Poll::Ready` | 读取完成，更新进度条 |
/// | `Poll::Pending` | 读取未完成，向上传播 Pending |
#[cfg(feature = "cli")]
impl<R: AsyncRead> AsyncRead for ProgressReader<R> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // 获取投影（pin_project 生成的方法）
        let p = self.project();
        match p.inner.poll_read(cx, buf)? {
            // 读取完成
            Poll::Ready(()) => {
                // 获取本次读取的字节数
                let n = buf.filled().len();
                if n > 0 {
                    // 更新进度条
                    p.bar.inc(n as u64);
                }
                Poll::Ready(Ok(()))
            }
            // 读取未完成，向上传播
            Poll::Pending => Poll::Pending,
        }
    }
}
