//! 端口对
//!
//! 本模块定义了端口对（PortPair）结构，用于在主机和访客系统之间建立端口映射。

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::MicrosandboxError;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// 端口对
///
/// 表示主机和访客系统之间的端口映射，遵循 Docker 端口映射的命名约定。
///
/// ## 格式说明
/// 端口对有两种表示格式：
/// - `host:guest` - 将主机端口映射到不同的访客端口（如 "8080:80"）
/// - `port` 或 `port:port` - 在主机和访客上映射相同的端口号（如 "8080" 或 "8080:8080"）
///
/// ## 变体说明
/// * `Same(u16)` - 主机和访客端口相同
/// * `Distinct { host: u16, guest: u16 }` - 主机和访客端口不同
///
/// ## 使用示例
///
/// 创建端口对：
/// ```
/// use microsandbox_core::config::PortPair;
///
/// // 主机和访客上的相同端口（8080:8080）
/// let same_port = PortPair::with_same(8080);
///
/// // 不同的端口（主机 8080 映射到访客 80）
/// let distinct_ports = PortPair::with_distinct(8080, 80);
///
/// // 从字符串解析
/// let from_str = "8080:80".parse::<PortPair>().unwrap();
/// assert_eq!(from_str, distinct_ports);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortPair {
    /// 访客端口和主机端口不同
    ///
    /// 用于将主机的端口转发到 VM 内的不同端口
    Distinct {
        /// 主机端口
        ///
        /// 主机系统上监听的端口号
        host: u16,

        /// 访客端口
        ///
        /// VM（访客系统）内的端口号
        guest: u16,
    },

    /// 访客端口和主机端口相同
    ///
    /// 用于在相同端口号上进行转发
    Same(u16),
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl PortPair {
    /// 创建一个新的 PortPair，主机和访客端口相同
    ///
    /// ## 参数
    /// * `port` - 要使用的端口号（同时用于主机和访客）
    pub fn with_same(port: u16) -> Self {
        Self::Same(port)
    }

    /// 创建一个新的 PortPair，主机和访客端口不同
    ///
    /// ## 参数
    /// * `host` - 主机端口
    /// * `guest` - 访客端口
    pub fn with_distinct(host: u16, guest: u16) -> Self {
        Self::Distinct { host, guest }
    }

    /// 获取主机端口
    ///
    /// 返回主机系统上的端口号
    pub fn get_host(&self) -> u16 {
        match self {
            Self::Distinct { host, .. } | Self::Same(host) => *host,
        }
    }

    /// 获取访客端口
    ///
    /// 返回 VM（访客系统）内的端口号
    pub fn get_guest(&self) -> u16 {
        match self {
            Self::Distinct { guest, .. } | Self::Same(guest) => *guest,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl FromStr for PortPair {
    type Err = MicrosandboxError;

    /// 从字符串解析端口对
    ///
    /// 期望格式为 "host:guest" 或单独的 "port"
    ///
    /// ## 参数
    /// * `s` - 要解析的字符串
    ///
    /// ## 返回值
    /// * `Ok(PortPair)` - 解析成功
    /// * `Err(MicrosandboxError::InvalidPortPair)` - 格式无效或端口号无法解析
    ///
    /// ## 格式说明
    /// * 空字符串 - 错误
    /// * ":" 或 "host:" 或 ":guest" - 错误（部分为空）
    /// * "port" - Same(port)
    /// * "port:port" - 如果相同则为 Same(port)，否则为 Distinct { host, guest }
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 空字符串无效
        if s.is_empty() {
            return Err(MicrosandboxError::InvalidPortPair(s.to_string()));
        }

        // 包含冒号的格式
        if s.contains(':') {
            let (host, guest) = s.split_once(':').unwrap();
            // 主机或访客端口为空时无效
            if guest.is_empty() || host.is_empty() {
                return Err(MicrosandboxError::InvalidPortPair(s.to_string()));
            }

            // 如果端口相同，使用 Same 变体
            if guest == host {
                return Ok(Self::Same(
                    host.parse()
                        .map_err(|_| MicrosandboxError::InvalidPortPair(s.to_string()))?,
                ));
            } else {
                // 端口不同，使用 Distinct 变体
                return Ok(Self::Distinct {
                    host: host
                        .parse()
                        .map_err(|_| MicrosandboxError::InvalidPortPair(s.to_string()))?,
                    guest: guest
                        .parse()
                        .map_err(|_| MicrosandboxError::InvalidPortPair(s.to_string()))?,
                });
            }
        }

        // 不包含冒号，只有单个端口，使用 Same 变体
        Ok(Self::Same(s.parse().map_err(|_| {
            MicrosandboxError::InvalidPortPair(s.to_string())
        })?))
    }
}

impl fmt::Display for PortPair {
    /// 格式化端口对为 "host:guest" 格式
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Distinct { host, guest } => {
                write!(f, "{}:{}", host, guest)
            }
            Self::Same(port) => write!(f, "{}:{}", port, port),
        }
    }
}

impl Serialize for PortPair {
    /// 序列化为字符串格式
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PortPair {
    /// 从字符串反序列化
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
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
    fn test_port_pair_from_str() {
        // 测试相同端口
        assert_eq!("8080".parse::<PortPair>().unwrap(), PortPair::Same(8080));
        assert_eq!(
            "8080:8080".parse::<PortPair>().unwrap(),
            PortPair::Same(8080)
        );

        // 测试不同端口（host:guest 格式）
        assert_eq!(
            "8080:80".parse::<PortPair>().unwrap(),
            PortPair::Distinct {
                host: 8080,
                guest: 80
            }
        );

        // 测试无效格式
        assert!("".parse::<PortPair>().is_err());
        assert!(":80".parse::<PortPair>().is_err());
        assert!("80:".parse::<PortPair>().is_err());
        assert!("invalid".parse::<PortPair>().is_err());
        assert!("invalid:80".parse::<PortPair>().is_err());
        assert!("80:Invalid".parse::<PortPair>().is_err());
    }

    #[test]
    fn test_port_pair_display() {
        // 测试相同端口
        assert_eq!(PortPair::Same(8080).to_string(), "8080:8080");

        // 测试不同端口（host:guest 格式）
        assert_eq!(
            PortPair::Distinct {
                host: 8080,
                guest: 80
            }
            .to_string(),
            "8080:80"
        );
    }

    #[test]
    fn test_port_pair_getters() {
        // 测试相同端口
        let same = PortPair::Same(8080);
        assert_eq!(same.get_host(), 8080);
        assert_eq!(same.get_guest(), 8080);

        // 测试不同端口
        let distinct = PortPair::Distinct {
            host: 8080,
            guest: 80,
        };
        assert_eq!(distinct.get_host(), 8080);
        assert_eq!(distinct.get_guest(), 80);
    }

    #[test]
    fn test_port_pair_constructors() {
        assert_eq!(PortPair::with_same(8080), PortPair::Same(8080));
        assert_eq!(
            PortPair::with_distinct(8080, 80),
            PortPair::Distinct {
                host: 8080,
                guest: 80
            }
        );
    }
}
