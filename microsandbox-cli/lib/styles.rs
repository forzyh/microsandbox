//! # ANSI 终端样式模块
//!
//! 本模块负责处理命令行界面的文本样式，包括颜色、粗体等效果。
//! ANSI 转义码是一种在终端中控制文本格式的标准方法。
//!
//! ## 背景知识
//!
//! **什么是 ANSI 转义码？**
//! - ANSI 转义码是嵌入文本中的特殊字符序列，用于控制终端显示
//! - 格式：`\x1b[<code>m`，其中 `\x1b` 是 ESC 字符
//! - 示例：`\x1b[1m` 开启粗体，`\x1b[31m` 设置红色，`\x1b[0m` 重置所有样式
//!
//! **为什么需要检测终端能力？**
//! - 不是所有终端都支持 ANSI 样式（如 Windows 旧版 CMD、重定向输出）
//! - 在不支持的终端中输出 ANSI 码会显示为乱码
//! - 本模块使用 `IS_ANSI_TERMINAL` 在运行时检测终端能力

use clap::builder::styling::{AnsiColor, Effects, Style, Styles};
use std::fmt::Write;

//--------------------------------------------------------------------------------------------------
// Constants - 常量定义
//--------------------------------------------------------------------------------------------------

/// ## ANSI 终端检测标志
///
/// 这是一个懒加载的全局常量，仅在首次使用时计算。
///
/// ### LazyLock 说明
/// - `LazyLock` 是 Rust 标准库的惰性初始化类型
/// - 第一次访问时才执行初始化函数
/// - 线程安全，多次访问返回相同值
///
/// ### `#[cfg(not(test))]` 说明
/// - 这是条件编译属性
/// - 仅在非测试环境下编译此代码
/// - 测试环境使用不同的检测逻辑
#[cfg(not(test))]
/// 全局标志，表示当前是否运行在支持 ANSI 的交互式终端中
static IS_ANSI_TERMINAL: std::sync::LazyLock<bool> =
    std::sync::LazyLock::new(microsandbox_utils::term::is_ansi_interactive_terminal);

//--------------------------------------------------------------------------------------------------
// Functions - 函数定义
//--------------------------------------------------------------------------------------------------

/// ## 返回 CLI 默认样式配置
///
/// 此函数创建一个 `Styles` 对象，定义了 CLI 各部分的显示样式。
/// 使用 `clap` 库的样式系统进行配置。
///
/// ### 样式定义说明
///
/// | 部分 | 颜色 | 效果 | 用途 |
/// |------|------|------|------|
/// | header | 黄色 | 粗体 | 命令帮助信息的标题 |
/// | usage | 黄色 | 粗体 | 命令使用格式 |
/// | literal | 蓝色 | 粗体 | 命令名、参数名等字面量 |
/// | placeholder | 绿色 | 无 | 占位符文本（如 <VALUE>） |
/// | error | 红色 | 粗体 | 错误消息 |
/// | valid | 绿色 | 粗体 | 有效输入提示 |
/// | invalid | 红色 | 粗体 | 无效输入提示 |
///
/// ### `AnsiColor` 枚举
/// - `AnsiColor::Yellow`: 黄色（前景色）
/// - `AnsiColor::Blue`: 蓝色
/// - `AnsiColor::Green`: 绿色
/// - `AnsiColor::Red`: 红色
///
/// ### `.on_default()` 方法
/// - 设置前景色，背景色使用终端默认
///
/// ### `Effects` 枚举
/// - `Effects::BOLD`: 粗体效果
/// - `Effects::ITALIC`: 斜体
/// - `Effects::UNDERLINE`: 下划线
pub fn styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .literal(AnsiColor::Blue.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default())
        .error(AnsiColor::Red.on_default() | Effects::BOLD)
        .valid(AnsiColor::Green.on_default() | Effects::BOLD)
        .invalid(AnsiColor::Red.on_default() | Effects::BOLD)
}

/// ## 内部辅助函数：应用样式到文本
///
/// 此函数将指定的样式应用到文本字符串。
///
/// ### 参数
/// - `text`: 要应用样式的原始文本
/// - `style`: 要应用的样式配置
///
/// ### 返回值
/// 包含 ANSI 转义码的样式化文本
///
/// ### 实现原理
/// 1. 检查终端是否支持 ANSI（非测试环境）
/// 2. 如不支持，直接返回原文本
/// 3. 如支持，在文本前后添加 ANSI 转义码
///
/// ### `write!` 宏的使用
/// - 类似于 `printf` 的格式化输出宏
/// - 写入到 `String` 类型时，返回 `Result<(), std::fmt::Error>`
/// - 使用 `_ = ` 忽略返回值，因为写入 String 不会失败
fn apply_style(text: String, style: &Style) -> String {
    // 非测试环境：检查终端能力
    #[cfg(not(test))]
    if !*IS_ANSI_TERMINAL {
        return text;  // 不支持 ANSI，返回原文本
    }

    // 测试环境：通过 TERM 环境变量判断
    #[cfg(test)]
    {
        if std::env::var("TERM").unwrap_or_default() == "dumb" {
            return text;  // TERM=dumb 表示不支持样式的终端
        }
    }

    // 预分配内存：原文本长度 + 约 20 字节的 ANSI 码
    let mut styled = String::with_capacity(text.len() + 20);

    // 写入开启样式的 ANSI 码
    let _ = write!(styled, "{}", style);

    // 写入原始文本
    styled.push_str(&text);

    // 写入重置样式的 ANSI 码（\x1b[0m）
    let _ = write!(styled, "{}", style.render_reset());

    styled
}

//--------------------------------------------------------------------------------------------------
// Traits - 特征（Trait）定义
//--------------------------------------------------------------------------------------------------

/// ## ANSI 样式扩展特征
///
/// 这是一个 trait（特征/接口），为字符串类型添加了样式方法。
/// Trait 是 Rust 实现接口和多态的主要方式。
///
/// ### 设计模式：扩展方法
/// - 通过实现 trait，为现有类型（String、&str）添加新方法
/// - 类似于其他语言的"扩展方法"或"猴子补丁"
///
/// ### 方法说明
///
/// | 方法 | 用途 |
/// |------|------|
/// | `header()` | 应用标题样式（黄色粗体） |
/// | `usage()` | 应用使用说明样式（黄色粗体） |
/// | `literal()` | 应用字面量样式（蓝色粗体） |
/// | `placeholder()` | 应用占位符样式（绿色） |
/// | `error()` | 应用错误样式（红色粗体） |
/// | `valid()` | 应用有效提示样式（绿色粗体） |
/// | `invalid()` | 应用无效提示样式（红色粗体） |
pub trait AnsiStyles {
    /// 应用标题样式到文本
    fn header(&self) -> String;

    /// 应用使用说明样式到文本
    fn usage(&self) -> String;

    /// 应用字面量样式到文本（如命令名）
    fn literal(&self) -> String;

    /// 应用占位符样式到文本（如 <VALUE>）
    fn placeholder(&self) -> String;

    /// 应用错误样式到文本
    fn error(&self) -> String;

    /// 应用有效提示样式到文本
    fn valid(&self) -> String;

    /// 应用无效提示样式到文本
    fn invalid(&self) -> String;
}

//--------------------------------------------------------------------------------------------------
// Trait Implementations - 特征实现
//--------------------------------------------------------------------------------------------------

/// ## 为 `String` 类型实现 `AnsiStyles` trait
///
/// 这个实现允许直接在 String 上调用样式方法：
/// ```rust,ignore
/// let text = String::from("error");
/// println!("{}", text.error());  // 输出红色粗体的 "error"
/// ```
impl AnsiStyles for String {
    fn header(&self) -> String {
        apply_style(self.clone(), styles().get_header())
    }

    fn usage(&self) -> String {
        apply_style(self.clone(), styles().get_usage())
    }

    fn literal(&self) -> String {
        apply_style(self.clone(), styles().get_literal())
    }

    fn placeholder(&self) -> String {
        apply_style(self.clone(), styles().get_placeholder())
    }

    fn error(&self) -> String {
        apply_style(self.clone(), styles().get_error())
    }

    fn valid(&self) -> String {
        apply_style(self.clone(), styles().get_valid())
    }

    fn invalid(&self) -> String {
        apply_style(self.clone(), styles().get_invalid())
    }
}

/// ## 为 `&str` 类型实现 `AnsiStyles` trait
///
/// 这个实现允许直接在字符串字面量上调用样式方法：
/// ```rust,ignore
/// println!("{}", "error:".error());  // 输出红色粗体的 "error:"
/// ```
///
/// ### 为什么需要两个实现？
/// - `String` 是堆分配的所有权类型
/// - `&str` 是字符串切片（引用）类型
/// - Rust 需要分别为这两种类型实现 trait
/// - 实现中通过 `.to_string()` 将 `&str` 转为 `String` 后复用代码
impl AnsiStyles for &str {
    fn header(&self) -> String {
        self.to_string().header()
    }

    fn usage(&self) -> String {
        self.to_string().usage()
    }

    fn literal(&self) -> String {
        self.to_string().literal()
    }

    fn placeholder(&self) -> String {
        self.to_string().placeholder()
    }

    fn error(&self) -> String {
        self.to_string().error()
    }

    fn valid(&self) -> String {
        self.to_string().valid()
    }

    fn invalid(&self) -> String {
        self.to_string().invalid()
    }
}

//--------------------------------------------------------------------------------------------------
// Tests
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    #[test]
    #[ignore = "this test won't work correctly in cargo-nextest. run with `cargo test -- --ignored`"]
    #[serial]
    fn test_ansi_styles_string_non_interactive() {
        helper::setup_non_interactive();

        let text = String::from("test");
        assert_eq!(text.header(), "test");
        assert_eq!(text.usage(), "test");
        assert_eq!(text.literal(), "test");
        assert_eq!(text.placeholder(), "test");
        assert_eq!(text.error(), "test");
        assert_eq!(text.valid(), "test");
        assert_eq!(text.invalid(), "test");
    }

    #[test]
    #[ignore = "this test won't work correctly in cargo-nextest. run with `cargo test -- --ignored`"]
    #[serial]
    fn test_ansi_styles_str_non_interactive() {
        helper::setup_non_interactive();

        let text = "test";
        assert_eq!(text.header(), "test");
        assert_eq!(text.usage(), "test");
        assert_eq!(text.literal(), "test");
        assert_eq!(text.placeholder(), "test");
        assert_eq!(text.error(), "test");
        assert_eq!(text.valid(), "test");
        assert_eq!(text.invalid(), "test");
    }

    #[test]
    #[ignore = "this test won't work correctly in cargo-nextest. run with `cargo test -- --ignored`"]
    #[serial]
    fn test_ansi_styles_string_interactive() {
        helper::setup_interactive();

        let text = String::from("test");

        let header = text.header();
        println!("header: {}", header);
        // Check for bold and yellow separately
        assert!(header.contains("\x1b[1m"));
        assert!(header.contains("\x1b[33m"));
        assert!(header.contains("test"));
        assert!(header.contains("\x1b[0m"));

        let usage = text.usage();
        println!("usage: {}", usage);
        assert!(usage.contains("\x1b[1m"));
        assert!(usage.contains("\x1b[33m"));
        assert!(usage.contains("test"));
        assert!(usage.contains("\x1b[0m"));

        let literal = text.literal();
        println!("literal: {}", literal);
        assert!(literal.contains("\x1b[1m"));
        assert!(literal.contains("\x1b[34m"));
        assert!(literal.contains("test"));
        assert!(literal.contains("\x1b[0m"));

        let placeholder = text.placeholder();
        println!("placeholder: {}", placeholder);
        // For placeholder, no bold is expected
        assert!(placeholder.contains("\x1b[32m"));
        assert!(placeholder.contains("test"));
        assert!(placeholder.contains("\x1b[0m"));

        let error = text.error();
        println!("error: {}", error);
        assert!(error.contains("\x1b[1m"));
        assert!(error.contains("\x1b[31m"));
        assert!(error.contains("test"));
        assert!(error.contains("\x1b[0m"));

        let valid = text.valid();
        println!("valid: {}", valid);
        assert!(valid.contains("\x1b[1m"));
        assert!(valid.contains("\x1b[32m"));
        assert!(valid.contains("test"));
        assert!(valid.contains("\x1b[0m"));

        let invalid = text.invalid();
        println!("invalid: {}", invalid);
        assert!(invalid.contains("\x1b[1m"));
        assert!(invalid.contains("\x1b[31m"));
        assert!(invalid.contains("test"));
        assert!(invalid.contains("\x1b[0m"));
    }

    #[test]
    #[ignore = "this test won't work correctly in cargo-nextest. run with `cargo test -- --ignored`"]
    #[serial]
    fn test_ansi_styles_str_interactive() {
        helper::setup_interactive();

        let text = "test";

        let header = text.header();
        assert!(header.contains("\x1b[1m"));
        assert!(header.contains("\x1b[33m"));
        assert!(header.contains("test"));
        assert!(header.contains("\x1b[0m"));

        let usage = text.usage();
        assert!(usage.contains("\x1b[1m"));
        assert!(usage.contains("\x1b[33m"));
        assert!(usage.contains("test"));
        assert!(usage.contains("\x1b[0m"));

        let literal = text.literal();
        assert!(literal.contains("\x1b[1m"));
        assert!(literal.contains("\x1b[34m"));
        assert!(literal.contains("test"));
        assert!(literal.contains("\x1b[0m"));

        let placeholder = text.placeholder();
        assert!(placeholder.contains("\x1b[32m"));
        assert!(placeholder.contains("test"));
        assert!(placeholder.contains("\x1b[0m"));

        let error = text.error();
        assert!(error.contains("\x1b[1m"));
        assert!(error.contains("\x1b[31m"));
        assert!(error.contains("test"));
        assert!(error.contains("\x1b[0m"));

        let valid = text.valid();
        assert!(valid.contains("\x1b[1m"));
        assert!(valid.contains("\x1b[32m"));
        assert!(valid.contains("test"));
        assert!(valid.contains("\x1b[0m"));

        let invalid = text.invalid();
        assert!(invalid.contains("\x1b[1m"));
        assert!(invalid.contains("\x1b[31m"));
        assert!(invalid.contains("test"));
        assert!(invalid.contains("\x1b[0m"));
    }

    #[test]
    #[ignore = "this test won't work correctly in cargo-nextest. run with `cargo test -- --ignored`"]
    #[serial]
    fn test_ansi_styles_empty_string() {
        helper::setup_interactive();

        let empty = String::new();
        assert!(empty.header().ends_with("\x1b[0m"));
        assert!(empty.usage().ends_with("\x1b[0m"));
        assert!(empty.literal().ends_with("\x1b[0m"));
        assert!(empty.placeholder().ends_with("\x1b[0m"));
        assert!(empty.error().ends_with("\x1b[0m"));
        assert!(empty.valid().ends_with("\x1b[0m"));
        assert!(empty.invalid().ends_with("\x1b[0m"));

        helper::setup_non_interactive();
        assert_eq!(empty.header(), "");
        assert_eq!(empty.usage(), "");
        assert_eq!(empty.literal(), "");
        assert_eq!(empty.placeholder(), "");
        assert_eq!(empty.error(), "");
        assert_eq!(empty.valid(), "");
        assert_eq!(empty.invalid(), "");
    }

    #[test]
    #[ignore = "this test won't work correctly in cargo-nextest. run with `cargo test -- --ignored`"]
    #[serial]
    fn test_ansi_styles_unicode_string() {
        helper::setup_interactive();

        let text = "测试";
        let header = text.header();
        assert!(header.contains("测试"));
        assert!(header.starts_with("\x1b["));
        assert!(header.ends_with("\x1b[0m"));
    }
}

#[cfg(test)]
mod helper {
    use std::env;

    /// Helper function to set up a non-interactive terminal environment
    pub(super) fn setup_non_interactive() {
        // Safety: this is used solely for tests
        unsafe { env::set_var("TERM", "dumb") };
    }

    /// Helper function to set up an interactive terminal environment
    pub(super) fn setup_interactive() {
        // Safety: this is used solely for tests
        unsafe { env::set_var("TERM", "xterm-256color") };
    }
}
