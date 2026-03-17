//! 路径段
//!
//! 本模块定义了路径段（PathSegment）结构，用于表示和处理单个路径组件。

use std::{
    ffi::{OsStr, OsString},
    fmt::{self, Display},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use crate::MicrosandboxError;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// 路径段
///
/// 此结构表示路径的单个组件，确保其为有效的路径段。
/// 路径段不能包含路径分隔符（Unix 上的 '/' 或 Windows 上的 '/' 和 '\\'），
/// 也不能是特殊组件（如 "." 或 ".."）。
///
/// ## 使用示例
/// ```
/// use microsandbox_core::config::PathSegment;
///
/// // 从字符串创建路径段
/// let segment: PathSegment = "example".parse().unwrap();
/// assert_eq!(segment.to_string(), "example");
///
/// // 访问底层字符串
/// assert_eq!(segment.as_os_str(), std::ffi::OsStr::new("example"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathSegment(OsString);

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl PathSegment {
    /// 返回段的 OS 字符串引用
    ///
    /// ## 返回值
    /// `&OsStr` - 底层操作系统字符串的引用
    pub fn as_os_str(&self) -> &OsStr {
        &self.0
    }

    /// 返回段的字节表示
    ///
    /// ## 返回值
    /// `&[u8]` - 编码后的字节切片
    pub fn as_bytes(&self) -> &[u8] {
        self.as_os_str().as_encoded_bytes()
    }

    /// 返回段的长度（字节数）
    ///
    /// ## 返回值
    /// `usize` - 段的字节长度
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// 检查段是否为空
    ///
    /// ## 返回值
    /// `bool` - 如果段为空则返回 true
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl FromStr for PathSegment {
    type Err = MicrosandboxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PathSegment::try_from(s)
    }
}

impl Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.to_string_lossy())
    }
}

impl TryFrom<&str> for PathSegment {
    type Error = MicrosandboxError;

    /// 从字符串尝试创建路径段
    ///
    /// 验证字符串是否为有效的路径段：
    /// - 不能为空
    /// - 不能包含路径分隔符（Unix: '/'，Windows: '/' 或 '\\'）
    /// - 不能是特殊组件（如 "." 或 ".."）
    ///
    /// ## 参数
    /// * `value` - 要转换的字符串
    ///
    /// ## 返回值
    /// * `Ok(PathSegment)` - 转换成功
    /// * `Err(MicrosandboxError::EmptyPathSegment)` - 字符串为空
    /// * `Err(MicrosandboxError::InvalidPathComponent)` - 包含分隔符或是特殊组件
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        // 空字符串无效
        if value.is_empty() {
            return Err(MicrosandboxError::EmptyPathSegment);
        }

        // Unix 系统：检查是否包含 '/'
        #[cfg(unix)]
        {
            if value.contains('/') {
                return Err(MicrosandboxError::InvalidPathComponent(value.to_string()));
            }
        }

        // Windows 系统：检查是否包含 '/' 或 '\\'
        #[cfg(windows)]
        {
            if value.contains('/') || value.contains('\\') {
                return Err(MicrosandboxError::InvalidPathComponent(value.to_string()));
            }
        }

        // 此时字符串不包含任何分隔符
        // 使用 Path::components() 验证是否为有效的路径组件
        let mut components = Path::new(value).components();
        let component = components
            .next()
            .ok_or_else(|| MicrosandboxError::InvalidPathComponent(value.to_string()))?;

        // 确保没有额外的组件
        if components.next().is_some() {
            return Err(MicrosandboxError::InvalidPathComponent(value.to_string()));
        }

        // 只接受 Normal 组件（非特殊组件）
        match component {
            Component::Normal(comp) => Ok(PathSegment(comp.to_os_string())),
            _ => Err(MicrosandboxError::InvalidPathComponent(value.to_string())),
        }
    }
}

impl<'a> TryFrom<Component<'a>> for PathSegment {
    type Error = MicrosandboxError;

    fn try_from(component: Component<'a>) -> Result<Self, Self::Error> {
        PathSegment::try_from(&component)
    }
}

impl<'a> TryFrom<&Component<'a>> for PathSegment {
    type Error = MicrosandboxError;

    /// 从 Path 组件尝试创建路径段
    ///
    /// 只接受 `Component::Normal` 类型的组件
    ///
    /// ## 参数
    /// * `component` - 要转换的 Path 组件
    ///
    /// ## 返回值
    /// * `Ok(PathSegment)` - 转换成功
    /// * `Err(MicrosandboxError::InvalidPathComponent)` - 组件类型无效
    fn try_from(component: &Component<'a>) -> Result<Self, Self::Error> {
        match component {
            Component::Normal(component) => Ok(PathSegment(component.to_os_string())),
            _ => Err(MicrosandboxError::InvalidPathComponent(
                component.as_os_str().to_string_lossy().into_owned(),
            )),
        }
    }
}

impl<'a> From<&'a PathSegment> for Component<'a> {
    /// 将路径段引用转换为 Path 组件
    ///
    /// 总是返回 `Component::Normal` 类型
    fn from(segment: &'a PathSegment) -> Self {
        Component::Normal(segment.as_os_str())
    }
}

impl From<PathSegment> for PathBuf {
    /// 将路径段转换为 PathBuf
    #[inline]
    fn from(segment: PathSegment) -> Self {
        PathBuf::from(segment.0)
    }
}

impl AsRef<[u8]> for PathSegment {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsRef<OsStr> for PathSegment {
    #[inline]
    fn as_ref(&self) -> &OsStr {
        self.as_os_str()
    }
}

impl AsRef<Path> for PathSegment {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(self.as_os_str())
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_as_os_str() {
        let segment = PathSegment::from_str("example").unwrap();
        assert_eq!(segment.as_os_str(), OsStr::new("example"));
    }

    #[test]
    fn test_segment_as_bytes() {
        let segment = PathSegment::from_str("example").unwrap();
        assert_eq!(segment.as_bytes(), b"example");
    }

    #[test]
    fn test_segment_len() {
        let segment = PathSegment::from_str("example").unwrap();
        assert_eq!(segment.len(), 7);
    }

    #[test]
    fn test_segment_display() {
        let segment = PathSegment::from_str("example").unwrap();
        assert_eq!(format!("{}", segment), "example");
    }

    #[test]
    fn test_segment_try_from_str() {
        // 有效输入
        assert!(PathSegment::try_from("example").is_ok());
        assert!(PathSegment::from_str("example").is_ok());
        assert!("example".parse::<PathSegment>().is_ok());

        // 无效输入
        assert!(PathSegment::from_str("").is_err());
        assert!(PathSegment::from_str(".").is_err());
        assert!(PathSegment::from_str("..").is_err());
        assert!(".".parse::<PathSegment>().is_err());
        assert!("..".parse::<PathSegment>().is_err());
        assert!("".parse::<PathSegment>().is_err());
        assert!(PathSegment::try_from(".").is_err());
        assert!(PathSegment::try_from("..").is_err());
        assert!(PathSegment::try_from("/").is_err());
        assert!(PathSegment::try_from("").is_err());
    }

    #[test]
    fn test_segment_from_path_segment_to_component() {
        let segment = PathSegment::from_str("example").unwrap();
        assert_eq!(
            Component::from(&segment),
            Component::Normal(OsStr::new("example"))
        );
    }

    #[test]
    fn test_segment_from_path_segment_to_path_buf() {
        let segment = PathSegment::from_str("example").unwrap();
        assert_eq!(PathBuf::from(segment), PathBuf::from("example"));
    }

    #[test]
    fn test_segment_normal_with_special_characters() {
        // 带特殊字符的有效段
        assert!(PathSegment::try_from("file.txt").is_ok());
        assert!(PathSegment::try_from("file-name").is_ok());
        assert!(PathSegment::try_from("file_name").is_ok());
        assert!(PathSegment::try_from("file name").is_ok());
        assert!(PathSegment::try_from("file:name").is_ok());
        assert!(PathSegment::try_from("file*name").is_ok());
        assert!(PathSegment::try_from("file?name").is_ok());
    }

    #[test]
    #[cfg(unix)]
    fn test_segment_with_unix_separator() {
        // Unix 系统上，'/' 是分隔符，不能出现在段中
        assert!(PathSegment::try_from("file/name").is_err());
        assert!(PathSegment::try_from("/").is_err());
        assert!(PathSegment::try_from("///").is_err());
        assert!(PathSegment::try_from("name/").is_err());
        assert!(PathSegment::try_from("/name").is_err());
    }

    #[test]
    #[cfg(windows)]
    fn test_segment_with_windows_separators() {
        // Windows 系统上，'/' 和 '\\' 都是分隔符
        assert!(PathSegment::try_from("file\\name").is_err());
        assert!(PathSegment::try_from("file/name").is_err());
        assert!(PathSegment::try_from("\\").is_err());
        assert!(PathSegment::try_from("/").is_err());
        assert!(PathSegment::try_from("\\\\\\").is_err());
        assert!(PathSegment::try_from("///").is_err());
        assert!(PathSegment::try_from("name\\").is_err());
        assert!(PathSegment::try_from("name/").is_err());
        assert!(PathSegment::try_from("\\name").is_err());
        assert!(PathSegment::try_from("/name").is_err());
    }
}
