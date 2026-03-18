//! Microsandbox 工具链管理模块。
//!
//! # 概述
//!
//! 本模块提供了 Microsandbox 工具链的管理功能，包括：
//! - **清理（clean）**：清理用户安装的 microsandbox 脚本
//! - **卸载（uninstall）**：完全卸载 Microsandbox 工具链
//!
//! 该模块主要处理构成 Microsandbox 运行时的二进制文件和库文件。
//!
//! # 架构设计
//!
//! Microsandbox 工具链安装在用户的本地目录中，遵循 XDG 基础目录规范：
//! - **可执行文件**：安装在 `~/.local/bin` 目录
//!   - `msb` - Microsandbox 主命令行工具
//!   - `msbrun` - 沙箱运行器
//!   - `msr` - 运行时工具
//!   - `msx` - 执行工具
//!   - `msi` - 安装工具
//!   - `msbserver` - 服务器模式工具
//! - **库文件**：安装在 `~/.local/lib` 目录
//!   - `libkrun` - KRun 虚拟机库
//!   - `libkrunfw` - KRun 固件库
//!
//! # 使用示例
//!
//! ```no_run
//! use microsandbox_core::management::toolchain;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // 清理用户安装的脚本
//! toolchain::clean().await?;
//!
//! // 完全卸载工具链
//! toolchain::uninstall().await?;
//! # Ok(())
//! # }
//! ```

use microsandbox_utils::{XDG_BIN_DIR, XDG_HOME_DIR, XDG_LIB_DIR};
use std::path::{Path, PathBuf};
use tokio::fs;

use crate::MicrosandboxResult;

//--------------------------------------------------------------------------------------------------
// 常量定义
//--------------------------------------------------------------------------------------------------

/// 核心工具链可执行文件列表（这些文件不会被 clean() 删除）
const CORE_EXECUTABLES: [&str; 3] = ["msi", "msx", "msr"];

//--------------------------------------------------------------------------------------------------
// 公开函数
//--------------------------------------------------------------------------------------------------

/// 清理用户安装的 microsandbox 脚本。
///
/// # 功能说明
///
/// 此函数会移除 `~/.local/bin` 目录中所有包含 `MSB-ALIAS` 标记的脚本文件，
/// 但会保留核心工具链脚本（`msi`, `msx`, `msr`）。
///
/// ## 什么是 MSB-ALIAS 标记？
///
/// `MSB-ALIAS` 是 Microsandbox 用来标识用户自定义脚本的标记。当用户使用
/// `msb alias` 命令创建命令别名时，会在生成的脚本文件中添加此标记。
/// 这样可以区分核心系统脚本和用户自定义脚本。
///
/// ## 工作原理
///
/// 1. 获取 `~/.local/bin` 目录路径
/// 2. 遍历目录中的所有文件
/// 3. 跳过受保护的核心可执行文件（msi, msx, msr）
/// 4. 读取每个文件内容，检查是否包含 `# MSB-ALIAS:` 标记
/// 5. 如果包含标记，则删除该文件
///
/// ## 示例
///
/// ```no_run
/// use microsandbox_core::management::toolchain;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 清理所有用户安装的脚本
/// toolchain::clean().await?;
/// # Ok(())
/// # }
/// ```
pub async fn clean() -> MicrosandboxResult<()> {
    // 获取 bin 目录路径：~/.local/bin
    let bin_dir = XDG_HOME_DIR.join(XDG_BIN_DIR);

    // 清理所有包含 MSB-ALIAS 标记的用户脚本
    clean_user_scripts(&bin_dir).await?;

    Ok(())
}

/// 卸载 Microsandbox 工具链。
///
/// # 功能说明
///
/// 此函数会从用户系统中完全移除所有与 Microsandbox 相关的文件，包括：
///
/// ## 移除的可执行文件（位于 ~/.local/bin）
/// - `msb` - 主命令行工具
/// - `msbrun` - 沙箱运行器
/// - `msr` - 运行时工具
/// - `msx` - 执行工具
/// - `msi` - 安装工具
/// - `msbserver` - 服务器模式工具
///
/// ## 移除的库文件（位于 ~/.local/lib）
/// - `libkrun.dylib` / `libkrun.so` - KRun 虚拟机库（macOS / Linux）
/// - `libkrunfw.dylib` / `libkrunfw.so` - KRun 固件库（macOS / Linux）
/// - 版本化的库文件（如 `libkrun.1.0.0.dylib`）
///
/// ## 注意事项
///
/// - 此操作是不可逆的，卸载后需要重新安装才能使用 Microsandbox
/// - 只移除工具链文件，不会影响项目的 `.menv` 环境目录
/// - 如果某些文件不存在，会记录日志但不会报错
///
/// ## 示例
///
/// ```no_run
/// use microsandbox_core::management::toolchain;
///
/// # async fn example() -> anyhow::Result<()> {
/// // 完全卸载 Microsandbox 工具链
/// toolchain::uninstall().await?;
/// # Ok(())
/// # }
/// ```
pub async fn uninstall() -> MicrosandboxResult<()> {
    // 获取 bin 目录路径：~/.local/bin
    let bin_dir = XDG_HOME_DIR.join(XDG_BIN_DIR);

    // 卸载可执行文件
    uninstall_executables(&bin_dir).await?;

    // 卸载库文件
    uninstall_libraries().await?;

    // 记录成功日志
    tracing::info!("microsandbox toolchain has been successfully uninstalled");

    Ok(())
}

//--------------------------------------------------------------------------------------------------
// 内部辅助函数
//--------------------------------------------------------------------------------------------------

/// 从用户系统中卸载 Microsandbox 可执行文件。
///
/// # 参数
///
/// * `bin_dir` - bin 目录路径（通常是 `~/.local/bin`）
///
/// # 实现细节
///
/// 此函数会：
/// 1. 定义要移除的可执行文件列表
/// 2. 遍历每个文件
/// 3. 检查文件是否存在
/// 4. 如果存在则删除，并记录日志
/// 5. 如果不存在，仅记录日志（不报错）
async fn uninstall_executables(bin_dir: &Path) -> MicrosandboxResult<()> {
    // 要移除的可执行文件列表
    let executables = ["msb", "msbrun", "msr", "msx", "msi", "msbserver"];

    // 遍历并删除每个可执行文件
    for executable in executables {
        let executable_path = bin_dir.join(executable);
        if executable_path.exists() {
            fs::remove_file(&executable_path).await?;
            tracing::info!("removed executable: {}", executable_path.display());
        } else {
            tracing::info!("executable not found: {}", executable_path.display());
        }
    }

    Ok(())
}

/// 从用户系统中卸载 Microsandbox 库文件。
///
/// # 实现细节
///
/// 库文件的卸载分为两步：
///
/// 1. **移除基础库符号链接**：
///    - `libkrun.dylib` / `libkrun.so`
///    - `libkrunfw.dylib` / `libkrunfw.so`
///
/// 2. **移除版本化的库文件**：
///    - 匹配 `libkrun.*.dylib` 或 `libkrun.*.so` 模式的文件
///    - 匹配 `libkrunfw.*.dylib` 或 `libkrunfw.*.so` 模式的文件
///
/// # 跨平台说明
///
/// - **macOS**：使用 `.dylib` 扩展名
/// - **Linux**：使用 `.so` 扩展名
///
/// 此函数同时支持两个平台，会自动处理两种格式的文件。
async fn uninstall_libraries() -> MicrosandboxResult<()> {
    // 获取 lib 目录路径：~/.local/lib
    let lib_dir = XDG_HOME_DIR.join(XDG_LIB_DIR);

    // 首先移除基础库符号链接
    remove_if_exists(lib_dir.join("libkrun.dylib")).await?;
    remove_if_exists(lib_dir.join("libkrunfw.dylib")).await?;
    remove_if_exists(lib_dir.join("libkrun.so")).await?;
    remove_if_exists(lib_dir.join("libkrunfw.so")).await?;

    // 移除版本化的库文件
    uninstall_versioned_libraries(&lib_dir, "libkrun").await?;
    uninstall_versioned_libraries(&lib_dir, "libkrunfw").await?;

    Ok(())
}

/// 如果文件存在则删除，如果不存在则忽略。
///
/// # 参数
///
/// * `path` - 要删除的文件路径
///
/// # 返回值
///
/// - 成功时返回 `Ok(())`
/// - 如果文件存在但删除失败，返回错误
/// - 如果文件不存在，直接返回 `Ok(())`
///
/// # 设计说明
///
/// 这是一个辅助函数，用于"尽力删除"场景：
/// - 我们期望文件可能存在，但不强求
/// - 文件不存在不是错误，只是记录调试日志
/// - 这种模式在清理/卸载操作中很常见
async fn remove_if_exists(path: PathBuf) -> MicrosandboxResult<()> {
    if path.exists() {
        fs::remove_file(&path).await?;
        tracing::info!("removed library: {}", path.display());
    } else {
        tracing::debug!("library not found: {}", path.display());
    }
    Ok(())
}

/// 卸载匹配指定前缀模式的版本化库文件。
///
/// # 参数
///
/// * `lib_dir` - 库目录路径
/// * `lib_prefix` - 库文件名前缀（如 "libkrun"）
///
/// # 匹配规则
///
/// 此函数会匹配以下模式的文件：
///
/// 1. **dylib 格式（macOS）**：
///    - 文件名以 `{lib_prefix}.` 开头
///    - 文件名以 `.dylib` 结尾
///    - 例如：`libkrun.1.0.0.dylib`
///
/// 2. **so 格式（Linux）**：
///    - 文件名以 `{lib_prefix}.` 开头
///    - 或文件名以 `{lib_prefix}.so.` 开头
///    - 例如：`libkrun.so.1.0.0` 或 `libkrun.1.0.0.so`
///
/// # 实现细节
///
/// 使用 `read_dir` 流式读取目录，逐个检查文件名：
/// - 不一次性加载所有目录项到内存，适合大目录
/// - 使用字符串匹配而非正则表达式，性能更好
async fn uninstall_versioned_libraries(lib_dir: &Path, lib_prefix: &str) -> MicrosandboxResult<()> {
    // 获取目录项迭代器
    let mut entries = fs::read_dir(lib_dir).await?;

    // 处理每个目录项
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
            // 检查是否是目标版本化库文件

            // dylib 格式检查：libkrun.1.0.0.dylib
            let is_dylib =
                filename.starts_with(&format!("{}.", lib_prefix)) && filename.ends_with(".dylib");

            // so 格式检查：libkrun.so.1.0.0 或 libkrun.1.0.0
            let is_so = filename.starts_with(&format!("{}.", lib_prefix))
                || filename.starts_with(&format!("{}.so.", lib_prefix));

            // 如果匹配任一格式，删除文件
            if is_dylib || is_so {
                fs::remove_file(&path).await?;
                tracing::info!("removed versioned library: {}", path.display());
            }
        }
    }

    Ok(())
}

/// 清理指定 bin 目录中所有包含 MSB-ALIAS 标记的用户脚本。
///
/// # 参数
///
/// * `bin_dir` - bin 目录路径
///
/// # 实现细节
///
/// 1. 如果 bin 目录不存在，直接返回成功（记录日志）
/// 2. 定义受保护的可执行文件列表（核心工具链）
/// 3. 遍历目录中的所有文件：
///    - 跳过目录和非普通文件
///    - 跳过受保护的核心可执行文件
///    - 读取文件内容，检查是否包含 `# MSB-ALIAS:` 标记
///    - 如果包含标记，删除文件并计数
/// 4. 记录清理统计信息
///
/// # 安全说明
///
/// 此函数只删除明确标记为用户脚本的文件，不会影响：
/// - 核心工具链可执行文件（msi, msx, msr）
/// - 没有 MSB-ALIAS 标记的文件
/// - 目录和符号链接
async fn clean_user_scripts(bin_dir: &Path) -> MicrosandboxResult<()> {
    // 如果 bin 目录不存在，提前返回
    if !bin_dir.exists() {
        tracing::info!("bin directory not found: {}", bin_dir.display());
        return Ok(());
    }

    // 受保护的核心可执行文件（不会被 clean 删除）
    let protected_executables = ["msi", "msx", "msr"];

    // 获取 bin 目录中的所有文件
    let mut entries = fs::read_dir(bin_dir).await?;
    let mut removed_count = 0;

    // 检查每个文件是否包含 MSB-ALIAS 标记
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // 跳过目录和非普通文件
        if !path.is_file() {
            continue;
        }

        // 跳过受保护的核心可执行文件
        if let Some(filename) = path.file_name().and_then(|f| f.to_str())
            && protected_executables.contains(&filename)
        {
            tracing::debug!("skipping protected executable: {}", filename);
            continue;
        }

        // 读取文件内容并检查是否包含 MSB-ALIAS 标记
        // 使用 if let Ok(...) 模式：读取失败时静默跳过
        if let Ok(content) = fs::read_to_string(&path).await
            && content.contains("# MSB-ALIAS:")
        {
            // 这是一个 microsandbox 别名脚本，删除它
            fs::remove_file(&path).await?;
            tracing::info!("removed user script: {}", path.display());
            removed_count += 1;
        }
    }

    // 记录清理统计
    tracing::info!(
        "removed {} user scripts with MSB-ALIAS markers",
        removed_count
    );

    Ok(())
}
