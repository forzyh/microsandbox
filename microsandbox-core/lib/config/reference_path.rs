//! 镜像引用或路径
//!
//! 本模块定义了 ReferenceOrPath 枚举，用于表示 OCI 镜像引用或本地 rootfs 路径。

use std::{
    fmt::{self, Display},
    path::PathBuf,
    str::FromStr,
};

use crate::{MicrosandboxError, oci::Reference};

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// OCI 镜像引用或本地 rootfs 路径
///
/// 此类型用于指定容器根文件系统的来源：
/// - 对于 OCI 镜像（如 "docker.io/library/ubuntu:latest"），使用 `Reference` 变体
/// - 对于本地 rootfs 目录（如 "/path/to/rootfs" 或 "./rootfs"），使用 `Path` 变体
///
/// ## 变体说明
/// * `Reference(Reference)` - OCI 镜像引用，从容器注册表拉取
/// * `Path(PathBuf)` - 本地 rootfs 目录路径
///
/// ## 解析规则
/// - 以 "." 或 "/" 开头的字符串被解释为本地 rootfs 路径
/// - 其他字符串被解释为 OCI 镜像引用
///
/// ## 使用示例
/// ```
/// use std::str::FromStr;
/// use microsandbox_core::config::ReferenceOrPath;
///
/// // 解析为本地 rootfs 路径
/// let local = ReferenceOrPath::from_str("./my-rootfs").unwrap();
/// let absolute = ReferenceOrPath::from_str("/var/lib/my-rootfs").unwrap();
///
/// // 解析为 OCI 镜像引用
/// let image = ReferenceOrPath::from_str("ubuntu:latest").unwrap();
/// let full = ReferenceOrPath::from_str("docker.io/library/debian:11").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub enum ReferenceOrPath {
    /// OCI 镜像引用（如 "docker.io/library/ubuntu:latest"）
    ///
    /// 用于从容器注册表拉取 rootfs
    Reference(Reference),

    /// 本地 rootfs 目录的路径
    ///
    /// 可以是绝对路径（如 "/path/to/rootfs"）或相对路径（如 "./rootfs"）
    Path(PathBuf),
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

impl FromStr for ReferenceOrPath {
    type Err = MicrosandboxError;

    /// 从字符串解析 ReferenceOrPath
    ///
    /// 解析规则：
    /// - 如果字符串以 "." 或 "/" 开头，则解释为本地 rootfs 路径
    /// - 否则，解释为 OCI 镜像引用
    ///
    /// ## 参数
    /// * `s` - 要解析的字符串
    ///
    /// ## 返回值
    /// * `Ok(ReferenceOrPath)` - 解析成功
    /// * `Err(MicrosandboxError)` - 解析失败（如格式无效的镜像引用）
    ///
    /// ## 使用示例
    /// ```
    /// # use std::str::FromStr;
    /// # use microsandbox_core::config::ReferenceOrPath;
    /// // 解析为本地 rootfs 路径
    /// let local = ReferenceOrPath::from_str("./my-rootfs").unwrap();
    /// let absolute = ReferenceOrPath::from_str("/var/lib/my-rootfs").unwrap();
    ///
    /// // 解析为 OCI 镜像引用
    /// let image = ReferenceOrPath::from_str("ubuntu:latest").unwrap();
    /// let full = ReferenceOrPath::from_str("docker.io/library/debian:11").unwrap();
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 检查字符串是否以 "." 或 "/" 开头来判断是否为路径
        if s.starts_with('.') || s.starts_with('/') {
            Ok(ReferenceOrPath::Path(PathBuf::from(s)))
        } else {
            // 解析为镜像引用
            let reference = Reference::from_str(s)?;
            Ok(ReferenceOrPath::Reference(reference))
        }
    }
}

impl Display for ReferenceOrPath {
    /// 格式化 ReferenceOrPath 为字符串
    ///
    /// ## 格式化规则
    /// - `Path` 变体：直接输出路径字符串
    /// - `Reference` 变体：使用 Reference 的 Display 实现（通常是标准化格式）
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReferenceOrPath::Path(path) => write!(f, "{}", path.display()),
            ReferenceOrPath::Reference(reference) => write!(f, "{}", reference),
        }
    }
}

impl TryFrom<String> for ReferenceOrPath {
    type Error = MicrosandboxError;

    /// 从 String 尝试转换为 ReferenceOrPath
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<ReferenceOrPath> for String {
    /// 将 ReferenceOrPath 转换为 String
    fn from(val: ReferenceOrPath) -> Self {
        val.to_string()
    }
}

//--------------------------------------------------------------------------------------------------
// 测试
//--------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_relative() {
        // 测试不同格式的相对路径
        let cases = vec![
            "./path/to/file",
            "./single",
            ".",
            "./path/with/multiple/segments",
            "./path.with.dots",
            "./path-with-dashes",
            "./path_with_underscores",
        ];

        for case in cases {
            let reference = ReferenceOrPath::from_str(case).unwrap();
            match &reference {
                ReferenceOrPath::Path(path) => {
                    assert_eq!(path, &PathBuf::from(case));
                    assert_eq!(reference.to_string(), case);
                }
                _ => panic!("Expected Path variant for {}", case),
            }
        }
    }

    #[test]
    fn test_path_absolute() {
        // 测试不同格式的绝对路径
        let cases = vec![
            "/absolute/path",
            "/root",
            "/path/with/multiple/segments",
            "/path.with.dots",
            "/path-with-dashes",
            "/path_with_underscores",
        ];

        for case in cases {
            let reference = ReferenceOrPath::from_str(case).unwrap();
            match &reference {
                ReferenceOrPath::Path(path) => {
                    assert_eq!(path, &PathBuf::from(case));
                    assert_eq!(reference.to_string(), case);
                }
                _ => panic!("Expected Path variant for {}", case),
            }
        }
    }

    #[test]
    fn test_image_reference_simple() {
        // 测试简单镜像引用
        let cases = vec![
            "alpine:latest",
            "ubuntu:20.04",
            "nginx:1.19",
            "redis:6",
            "postgres:13-alpine",
        ];

        for case in cases {
            let reference = ReferenceOrPath::from_str(case).unwrap();
            match &reference {
                ReferenceOrPath::Reference(ref_) => {
                    assert_eq!(reference.to_string(), ref_.to_string());
                }
                _ => panic!("Expected Reference variant for {}", case),
            }
        }
    }

    #[test]
    fn test_image_reference_with_registry() {
        // 测试带注册表的镜像引用
        let cases = vec![
            "docker.io/library/alpine:latest",
            "registry.example.com/myapp:v1.0",
            "ghcr.io/owner/repo:tag",
            "k8s.gcr.io/pause:3.2",
            "quay.io/organization/image:1.0",
        ];

        for case in cases {
            let reference = ReferenceOrPath::from_str(case).unwrap();
            match &reference {
                ReferenceOrPath::Reference(ref_) => {
                    assert_eq!(reference.to_string(), ref_.to_string());
                }
                _ => panic!("Expected Reference variant for {}", case),
            }
        }
    }

    #[test]
    fn test_image_reference_with_digest() {
        // 测试带 digest 的镜像引用
        let valid_digest = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        let cases = vec![
            format!("alpine@sha256:{}", valid_digest),
            format!("docker.io/library/ubuntu@sha256:{}", valid_digest),
            format!("registry.example.com/myapp:v1.0@sha256:{}", valid_digest),
        ];

        for case in cases {
            let reference = ReferenceOrPath::from_str(&case).unwrap();
            match &reference {
                ReferenceOrPath::Reference(ref_) => {
                    assert_eq!(reference.to_string(), ref_.to_string());
                }
                _ => panic!("Expected Reference variant for {}", case),
            }
        }
    }

    #[test]
    fn test_image_reference_with_port() {
        // 测试带端口号的注册表镜像引用
        let cases = vec![
            "localhost:5000/myapp:latest",
            "registry.example.com:5000/app:v1",
            "192.168.1.1:5000/image:tag",
        ];

        for case in cases {
            let reference = ReferenceOrPath::from_str(case).unwrap();
            match &reference {
                ReferenceOrPath::Reference(ref_) => {
                    assert_eq!(reference.to_string(), ref_.to_string());
                }
                _ => panic!("Expected Reference variant for {}", case),
            }
        }
    }

    #[test]
    fn test_empty_input() {
        // 测试空输入
        assert!(ReferenceOrPath::from_str("").is_err());
    }

    #[test]
    fn test_display_formatting() {
        // 测试两种变体的显示格式
        let test_cases = vec![
            ("./local/path", "./local/path"),
            ("/absolute/path", "/absolute/path"),
            ("alpine:latest", "docker.io/library/alpine:latest"),
            (
                "registry.example.com/app:v1.0",
                "registry.example.com/app:v1.0",
            ),
        ];

        for (input, expected) in test_cases {
            let reference = ReferenceOrPath::from_str(input).unwrap();
            assert_eq!(reference.to_string(), expected);
        }
    }

    #[test]
    fn test_serde_path_roundtrip() {
        // 测试 Path 变体的序列化/反序列化往返
        let test_cases = vec![
            ReferenceOrPath::Path(PathBuf::from("./local/rootfs")),
            ReferenceOrPath::Path(PathBuf::from("/absolute/path/to/rootfs")),
            ReferenceOrPath::Path(PathBuf::from(".")),
            ReferenceOrPath::Path(PathBuf::from("/root")),
        ];

        for case in test_cases {
            let serialized = serde_yaml::to_string(&case).unwrap();
            let deserialized: ReferenceOrPath = serde_yaml::from_str(&serialized).unwrap();
            assert_eq!(case, deserialized);
        }
    }

    #[test]
    fn test_serde_reference_roundtrip() {
        // 测试 Reference 变体的序列化/反序列化往返
        let test_cases = vec![
            "alpine:latest",
            "docker.io/library/ubuntu:20.04",
            "registry.example.com:5000/myapp:v1.0",
            "ghcr.io/owner/repo:tag",
        ];

        for case in test_cases {
            let reference = ReferenceOrPath::from_str(case).unwrap();
            let serialized = serde_yaml::to_string(&reference).unwrap();
            let deserialized: ReferenceOrPath = serde_yaml::from_str(&serialized).unwrap();
            assert_eq!(reference, deserialized);
        }
    }

    #[test]
    fn test_serde_yaml_format() {
        // 测试 Path 变体的序列化格式
        let path = ReferenceOrPath::Path(PathBuf::from("/test/rootfs"));
        let serialized = serde_yaml::to_string(&path).unwrap();
        assert_eq!(serialized.trim(), "/test/rootfs");

        // 测试 Reference 变体的序列化格式
        let reference = ReferenceOrPath::from_str("ubuntu:latest").unwrap();
        let serialized = serde_yaml::to_string(&reference).unwrap();
        assert!(serialized.trim().contains("ubuntu:latest"));
    }

    #[test]
    fn test_serde_invalid_input() {
        // 测试反序列化无效的 YAML
        let invalid_yaml = "- not a valid reference path";
        assert!(serde_yaml::from_str::<ReferenceOrPath>(invalid_yaml).is_err());

        // 测试反序列化无效的镜像引用格式
        let invalid_reference = "invalid!reference:format";
        assert!(serde_yaml::from_str::<ReferenceOrPath>(invalid_reference).is_err());
    }
}
