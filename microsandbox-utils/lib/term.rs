//! # 终端工具模块
//!
//! 本模块提供终端相关的工具函数和常量，用于在 CLI 中显示进度条、spinner 等可视化元素。
//!
//! ## 核心功能
//!
//! ### 终端检测
//! - [`is_interactive_terminal()`]: 检测是否在交互式 TTY 环境中运行
//! - [`is_ansi_interactive_terminal()`]: 检测是否支持 ANSI 转义序列的终端
//!
//! ### 进度条工具
//! - [`create_spinner()`]: 创建带 spinner 的进度条
//! - [`finish_with_error()`]: 以错误标记结束进度条
//!
//! ### 全局常量
//! - [`MULTI_PROGRESS`]: 全局多进度条实例
//! - [`CHECKMARK`]: 绿色对勾符号 (✓)
//! - [`ERROR_MARK`]: 红色叉号 (✗)
//! - [`TICK_STRINGS`]: spinner 动画帧
//! - [`ERROR_TICK_STRINGS`]: 错误状态的 spinner 帧
//!
//! ## 依赖库
//!
//! 本模块使用以下 crate：
//! - **indicatif**: 用于显示进度条和 spinner
//! - **console**: 用于终端样式化（颜色、符号等）
//! - **libc**: 用于底层 TTY 检测
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::term::{
//!     create_spinner, finish_with_error, is_interactive_terminal,
//!     MULTI_PROGRESS, CHECKMARK,
//! };
//!
//! // 检测终端类型
//! if is_interactive_terminal() {
//!     println!("运行在交互式终端中");
//! }
//!
//! // 创建 spinner 进度条
//! let pb = create_spinner("正在下载...".to_string(), None, None);
//!
//! // 模拟工作
//! std::thread::sleep(std::time::Duration::from_secs(1));
//!
//! // 成功完成（spinner 会自动显示对勾）
//! pb.finish_with_message("下载完成！");
//!
//! // 或者以错误结束
//! // finish_with_error(&pb);
//! ```
//!
//! ## LazyLock 说明
//!
//! 本模块使用 `LazyLock` 来初始化全局常量。`LazyLock` 是 Rust 标准库中的
//! 延迟初始化智能指针，特点是：
//! - 首次访问时才进行初始化（懒加载）
//! - 线程安全，保证只初始化一次
//! - 后续访问零开销
//!
//! 这对于 `MultiProgress` 这样的复杂对象特别有用，因为它们的创建
//! 可能涉及系统调用和资源分配。

use indicatif::{MultiProgress, MultiProgressAlignment, ProgressBar, ProgressStyle};
use std::sync::{Arc, LazyLock};

//--------------------------------------------------------------------------------------------------
// 全局常量
//--------------------------------------------------------------------------------------------------

/// ### 全局多进度条实例
///
/// 这是一个懒加载的全局 `MultiProgress` 实例，用于管理和显示多个进度条。
///
/// ## MultiProgress 简介
/// `MultiProgress` 是 `indicatif` 库提供的类型，允许同时显示和管理多个
/// 进度条。它确保多个进度条的输出不会相互干扰，正确地在终端上刷新。
///
/// ## 初始化细节
/// 1. 创建一个新的 `MultiProgress` 实例
/// 2. 设置对齐方式为 `Top`（从顶部开始显示）
/// 3. 用 `Arc` 包装，允许多线程共享
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::term::MULTI_PROGRESS;
///
/// // 添加一个新的进度条
/// let pb = MULTI_PROGRESS.add(ProgressBar::new(100));
/// pb.set_message("任务 1");
///
/// // 在另一个线程中也可以使用
/// let pb2 = MULTI_PROGRESS.add(ProgressBar::new(50));
/// ```
pub static MULTI_PROGRESS: LazyLock<Arc<MultiProgress>> = LazyLock::new(|| {
    let mp = MultiProgress::new();
    mp.set_alignment(MultiProgressAlignment::Top);
    Arc::new(mp)
});

/// ### 绿色对勾符号：✓
///
/// 用于表示操作成功完成的视觉标记。
///
/// ## 样式
/// 使用 `console` 库设置为绿色，在支持颜色的终端上会以绿色显示。
///
/// ## 显示效果
/// - 支持颜色的终端：<span style="color: green">✓</span>
/// - 不支持颜色的终端：✓
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::term::CHECKMARK;
///
/// println!("操作完成 {}", *CHECKMARK);
/// // 输出：操作完成 ✓（绿色）
/// ```
pub static CHECKMARK: LazyLock<String> =
    LazyLock::new(|| format!("{}", console::style("✓").green()));

/// ### 红色叉号符号：✗
///
/// 用于表示操作失败的视觉标记。
///
/// ## 样式
/// 使用 `console` 库设置为红色，在支持颜色的终端上会以红色显示。
///
/// ## 显示效果
/// - 支持颜色的终端：<span style="color: red">✗</span>
/// - 不支持颜色的终端：✗
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::term::ERROR_MARK;
///
/// println!("操作失败 {}", *ERROR_MARK);
/// // 输出：操作失败 ✗（红色）
/// ```
pub static ERROR_MARK: LazyLock<String> =
    LazyLock::new(|| format!("{}", console::style("✗").red()));

/// ### Spinner 动画帧序列
///
/// 定义 spinner 旋转动画的每一帧，共 11 帧。
///
/// ## 动画序列
/// 前 10 帧是旋转的 Unicode 字符：
/// - ⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏
///
/// 第 11 帧是对勾符号（`CHECKMARK`），用于表示完成状态。
///
/// ## Unicode 字符说明
/// 这些字符来自 Braille 图案（盲文），Unicode 范围 U+2800-U+28FF。
/// 它们被广泛用于终端 spinner 动画，因为：
/// - 视觉上看起来像旋转
/// - 在所有终端上都能正确显示
/// - 不会与其他字符混淆
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::term::TICK_STRINGS;
///
/// // 打印所有帧
/// for (i, tick) in TICK_STRINGS.iter().enumerate() {
///     println!("帧 {}: {}", i, tick);
/// }
/// ```
pub static TICK_STRINGS: LazyLock<[&str; 11]> =
    LazyLock::new(|| ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", &CHECKMARK]);

/// ### 错误状态 Spinner 帧序列
///
/// 用于错误状态的 spinner 动画，只有 2 帧。
///
/// ## 动画序列
/// - ⠏: 等待帧
/// - `ERROR_MARK`: 错误标记（红色叉号）
///
/// ## 使用场景
/// 当操作失败时，使用这个简短的动画来表示错误状态，
/// 然后以红色叉号结束，给用户清晰的视觉反馈。
pub static ERROR_TICK_STRINGS: LazyLock<[&str; 2]> = LazyLock::new(|| ["⠏", &ERROR_MARK]);

//--------------------------------------------------------------------------------------------------
// 函数定义
//--------------------------------------------------------------------------------------------------

/// ### 检测交互式终端环境
///
/// 此函数判断当前进程是否运行在交互式终端（TTY）环境中。
///
/// ## 检测逻辑
///
/// 1. **TTY 检查**: 使用 `libc::isatty()` 检查 stdin 和 stdout 是否都是 TTY
///    - `STDIN_FILENO`: 标准输入的文件描述符（通常是 0）
///    - `STDOUT_FILENO`: 标准输出的文件描述符（通常是 1）
///    - `isatty()`: 返回 1 表示是 TTY，0 表示不是
///
/// 2. **TERM 环境变量检查**（可选）: 检查是否存在 `TERM` 环境变量
///    - `TERM` 变量定义了终端类型（如 `xterm-256color`、`vt100` 等）
///    - 某些精简环境可能没有 `TERM`，但仍可能是有效的 TTY
///
/// 3. **最终判断**: 只要 stdin 和 stdout 都是 TTY，就返回 `true`
///    - 即使没有 `TERM` 变量也返回 `true`
///    - 但会记录调试日志提醒用户
///
/// ## 返回值
/// - `true`: 运行在交互式 TTY 环境中
/// - `false`: 运行在非交互式环境（如管道、重定向、CI/CD）
///
/// ## TTY 概念解释
///
/// **TTY**（Teletypewriter）是 Unix/Linux 系统中的终端设备：
/// - **交互式 TTY**: 用户可以直接输入命令的终端（如终端窗口、SSH 会话）
/// - **非交互式**: 重定向到文件或其他进程的输入输出
///
/// 示例：
/// ```bash
/// # 交互式 TTY
/// ./myapp
///
/// # 非交互式（输出重定向到文件）
/// ./myapp > output.txt
///
/// # 非交互式（管道）
/// ./myapp | grep "something"
/// ```
///
/// ## 使用场景
///
/// - 决定是否显示进度条和 spinner
/// - 决定是否使用 ANSI 颜色和格式化
/// - 决定是否启用交互式提示
///
/// ## 实现细节
///
/// 使用 `unsafe` 调用 libc 函数是因为 `isatty()` 是一个 FFI（外部函数接口）调用。
/// 虽然 `isatty()` 本身是线程安全的，但 FFI 调用在 Rust 中默认标记为 `unsafe`。
///
/// ## 示例
///
/// ```rust
/// use microsandbox_utils::term::is_interactive_terminal;
///
/// if is_interactive_terminal() {
///     println!("显示进度条和动画");
/// } else {
///     println!("使用纯文本输出");
/// }
/// ```
///
/// ## 相关函数
/// - [`is_ansi_interactive_terminal()`]: 进一步检查是否支持 ANSI
pub fn is_interactive_terminal() -> bool {
    // 检查 stdin 和 stdout 是否都是 TTY
    // libc::isatty() 是一个 FFI 调用，检查文件描述符是否连接到终端设备
    // 返回 1 表示是 TTY，0 表示不是
    // 使用 unsafe 是因为这是外部 C 函数调用
    let stdin_is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) == 1 };
    let stdout_is_tty = unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 };

    // 基本条件：stdin 和 stdout 必须都是 TTY
    // 这确保了我们真的有交互式输入和输出能力
    let is_tty = stdin_is_tty && stdout_is_tty;

    // 可选增强：检查 TERM 环境变量
    // TERM 定义了终端类型和能力，但某些环境可能没有设置
    let has_term = std::env::var("TERM").is_ok();

    // 记录调试信息
    // 如果是 TTY 但没有 TERM 变量，可能是配置问题
    if is_tty && !has_term {
        tracing::debug!("detected TTY without TERM environment variable");
    }

    // 返回 TTY 检测结果
    // 注意：即使没有 TERM，只要是 TTY 就返回 true
    // 这是因为某些精简环境（如容器）可能没有 TERM 但仍支持交互
    is_tty
}

/// ### 检测 ANSI 交互式终端
///
/// 此函数在 [`is_interactive_terminal()`] 的基础上，进一步检查终端是否支持 ANSI 转义序列。
///
/// ## ANSI 转义序列
///
/// ANSI 转义序列是用于控制终端输出的特殊字符序列，可以：
/// - 设置文本颜色（前景/背景）
/// - 移动光标位置
/// - 清除屏幕
/// - 设置文本属性（粗体、下划线等）
///
/// 示例：`\x1b[31m` 设置红色文本，`\x1b[0m` 重置所有属性。
///
/// ## 检测逻辑
///
/// 1. 首先调用 [`is_interactive_terminal()`] 检查是否是交互式 TTY
/// 2. 然后检查 `TERM` 变量是否包含 "dumb"
///    - "dumb" 终端是最基本的终端类型，只支持纯文本
///    - 不支持颜色、光标控制等高级功能
///
/// ## 返回值
/// - `true`: 支持 ANSI 的交互式终端
/// - `false`: 非交互式终端或 "dumb" 终端
///
/// ## "dumb" 终端说明
///
/// `TERM=dumb` 通常在以下情况出现：
/// - Emacs 的内部终端
/// - 某些 IDE 的集成终端
/// - 明确设置为最基本终端模式
///
/// ## 使用场景
///
/// - 决定是否使用颜色和样式
/// - 决定是否使用 Unicode 符号（可能在某些终端显示异常）
/// - 决定是否使用进度条动画
///
/// ## 示例
///
/// ```rust
/// use microsandbox_utils::term::is_ansi_interactive_terminal;
///
/// if is_ansi_interactive_terminal() {
///     println!("\x1b[32m显示绿色文本\x1b[0m");
/// } else {
///     println!("显示纯文本");
/// }
/// ```
pub fn is_ansi_interactive_terminal() -> bool {
    // 首先检查是否是交互式终端
    // 然后检查 TERM 是否不是 "dumb"
    // contains("dumb") 返回 true 表示是 dumb 终端，所以取反
    is_interactive_terminal() && !std::env::var("TERM").unwrap_or_default().contains("dumb")
}

/// ### 创建 spinner 进度条
///
/// 此函数创建一个带 spinner 动画的进度条，用于可视化长时间运行的操作。
///
/// ## 参数说明
///
/// - `message`: 显示在 spinner 旁边的消息
/// - `insert_at_position`: 可选参数，指定在多进度条中的插入位置
///   - `None`: 添加到末尾（默认）
///   - `Some(pos)`: 插入到指定位置
/// - `len`: 可选参数，指定进度条的总长度
///   - `None`: 创建无限 spinner（用于未知进度的操作）
///   - `Some(n)`: 创建有确定长度的进度条，显示 `x / n` 格式
///
/// ## 返回值
///
/// 返回一个 `ProgressBar` 实例，可以用于：
/// - 更新消息：`pb.set_message("新消息")`
/// - 更新进度：`pb.inc(1)` 或 `pb.set_position(50)`
/// - 完成：`pb.finish()` 或 `pb.finish_with_message("完成")`
///
/// ## Spinner 样式
///
/// ### 无限 Spinner（len = None）
/// 模板：`{spinner} {msg}`
/// 显示效果：`⠋ 正在下载...`
///
/// ### 有限进度条（len = Some(n)）
/// 模板：`{spinner} {msg} {pos:.bold} / {len:.dim}`
/// 显示效果：`⠋ 正在处理 5 / 100`
///
/// ## 动画设置
///
/// - 使用 [`TICK_STRINGS`] 作为动画帧
/// - 刷新频率：80 毫秒（约 12.5 FPS）
/// - 通过 `enable_steady_tick()` 自动旋转
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_utils::term::create_spinner;
///
/// // 创建无限 spinner
/// let pb = create_spinner("正在下载文件...".to_string(), None, None);
///
/// // 模拟工作
/// std::thread::sleep(std::time::Duration::from_secs(2));
///
/// // 完成
/// pb.finish_with_message("下载完成！");
///
/// // 创建有限进度条
/// let pb = create_spinner("正在处理...".to_string(), None, Some(100));
/// for i in 0..=100 {
///     pb.set_position(i);
///     std::thread::sleep(std::time::Duration::from_millis(50));
/// }
/// pb.finish_with_message("处理完成！");
/// ```
///
/// ## 多进度条管理
///
/// 当有多个并发任务时，可以使用 `insert_at_position` 参数来组织显示：
///
/// ```rust,ignore
/// let pb1 = create_spinner("任务 1".to_string(), Some(0), None);
/// let pb2 = create_spinner("任务 2".to_string(), Some(1), None);
/// ```
///
/// ## 注意
///
/// 如果在非交互式终端中运行，`indicatif` 库会自动降级为纯文本输出。
pub fn create_spinner(
    message: String,
    insert_at_position: Option<usize>,
    len: Option<u64>,
) -> ProgressBar {
    // 根据 len 参数决定创建 spinner 还是进度条
    let pb = if let Some(len) = len {
        ProgressBar::new(len)  // 创建有限长度的进度条
    } else {
        ProgressBar::new_spinner()  // 创建无限 spinner
    };

    // 根据 insert_at_position 决定如何添加到多进度条管理器
    let pb = if let Some(pos) = insert_at_position {
        MULTI_PROGRESS.insert(pos, pb)  // 插入到指定位置
    } else {
        MULTI_PROGRESS.add(pb)  // 添加到末尾
    };

    // 根据是否有确定长度选择不同的样式模板
    let style = if len.is_some() {
        // 有限进度条样式：显示当前进度/总进度
        ProgressStyle::with_template("{spinner} {msg} {pos:.bold} / {len:.dim}")
            .unwrap()
            .tick_strings(&*TICK_STRINGS)
    } else {
        // 无限 spinner 样式：只显示消息
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap()
            .tick_strings(&*TICK_STRINGS)
    };

    // 应用样式
    pb.set_style(style);
    // 设置消息
    pb.set_message(message);
    // 启用自动旋转，每 80 毫秒更新一次动画帧
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

/// ### 以错误标记结束 spinner
///
/// 此函数用于在操作失败时，以红色叉号（✗）结束 spinner，
/// 而不是通常的绿色对勾（✓）。
///
/// ## 参数说明
///
/// - `pb`: 要结束的进度条
///
/// ## 视觉效果
///
/// 正常完成：`⠏ 正在处理...` → `✓ 处理完成`
/// 操作失败：`⠏ 正在处理...` → `✗ 操作失败`
///
/// ## 实现细节
///
/// 1. 创建一个新的样式，使用 [`ERROR_TICK_STRINGS`] 作为动画帧
/// 2. 应用到进度条
/// 3. 调用 `finish()` 结束进度条
///
/// ## 使用示例
///
/// ```rust
/// use microsandbox_utils::term::{create_spinner, finish_with_error};
///
/// let pb = create_spinner("正在处理...".to_string(), None, None);
///
/// // 模拟可能失败的操作
/// match do_risky_operation() {
///     Ok(result) => {
///         pb.finish_with_message(format!("处理完成：{}", result));
///     }
///     Err(e) => {
///         // 以错误标记结束
///         finish_with_error(&pb);
///         eprintln!("操作失败：{}", e);
///     }
/// }
/// ```
///
/// ## 注意
///
/// 此函数不会打印错误消息，只是改变 spinner 的结束视觉。
/// 错误消息应该单独打印。
pub fn finish_with_error(pb: &ProgressBar) {
    // 创建错误状态的样式，使用错误 tick 字符串
    let style = ProgressStyle::with_template("{spinner} {msg}")
        .unwrap()
        .tick_strings(&*ERROR_TICK_STRINGS);

    // 应用样式
    pb.set_style(style);
    // 结束进度条（会显示最后一帧，即红色叉号）
    pb.finish();
}
