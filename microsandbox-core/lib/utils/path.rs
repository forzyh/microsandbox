//! 路径操作工具函数
//!
//! 本模块提供了与路径处理相关的实用函数。
//! 主要用于路径冲突检测和卷挂载路径的规范化。

use microsandbox_utils::SupportedPathType;

use crate::{MicrosandboxError, MicrosandboxResult};

//--------------------------------------------------------------------------------------------------
// 函数实现
//--------------------------------------------------------------------------------------------------

/// 检查两个路径是否重叠（一个是另一个的父/子路径或两者相同）
///
/// 此函数用于检测卷挂载时的路径冲突。
/// 如果两个挂载点的路径重叠，可能导致文件系统问题。
///
/// ## 参数
/// * `path1` - 第一个路径
/// * `path2` - 第二个路径
///
/// ## 返回值
/// * `true` - 路径重叠（有冲突）
/// * `false` - 路径不重叠（无冲突）
///
/// ## 实现细节
/// 函数会在路径末尾添加 `/` 然后使用前缀匹配来判断重叠关系。
/// 这样可以正确处理以下情况：
/// - `/data` 和 `/data` -> 重叠（相同）
/// - `/data` 和 `/data/app` -> 重叠（父子关系）
/// - `/data` 和 `/database` -> 不重叠（不同路径）
///
/// ## 示例
/// ```
/// assert!(paths_overlap("/data", "/data/app"));
/// assert!(paths_overlap("/data/app", "/data"));
/// assert!(!paths_overlap("/data", "/database"));
/// ```
pub fn paths_overlap(path1: &str, path2: &str) -> bool {
    let path1 = if path1.ends_with('/') {
        path1.to_string()
    } else {
        format!("{}/", path1)
    };

    let path2 = if path2.ends_with('/') {
        path2.to_string()
    } else {
        format!("{}/", path2)
    };

    path1.starts_with(&path2) || path2.starts_with(&path1)
}

/// 规范化和处理卷挂载路径的辅助函数
///
/// 此函数将用户提供的路径转换为规范的绝对路径，
/// 并确保路径在允许的基路径范围内。
///
/// ## 参数
/// * `base_path` - 基路径（必须是绝对路径）
/// * `requested_path` - 用户请求的路径（可以是绝对或相对路径）
///
/// ## 返回值
/// * `Ok(String)` - 规范化后的绝对路径
/// * `Err(MicrosandboxError)` - 路径验证失败
///
/// ## 处理逻辑
/// 1. 首先规范化基路径
/// 2. 如果请求路径是绝对的：
///    - 规范化请求路径
///    - 验证请求路径在基路径之下
/// 3. 如果请求路径是相对的：
///    - 先规范化以捕获 `../` 等路径遍历尝试
///    - 将请求路径与基路径连接
///    - 再次规范化得到最终的绝对路径
///
/// ## 安全考虑
/// 此函数防止路径遍历攻击（如使用 `../../../etc/passwd`），
/// 确保挂载的路径在允许的范围内。
pub fn normalize_volume_path(base_path: &str, requested_path: &str) -> MicrosandboxResult<String> {
    // 首先规范化基路径
    let normalized_base =
        microsandbox_utils::normalize_path(base_path, SupportedPathType::Absolute)?;

    // 如果请求路径是绝对的，验证其在基路径之下
    if requested_path.starts_with('/') {
        let normalized_requested =
            microsandbox_utils::normalize_path(requested_path, SupportedPathType::Absolute)?;
        // 检查规范化后的请求路径是否以基路径开头
        if !normalized_requested.starts_with(&normalized_base) {
            return Err(MicrosandboxError::PathValidation(format!(
                "Absolute path '{}' must be under base path '{}'",
                normalized_requested, normalized_base
            )));
        }
        Ok(normalized_requested)
    } else {
        // 对于相对路径，先规范化以捕获 ../ 等路径遍历尝试
        let normalized_requested =
            microsandbox_utils::normalize_path(requested_path, SupportedPathType::Relative)?;

        // 然后与基路径连接并再次规范化
        let full_path = format!("{}/{}", normalized_base, normalized_requested);
        microsandbox_utils::normalize_path(&full_path, SupportedPathType::Absolute)
            .map_err(Into::into)
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths_overlap() {
        // 应该冲突的测试用例
        assert!(paths_overlap("/data", "/data"));
        assert!(paths_overlap("/data", "/data/app"));
        assert!(paths_overlap("/data/app", "/data"));
        assert!(paths_overlap("/data/app/logs", "/data/app"));

        // 不应该冲突的测试用例
        assert!(!paths_overlap("/data", "/database"));
        assert!(!paths_overlap("/var/log", "/var/lib"));
        assert!(!paths_overlap("/data/app1", "/data/app2"));
        assert!(!paths_overlap("/data/app/logs", "/data/web/logs"));
    }
}
