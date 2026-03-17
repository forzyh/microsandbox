//! 类型转换工具函数
//!
//! 本模块提供了在不同数据类型之间进行转换的实用函数。
//! 主要用于范围边界转换、FFI 字符串数组创建和文件权限格式化。

use std::{
    ffi::{CString, c_char},
    ops::{Bound, RangeBounds},
};

//--------------------------------------------------------------------------------------------------
// 函数实现
//--------------------------------------------------------------------------------------------------

/// 将范围边界转换为 u64 的起始和结束值
///
/// 此函数用于将 Rust 的范围语法（如 `1..10`、`..10`、`1..`）
/// 转换为具体的起始和结束数值，常用于数据库查询或分页操作。
///
/// ## 参数
/// * `range` - 实现 RangeBounds<u64> 的任何范围类型
///
/// ## 返回值
/// 返回 (start, end) 元组，其中：
/// - `start` - 范围的起始值（包含）
/// - `end` - 范围的结束值（包含）
///
/// ## 示例
///
/// ```
/// use microsandbox_core::utils::convert_bounds;
///
/// // 包含两端：1..10 -> [1, 9]
/// let (start, end) = convert_bounds(1..10);
/// assert_eq!(start, 1);
/// assert_eq!(end, 9);
///
/// // 从开头到 10：..10 -> [0, 9]
/// let (start, end) = convert_bounds(..10);
/// assert_eq!(start, 0);
/// assert_eq!(end, 9);
///
/// // 从 1 到末尾：1.. -> [1, u64::MAX]
/// let (start, end) = convert_bounds(1..);
/// assert_eq!(start, 1);
/// assert_eq!(end, u64::MAX);
///
/// // 包含右端：..=10 -> [0, 10]
/// let (start, end) = convert_bounds(..=10);
/// assert_eq!(start, 0);
/// assert_eq!(end, 10);
/// ```
///
/// ## 边界处理规则
/// - `Bound::Included(n)` -> 使用 n 作为边界值
/// - `Bound::Excluded(n)` -> 使用 n+1（起始）或 n-1（结束）
/// - `Bound::Unbounded` -> 使用 0（起始）或 u64::MAX（结束）
pub fn convert_bounds(range: impl RangeBounds<u64>) -> (u64, u64) {
    let start = match range.start_bound() {
        Bound::Included(&start) => start,
        Bound::Excluded(&start) => start + 1,
        Bound::Unbounded => 0,
    };

    let end = match range.end_bound() {
        Bound::Included(&end) => end,
        Bound::Excluded(&end) => end - 1,
        Bound::Unbounded => u64::MAX,
    };

    (start, end)
}

/// 从字符串切片创建空终止的 C 字符串指针数组
///
/// 此函数用于 FFI 调用，特别是需要将 Rust 字符串数组
/// 传递给 C API 时。许多 C API 期望以 NULL 指针结尾的字符串数组。
///
/// ## 参数
/// * `strings` - CString 的切片
///
/// ## 返回值
/// 返回包含 C 字符串指针的向量，末尾添加了 NULL 指针。
///
/// ## 安全性
/// 返回的指针只在 `strings` 向量存活期间有效。
/// 调用者必须确保原始 CString 数据在使用指针期间不被释放。
///
/// ## 使用示例
/// ```
/// use std::ffi::CString;
/// use microsandbox_core::utils::to_null_terminated_c_array;
///
/// let strings = vec![
///     CString::new("arg1").unwrap(),
///     CString::new("arg2").unwrap(),
/// ];
/// let ptrs = to_null_terminated_c_array(&strings);
/// // ptrs 现在可以传递给期望 char** 的 C 函数
/// ```
pub fn to_null_terminated_c_array(strings: &[CString]) -> Vec<*const c_char> {
    let mut ptrs: Vec<*const c_char> = strings.iter().map(|s| s.as_ptr()).collect();
    ptrs.push(std::ptr::null());

    ptrs
}

/// 将文件权限模式转换为字符串表示形式（类似 ls -l 的输出）
///
/// 此函数将 Unix 文件权限的八进制表示（如 0o755）
/// 转换为人类可读的字符串格式（如 "-rwxr-xr-x"）。
///
/// ## 参数
/// * `mode` - 文件权限模式（八进制）
///
/// ## 返回值
/// 返回 10 字符的字符串，格式为：
/// - 第 1 位：文件类型（d=目录，l=链接，p=管道，s=套接字，b=块设备，c=字符设备，-=普通文件）
/// - 第 2-4 位：所有者权限（rwx）
/// - 第 5-7 位：组权限（rwx）
/// - 第 8-10 位：其他用户权限（rwx）
///
/// ## 示例
/// ```
/// use microsandbox_core::utils::format_mode;
/// assert_eq!(format_mode(0o755), "-rwxr-xr-x");
/// assert_eq!(format_mode(0o644), "-rw-r--r--");
/// assert_eq!(format_mode(0o40755), "drwxr-xr-x");
/// ```
///
/// ## 文件类型掩码
/// - `0o040000` - 目录（directory）
/// - `0o120000` - 符号链接（symbolic link）
/// - `0o010000` - 命名管道（FIFO）
/// - `0o140000` - 套接字（socket）
/// - `0o060000` - 块设备（block device）
/// - `0o020000` - 字符设备（character device）
/// - 其他 - 普通文件
pub fn format_mode(mode: u32) -> String {
    let file_type = match mode & 0o170000 {
        0o040000 => 'd', // directory
        0o120000 => 'l', // symbolic link
        0o010000 => 'p', // named pipe (FIFO)
        0o140000 => 's', // socket
        0o060000 => 'b', // block device
        0o020000 => 'c', // character device
        _ => '-',        // regular file
    };

    let user = format_triplet((mode >> 6) & 0o7);
    let group = format_triplet((mode >> 3) & 0o7);
    let other = format_triplet(mode & 0o7);

    format!("{}{}{}{}", file_type, user, group, other)
}

/// 将权限三元组（3 位）转换为 rwx 格式的辅助函数
///
/// ## 参数
/// * `mode` - 3 位权限值（0-7）
///
/// ## 返回值
/// 返回 3 字符字符串，如 "rwx"、"r-x"、"r--" 等
///
/// ## 位掩码
/// - `0o4` - 读权限（read）
/// - `0o2` - 写权限（write）
/// - `0o1` - 执行权限（execute）
fn format_triplet(mode: u32) -> String {
    let r = if mode & 0o4 != 0 { 'r' } else { '-' };
    let w = if mode & 0o2 != 0 { 'w' } else { '-' };
    let x = if mode & 0o1 != 0 { 'x' } else { '-' };
    format!("{}{}{}", r, w, x)
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_mode() {
        assert_eq!(format_mode(0o755), "-rwxr-xr-x");
        assert_eq!(format_mode(0o644), "-rw-r--r--");
        assert_eq!(format_mode(0o40755), "drwxr-xr-x");
        assert_eq!(format_mode(0o100644), "-rw-r--r--");
        assert_eq!(format_mode(0o120777), "lrwxrwxrwx");
        assert_eq!(format_mode(0o010644), "prw-r--r--");
    }
}
