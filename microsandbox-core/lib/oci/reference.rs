//! OCI 镜像引用模块
//!
//! 本模块定义了 OCI（Open Container Initiative）镜像引用的封装类型。
//!
//! ## 什么是 OCI 镜像引用？
//!
//! OCI 镜像引用是用于唯一标识容器镜像的字符串，格式如下：
//!
//! ```text
//! [registry/][repository/]image:tag
//! ```
//!
//! ### 引用组成部分
//!
//! | 部分 | 说明 | 示例 |
//! |------|------|------|
//! | registry | 镜像注册表域名 | `docker.io`, `ghcr.io` |
//! | repository | 仓库路径 | `library`, `myuser` |
//! | image | 镜像名称 | `ubuntu`, `nginx` |
//! | tag | 版本标签 | `latest`, `20.04` |
//!
//! ### 常见示例
//!
//! ```text
//! docker.io/library/ubuntu:latest    # 完整的 Ubuntu 镜像引用
//! nginx:alpine                       # 使用默认注册表的 Nginx 镜像
//! ghcr.io/owner/repo:v1.0.0         # GitHub Container Registry
//! ```
//!
//! ## 为什么需要封装？
//!
//! `Reference` 结构体是对底层 `oci_client::Reference` 的封装，提供：
//!
//! 1. **类型安全**: 确保只有有效的镜像引用才能被创建
//! 2. **序列化支持**: 通过 serde 实现与数据库的交互
//! 3. **字符串转换**: 方便的 `FromStr` 和 `Display` 实现
//! 4. **数据库键**: 提供将引用转换为数据库键的方法
//!
//! ## 使用示例
//!
//! ```rust,ignore
//! // 从字符串解析镜像引用
//! let reference: Reference = "docker.io/library/ubuntu:latest".parse()?;
//!
//! // 转换为字符串
//! let ref_string: String = reference.clone().into();
//!
//! // 获取数据库键
//! let db_key = reference.as_db_key();
//! ```

use core::fmt;
use std::{ops::Deref, str::FromStr};

use serde;

use crate::MicrosandboxError;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// OCI 兼容的镜像引用封装
///
/// 此结构体封装了底层的 `oci_client::Reference`，提供类型安全的镜像引用表示。
///
/// ## 序列化特性
///
/// 使用 serde 的转换属性实现与字符串的双向转换：
/// - `#[serde(try_from = "String")]`: 反序列化时尝试从 String 转换
/// - `#[serde(into = "String")]`: 序列化时转换为 String
///
/// 这使得 `Reference` 可以直接存储到 SQLite 数据库中。
///
/// ## 字段说明
///
/// - `reference`: 底层的 OCI 引用对象，包含解析后的镜像名称、标签等信息
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct Reference {
    /// 底层的 OCI 引用对象
    reference: oci_client::Reference,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String")]
#[serde(into = "String")]
pub struct Reference {
    reference: oci_client::Reference,
}

impl Reference {
    /// 转换为底层的 OCI 引用对象
    ///
    /// 此方法用于与 `oci_client` 库交互时，获取底层的引用对象。
    ///
    /// ## 返回值
    ///
    /// 返回 `oci_client::Reference` 的克隆副本
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// let reference: Reference = "nginx:latest".parse()?;
    /// let oci_ref = reference.as_oci_reference();
    /// // 现在可以使用 oci_client 的方法
    /// ```
    pub fn as_oci_reference(&self) -> oci_client::Reference {
        self.reference.clone()
    }

    /// 获取数据库键字符串
    ///
    /// 将镜像引用转换为适合存储到数据库的字符串格式。
    /// 此方法主要用于 SQLite 数据库的主键或外键。
    ///
    /// ## 返回值
    ///
    /// 返回完整的镜像引用字符串，例如：
    /// - `docker.io/library/ubuntu:latest`
    /// - `nginx:alpine`
    ///
    /// ## 使用示例
    ///
    /// ```rust,ignore
    /// let reference: Reference = "ubuntu:20.04".parse()?;
    /// let db_key = reference.as_db_key();
    /// // db_key = "ubuntu:20.04"
    /// ```
    pub(crate) fn as_db_key(&self) -> String {
        self.reference.to_string()
    }
}

/// 实现 Deref trait，允许直接访问底层 OCI 引用的方法
///
/// 通过实现 `Deref<Target = oci_client::Reference>`，`Reference` 可以：
/// 1. 自动解引用为 `oci_client::Reference`
/// 2. 直接调用底层对象的方法（称为 "deref coercion"）
///
/// ## 示例
///
/// ```rust,ignore
/// let reference: Reference = "nginx:latest".parse()?;
/// // 可以直接调用 oci_client::Reference 的方法
/// let tag = reference.tag();  // 自动解引用
/// ```
impl Deref for Reference {
    type Target = oci_client::Reference;

    fn deref(&self) -> &Self::Target {
        &self.reference
    }
}

/// 实现 FromStr trait，支持从字符串解析镜像引用
///
/// 此实现允许使用 `.parse()` 方法从字符串创建 `Reference` 对象。
///
/// ## 解析规则
///
/// 支持的格式：
/// - `image:tag` - 仅镜像名和标签（使用默认注册表）
/// - `registry/image:tag` - 包含注册表
/// - `registry/namespace/image:tag` - 完整路径
///
/// ## 错误处理
///
/// 如果字符串格式无效，返回 `MicrosandboxError::OciError`
///
/// ## 使用示例
///
/// ```rust,ignore
/// // 使用 parse() 方法
/// let reference: Reference = "ubuntu:latest".parse()?;
///
/// // 或使用 FromStr trait
/// let reference = Reference::from_str("nginx:alpine")?;
/// ```
impl FromStr for Reference {
    type Err = MicrosandboxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 使用 oci_client::Reference 的解析器
        // 会验证格式并提取 registry、repository、image、tag 等部分
        Ok(Reference {
            reference: oci_client::Reference::from_str(s)?,
        })
    }
}

/// 实现 From trait，将 Reference 转换为 String
///
/// 此实现允许使用 `.into()` 方法将 `Reference` 转换为字符串。
/// 与 `as_db_key()` 不同，此实现消耗所有权。
///
/// ## 使用示例
///
/// ```rust,ignore
/// let reference: Reference = "ubuntu:20.04".parse()?;
/// let string: String = reference.into();  // 消耗所有权
/// ```
impl From<Reference> for String {
    fn from(reference: Reference) -> Self {
        reference.reference.to_string()
    }
}

/// 实现 TryFrom trait，尝试从 String 创建 Reference
///
/// 与 `FromStr` 类似，但专门用于从 `String` 类型转换。
/// 使用 `TryFrom` 而不是 `From` 是因为字符串可能包含无效的镜像引用格式。
///
/// ## 与 FromStr 的区别
///
/// - `FromStr`: 从 `&str` 解析，使用 `.parse()` 方法
/// - `TryFrom<String>`: 从 `String` 转换，使用 `.try_into()` 方法
///
/// ## 使用示例
///
/// ```rust,ignore
/// let string = "nginx:latest".to_string();
/// let reference: Reference = string.try_into()?;
/// ```
impl TryFrom<String> for Reference {
    type Error = MicrosandboxError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        // 使用 oci_client::Reference 的转换器
        Ok(Reference {
            reference: oci_client::Reference::try_from(value)?,
        })
    }
}

/// 实现 Display trait，支持使用 {} 格式化输出
///
/// 此实现允许直接使用 `format!("{}", reference)` 或 `println!("{}", reference)` 输出镜像引用。
/// 输出格式与原始输入格式一致（完整引用字符串）。
///
/// ## 使用示例
///
/// ```rust,ignore
/// let reference: Reference = "ubuntu:latest".parse()?;
/// println!("镜像引用：{}", reference);  // 输出：镜像引用：ubuntu:latest
/// ```
impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reference)
    }
}
