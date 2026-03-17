//! 环境变量对
//!
//! 本模块定义了环境变量对（EnvPair）结构，用于封装环境变量名称和值。

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::MicrosandboxError;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// 环境变量对
///
/// 此结构封装了环境变量名称及其对应的值。
/// 用于管理进程的环境变量配置。
///
/// ## 格式说明
/// 环境变量对的格式为 "NAME=value"，其中：
/// - `NAME` - 环境变量名称（不能为空）
/// - `value` - 环境变量值（可以为空）
///
/// ## 使用示例
///
/// ```
/// use microsandbox_core::config::EnvPair;
/// use std::str::FromStr;
///
/// // 创建环境变量对
/// let env_pair = EnvPair::new("PATH", "/usr/local/bin:/usr/bin");
///
/// assert_eq!(env_pair.get_name(), "PATH");
/// assert_eq!(env_pair.get_value(), "/usr/local/bin:/usr/bin");
///
/// // 从字符串解析
/// let env_pair = EnvPair::from_str("USER=alice").unwrap();
///
/// assert_eq!(env_pair.get_name(), "USER");
/// assert_eq!(env_pair.get_value(), "alice");
/// ```
#[derive(Debug, Hash, Clone, PartialEq, Eq, Getters)]
#[getset(get = "pub with_prefix")]
pub struct EnvPair {
    /// 环境变量名称
    ///
    /// 环境变量的标识符，如 "PATH"、"HOME"、"USER" 等
    name: String,

    /// 环境变量的值
    ///
    /// 与名称关联的值，可以为空字符串
    value: String,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl EnvPair {
    /// 创建新的 EnvPair 实例
    ///
    /// ## 参数
    /// * `name` - 环境变量名称
    /// * `value` - 环境变量值
    ///
    /// ## 使用示例
    ///
    /// ```
    /// use microsandbox_core::config::EnvPair;
    ///
    /// let env_pair = EnvPair::new("HOME", "/home/user");
    /// assert_eq!(env_pair.get_name(), "HOME");
    /// assert_eq!(env_pair.get_value(), "/home/user");
    /// ```
    pub fn new<S: Into<String>>(name: S, value: S) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl FromStr for EnvPair {
    type Err = MicrosandboxError;

    /// 从字符串解析环境变量对
    ///
    /// 期望格式为 "NAME=value"
    ///
    /// ## 参数
    /// * `s` - 要解析的字符串
    ///
    /// ## 返回值
    /// * `Ok(EnvPair)` - 解析成功
    /// * `Err(MicrosandboxError::InvalidEnvPair)` - 格式无效（如缺少 '=' 或名称为空）
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 使用 split_once 在第一个 '=' 处分割
        let (var, value) = s
            .split_once('=')
            .ok_or_else(|| MicrosandboxError::InvalidEnvPair(s.to_string()))?;

        // 变量名不能为空
        if var.is_empty() {
            return Err(MicrosandboxError::InvalidEnvPair(s.to_string()));
        }

        Ok(Self::new(var, value))
    }
}

impl fmt::Display for EnvPair {
    /// 格式化环境变量对为 "NAME=value" 格式
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}={}", self.name, self.value)
    }
}

impl Serialize for EnvPair {
    /// 序列化为字符串格式
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EnvPair {
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
    fn test_env_pair_new() {
        let env_pair = EnvPair::new("VAR", "VALUE");
        assert_eq!(env_pair.name, String::from("VAR"));
        assert_eq!(env_pair.value, String::from("VALUE"));
    }

    #[test]
    fn test_env_pair_from_str() -> anyhow::Result<()> {
        // 基本格式
        let env_pair: EnvPair = "VAR=VALUE".parse()?;
        assert_eq!(env_pair.name, String::from("VAR"));
        assert_eq!(env_pair.value, String::from("VALUE"));

        // 空值
        let env_pair: EnvPair = "VAR=".parse()?;
        assert_eq!(env_pair.name, String::from("VAR"));
        assert_eq!(env_pair.value, String::from(""));

        // 无效格式：缺少 '='
        assert!("VAR".parse::<EnvPair>().is_err());
        // 无效格式：名称为空
        assert!("=VALUE".parse::<EnvPair>().is_err());

        Ok(())
    }

    #[test]
    fn test_env_pair_display() {
        let env_pair = EnvPair::new("VAR", "VALUE");
        assert_eq!(env_pair.to_string(), "VAR=VALUE");

        let env_pair = EnvPair::new("VAR", "");
        assert_eq!(env_pair.to_string(), "VAR=");
    }

    #[test]
    fn test_env_pair_serialize_deserialize() -> anyhow::Result<()> {
        let env_pair = EnvPair::new("VAR", "VALUE");
        let serialized = serde_json::to_string(&env_pair)?;
        assert_eq!(serialized, "\"VAR=VALUE\"");

        let deserialized: EnvPair = serde_json::from_str(&serialized)?;
        assert_eq!(deserialized, env_pair);

        let env_pair = EnvPair::new("VAR", "");
        let serialized = serde_json::to_string(&env_pair)?;
        assert_eq!(serialized, "\"VAR=\"");

        let deserialized: EnvPair = serde_json::from_str(&serialized)?;
        assert_eq!(deserialized, env_pair);

        Ok(())
    }

    #[test]
    fn test_env_pair_with_special_characters() -> anyhow::Result<()> {
        // 带下划线的变量名和带空格的值
        let env_pair: EnvPair = "VAR_WITH_UNDERSCORE=VALUE WITH SPACES".parse()?;
        assert_eq!(env_pair.name, "VAR_WITH_UNDERSCORE");
        assert_eq!(env_pair.value, "VALUE WITH SPACES");

        // 带点的变量名
        let env_pair: EnvPair = "VAR.WITH.DOTS=VALUE_WITH_UNDERSCORE".parse()?;
        assert_eq!(env_pair.name, "VAR.WITH.DOTS");
        assert_eq!(env_pair.value, "VALUE_WITH_UNDERSCORE");

        Ok(())
    }
}
