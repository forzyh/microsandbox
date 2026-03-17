//! 路径对
//!
//! 本模块定义了路径对（PathPair）结构，用于在主机和访客系统之间建立路径映射。

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use typed_path::Utf8UnixPathBuf;

use crate::MicrosandboxError;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// 路径对
///
/// 表示主机和访客系统之间的路径映射，遵循 Docker 卷映射的命名约定。
///
/// ## 格式说明
/// 路径对有两种表示格式：
/// - `host:guest` - 将主机路径映射到不同的访客路径（如 "/host/path:/container/path"）
/// - `path` 或 `path:path` - 在主机和访客上映射相同的路径（如 "/data" 或 "/data:/data"）
///
/// ## 变体说明
/// * `Same` - 主机和访客路径相同
/// * `Distinct` - 主机和访客路径不同
///
/// ## 使用示例
///
/// 创建路径对：
/// ```
/// use microsandbox_core::config::PathPair;
/// use typed_path::Utf8UnixPathBuf;
///
/// // 主机和访客上的相同路径（/data:/data）
/// let same_path = PathPair::with_same("/data".into());
///
/// // 不同的路径（主机 /host/data 映射到访客 /container/data）
/// let distinct_paths = PathPair::with_distinct(
///     "/host/data".into(),
///     "/container/data".into()
/// );
///
/// // 从字符串解析
/// let from_str = "/host/data:/container/data".parse::<PathPair>().unwrap();
/// assert_eq!(from_str, distinct_paths);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPair {
    /// 访客路径和主机路径不同
    ///
    /// 用于将主机的某个目录映射到 VM 内的不同位置
    Distinct {
        /// 主机路径
        ///
        /// 主机系统上的目录路径
        host: Utf8UnixPathBuf,

        /// 访客路径
        ///
        /// VM（访客系统）内的目录路径
        guest: Utf8UnixPathBuf,
    },

    /// 访客路径和主机路径相同
    ///
    /// 用于在相同位置挂载目录
    Same(Utf8UnixPathBuf),
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl PathPair {
    /// 创建一个新的 PathPair，主机和访客路径相同
    ///
    /// ## 参数
    /// * `path` - 要使用的路径（同时用于主机和访客）
    pub fn with_same(path: Utf8UnixPathBuf) -> Self {
        Self::Same(path)
    }

    /// 创建一个新的 PathPair，主机和访客路径不同
    ///
    /// ## 参数
    /// * `host` - 主机路径
    /// * `guest` - 访客路径
    pub fn with_distinct(host: Utf8UnixPathBuf, guest: Utf8UnixPathBuf) -> Self {
        Self::Distinct { host, guest }
    }

    /// 获取主机路径
    ///
    /// 返回主机系统上的目录路径引用
    pub fn get_host(&self) -> &Utf8UnixPathBuf {
        match self {
            Self::Distinct { host, .. } | Self::Same(host) => host,
        }
    }

    /// 获取访客路径
    ///
    /// 返回 VM（访客系统）内的目录路径引用
    pub fn get_guest(&self) -> &Utf8UnixPathBuf {
        match self {
            Self::Distinct { guest, .. } | Self::Same(guest) => guest,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl FromStr for PathPair {
    type Err = MicrosandboxError;

    /// 从字符串解析路径对
    ///
    /// 期望格式为 "host:guest" 或单独的 "path"
    ///
    /// ## 参数
    /// * `s` - 要解析的字符串
    ///
    /// ## 返回值
    /// * `Ok(PathPair)` - 解析成功
    /// * `Err(MicrosandboxError::InvalidPathPair)` - 格式无效
    ///
    /// ## 格式说明
    /// * 空字符串 - 错误
    /// * "host:" 或 ":guest" - 错误（部分为空）
    /// * "path" - Same(path)
    /// * "path:path" - 如果相同则为 Same(path)，否则为 Distinct { host, guest }
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 空字符串无效
        if s.is_empty() {
            return Err(MicrosandboxError::InvalidPathPair(s.to_string()));
        }

        // 包含冒号的格式
        if s.contains(':') {
            let (host, guest) = s.split_once(':').unwrap();
            // 主机或访客路径为空时无效
            if guest.is_empty() || host.is_empty() {
                return Err(MicrosandboxError::InvalidPathPair(s.to_string()));
            }

            // 如果路径相同，使用 Same 变体
            if guest == host {
                return Ok(Self::Same(host.into()));
            } else {
                // 路径不同，使用 Distinct 变体
                return Ok(Self::Distinct {
                    host: host.into(),
                    guest: guest.into(),
                });
            }
        }

        // 不包含冒号，只有单个路径，使用 Same 变体
        Ok(Self::Same(s.into()))
    }
}

impl fmt::Display for PathPair {
    /// 格式化路径对为 "host:guest" 格式
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Distinct { host, guest } => {
                write!(f, "{}:{}", host, guest)
            }
            Self::Same(path) => write!(f, "{}:{}", path, path),
        }
    }
}

impl Serialize for PathPair {
    /// 序列化为字符串格式
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PathPair {
    /// 从字符串反序列化
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_pair_from_str() {
        // 测试相同路径
        assert_eq!(
            "/data".parse::<PathPair>().unwrap(),
            PathPair::Same("/data".into())
        );
        assert_eq!(
            "/data:/data".parse::<PathPair>().unwrap(),
            PathPair::Same("/data".into())
        );

        // 测试不同路径（host:guest 格式）
        assert_eq!(
            "/host/data:/container/data".parse::<PathPair>().unwrap(),
            PathPair::Distinct {
                host: "/host/data".into(),
                guest: "/container/data".into()
            }
        );

        // 测试无效格式
        assert!("".parse::<PathPair>().is_err());
        assert!(":".parse::<PathPair>().is_err());
        assert!(":/data".parse::<PathPair>().is_err());
        assert!("/data:".parse::<PathPair>().is_err());
    }

    #[test]
    fn test_path_pair_display() {
        // 测试相同路径
        assert_eq!(PathPair::Same("/data".into()).to_string(), "/data:/data");

        // 测试不同路径（host:guest 格式）
        assert_eq!(
            PathPair::Distinct {
                host: "/host/data".into(),
                guest: "/container/data".into()
            }
            .to_string(),
            "/host/data:/container/data"
        );
    }

    #[test]
    fn test_path_pair_getters() {
        // 测试相同路径
        let same = PathPair::Same("/data".into());
        assert_eq!(same.get_host().as_str(), "/data");
        assert_eq!(same.get_guest().as_str(), "/data");

        // 测试不同路径
        let distinct = PathPair::Distinct {
            host: "/host/data".into(),
            guest: "/container/data".into(),
        };
        assert_eq!(distinct.get_host().as_str(), "/host/data");
        assert_eq!(distinct.get_guest().as_str(), "/container/data");
    }

    #[test]
    fn test_path_pair_constructors() {
        assert_eq!(
            PathPair::with_same("/data".into()),
            PathPair::Same("/data".into())
        );
        assert_eq!(
            PathPair::with_distinct("/host/data".into(), "/container/data".into()),
            PathPair::Distinct {
                host: "/host/data".into(),
                guest: "/container/data".into()
            }
        );
    }
}
