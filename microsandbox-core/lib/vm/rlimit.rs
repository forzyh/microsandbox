//! Linux 资源限制（rlimit）模块
//!
//! 本模块定义了 Linux 进程资源限制的类型和解析逻辑。
//!
//! ## 什么是资源限制（rlimit）？
//!
//! Linux 内核通过资源限制机制控制进程可以使用的系统资源：
//! - **软限制（soft limit）**: 内核实际执行的限制值
//! - **硬限制（hard limit）**: 软限制的上限，只有 root 用户可以提升
//!
//! ## 资源类型
//!
//! 本模块支持以下 Linux 资源类型：
//!
//! | 资源 | 说明 | 典型用途 |
//! |------|------|----------|
//! | `RLIMIT_CPU` | CPU 时间（秒） | 限制进程运行时间 |
//! | `RLIMIT_FSIZE` | 创建文件的最大大小 | 防止磁盘空间耗尽 |
//! | `RLIMIT_DATA` | 数据段最大大小 | 限制堆内存 |
//! | `RLIMIT_STACK` | 栈段最大大小 | 限制栈溢出 |
//! | `RLIMIT_CORE` | 核心转储最大大小 | 控制调试文件 |
//! | `RLIMIT_NOFILE` | 打开文件描述符数量 | 限制文件句柄 |
//! | `RLIMIT_NPROC` | 最大进程数 | 限制 fork 炸弹 |
//! | `RLIMIT_AS` | 地址空间最大大小 | 限制虚拟内存 |
//!
//! ## 字符串格式
//!
//! 资源限制支持从字符串解析，格式为：
//! ```text
//! <RESOURCE>=<soft>:<hard>
//! ```
//!
//! 示例：
//! ```text
//! RLIMIT_NOFILE=1000:2000    # 软限制 1000，硬限制 2000
//! 0=10:20                     # 使用数字 ID（0=RLIMIT_CPU）
//! ```

use crate::MicrosandboxError;
use getset::Getters;
use serde::{Deserialize, Serialize};
use std::{convert::TryFrom, fmt, str::FromStr};

//--------------------------------------------------------------------------------------------------
// LinuxRLimitResource - Linux 资源类型枚举
//--------------------------------------------------------------------------------------------------

/// Linux 资源限制类型枚举
///
/// 此枚举定义了所有支持的 Linux 资源限制类型。
/// 每个变体对应一个 Linux 内核中的资源限制编号（0-15）。
///
/// ## `#[repr(u32)]` 属性
///
/// 使用 `#[repr(u32)]` 指定枚举的底层表示为 `u32`，
/// 这样可以直接与 Linux 系统调用的参数对应。
///
/// ## `#[allow(non_camel_case_types)]`
///
/// 允许使用非驼峰命名法，因为资源名称来自 Linux 内核定义
/// （如 `RLIMIT_CPU` 而不是 `RlimitCpu`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(non_camel_case_types)]
pub enum LinuxRLimitResource {
    /// CPU 时间限制（秒）
    ///
    /// 超过此限制的进程会收到 SIGXCPU 信号
    RLIMIT_CPU = 0,

    /// 创建文件的最大大小
    ///
    /// 超过此限制时 write() 会失败并返回 EFBIG 错误
    RLIMIT_FSIZE = 1,

    /// 数据段的最大大小（堆内存）
    ///
    /// 控制进程可以分配的堆内存大小
    RLIMIT_DATA = 2,

    /// 栈段的最大大小
    ///
    /// 限制栈溢出，超过会导致段错误
    RLIMIT_STACK = 3,

    /// 核心转储文件的最大大小
    ///
    /// 设置为 0 可以禁用核心转储
    RLIMIT_CORE = 4,

    /// 最大常驻内存集大小（RSS）
    ///
    /// 在 Linux 上不强制执行，仅供查询使用
    RLIMIT_RSS = 5,

    /// 最大进程数
    ///
    /// 限制进程可以创建的子进程数量，防止 fork 炸弹
    RLIMIT_NPROC = 6,

    /// 打开文件描述符的最大数量
    ///
    /// 限制进程可以同时打开的文件数量
    RLIMIT_NOFILE = 7,

    /// 最大锁定内存大小
    ///
    /// 限制可以使用 mlock()/mlockall() 锁定的内存
    RLIMIT_MEMLOCK = 8,

    /// 地址空间的最大大小（虚拟内存）
    ///
    /// 限制进程可以使用的总内存地址空间
    RLIMIT_AS = 9,

    /// 文件锁的最大数量
    ///
    /// 限制进程可以持有的文件锁数量
    RLIMIT_LOCKS = 10,

    /// 可以排队的最大信号数
    ///
    /// 限制实时信号队列的大小
    RLIMIT_SIGPENDING = 11,

    /// POSIX 消息队列的最大字节数
    ///
    /// 限制 mq_open() 创建的消息队列大小
    RLIMIT_MSGQUEUE = 12,

    /// 最大 nice 优先级
    ///
    /// 限制进程可以设置的 nice 值下限
    RLIMIT_NICE = 13,

    /// 最大实时优先级
    ///
    /// 限制实时调度策略的优先级
    RLIMIT_RTPRIO = 14,

    /// 实时时钟的最大休眠时间（秒）
    ///
    /// 限制 clock_nanosleep() 的实时睡眠时长
    RLIMIT_RTTIME = 15,
}

/// Linux 资源限制结构体
///
/// 此结构体封装了资源类型及其对应的软限制和硬限制。
///
/// ## 软限制和硬限制的关系
///
/// ```text
/// 软限制 ≤ 硬限制
///    ↓        ↓
///  内核    普通用户可设
///  执行    置的上限
/// ```
///
/// - **软限制**: 内核实际执行的限制值。进程可以自己降低软限制。
/// - **硬限制**: 软限制的上限。只有 root 用户可以提升硬限制。
///
/// ## `#[getset(get = "pub with_prefix")]`
///
/// 使用 `getset` 宏自动生成 getter 方法：
/// - `get_resource()` - 返回资源类型
/// - `get_soft()` - 返回软限制
/// - `get_hard()` - 返回硬限制
///
/// `with_prefix` 表示方法名带 `get_` 前缀。
///
/// ## 使用示例
///
/// ```
/// use microsandbox_core::vm::{LinuxRlimit, LinuxRLimitResource};
///
/// // 创建一个新的资源限制
/// let cpu_limit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_CPU, 10, 20);
///
/// // 使用 getter 方法获取值
/// assert_eq!(cpu_limit.get_resource(), &LinuxRLimitResource::RLIMIT_CPU);
/// assert_eq!(cpu_limit.get_soft(), &10);
/// assert_eq!(cpu_limit.get_hard(), &20);
///
/// // 从字符串解析
/// let nofile_limit: LinuxRlimit = "RLIMIT_NOFILE=1000:2000".parse().unwrap();
/// assert_eq!(nofile_limit.get_soft(), &1000);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Getters)]
#[getset(get = "pub with_prefix")]
pub struct LinuxRlimit {
    /// 要限制的资源类型
    resource: LinuxRLimitResource,

    /// 软限制值
    ///
    /// 这是内核为相应资源强制执行的值
    soft: u64,

    /// 硬限制值
    ///
    /// 这作为软限制的上限
    hard: u64,
}

//--------------------------------------------------------------------------------------------------
// LinuxRLimitResource 实现
//--------------------------------------------------------------------------------------------------

impl LinuxRLimitResource {
    /// 获取对应的枚举整数值
    ///
    /// 此方法返回资源类型的底层 `u32` 值，
    /// 可用于与 Linux 系统调用交互。
    ///
    /// ## 返回值
    ///
    /// 返回资源类型的编号（0-15）
    ///
    /// ## 示例
    ///
    /// ```
    /// use microsandbox_core::vm::LinuxRLimitResource;
    ///
    /// assert_eq!(LinuxRLimitResource::RLIMIT_CPU.as_int(), 0);
    /// assert_eq!(LinuxRLimitResource::RLIMIT_NOFILE.as_int(), 7);
    /// ```
    pub fn as_int(&self) -> u32 {
        *self as u32
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRlimit 实现
//--------------------------------------------------------------------------------------------------

impl LinuxRlimit {
    /// 创建新的资源限制实例
    ///
    /// ## 参数
    ///
    /// * `resource` - 资源类型
    /// * `soft` - 软限制值
    /// * `hard` - 硬限制值
    ///
    /// ## 返回值
    ///
    /// 返回一个新的 `LinuxRlimit` 实例
    ///
    /// ## 使用示例
    ///
    /// ```
    /// use microsandbox_core::vm::{LinuxRlimit, LinuxRLimitResource};
    ///
    /// // 限制 CPU 时间为 10 秒（软）/20 秒（硬）
    /// let cpu_limit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_CPU, 10, 20);
    ///
    /// // 限制打开文件数为 1000（软）/2000（硬）
    /// let nofile_limit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_NOFILE, 1000, 2000);
    /// ```
    pub fn new(resource: LinuxRLimitResource, soft: u64, hard: u64) -> Self {
        Self {
            resource,
            soft,
            hard,
        }
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRLimitResource 的 TryFrom<u32> 实现
//--------------------------------------------------------------------------------------------------

/// 从 u32 转换为 LinuxRLimitResource
///
/// 此实现允许将 Linux 内核的资源编号转换为对应的枚举变体。
///
/// ## 错误处理
///
/// 如果输入值不在 0-15 范围内，返回 `InvalidRLimitResource` 错误。
impl TryFrom<u32> for LinuxRLimitResource {
    type Error = MicrosandboxError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::RLIMIT_CPU),
            1 => Ok(Self::RLIMIT_FSIZE),
            2 => Ok(Self::RLIMIT_DATA),
            3 => Ok(Self::RLIMIT_STACK),
            4 => Ok(Self::RLIMIT_CORE),
            5 => Ok(Self::RLIMIT_RSS),
            6 => Ok(Self::RLIMIT_NPROC),
            7 => Ok(Self::RLIMIT_NOFILE),
            8 => Ok(Self::RLIMIT_MEMLOCK),
            9 => Ok(Self::RLIMIT_AS),
            10 => Ok(Self::RLIMIT_LOCKS),
            11 => Ok(Self::RLIMIT_SIGPENDING),
            12 => Ok(Self::RLIMIT_MSGQUEUE),
            13 => Ok(Self::RLIMIT_NICE),
            14 => Ok(Self::RLIMIT_RTPRIO),
            15 => Ok(Self::RLIMIT_RTTIME),
            _ => Err(MicrosandboxError::InvalidRLimitResource(value.to_string())),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRLimitResource 的 FromStr 实现
//--------------------------------------------------------------------------------------------------

/// 从字符串解析 LinuxRLimitResource
///
/// 支持的资源名称格式（区分大小写）：
/// - `RLIMIT_CPU`, `RLIMIT_FSIZE`, `RLIMIT_NOFILE`, 等
///
/// ## 错误处理
///
/// 如果字符串不是有效的资源名称，返回 `InvalidRLimitResource` 错误。
impl FromStr for LinuxRLimitResource {
    type Err = MicrosandboxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RLIMIT_CPU" => Ok(Self::RLIMIT_CPU),
            "RLIMIT_FSIZE" => Ok(Self::RLIMIT_FSIZE),
            "RLIMIT_DATA" => Ok(Self::RLIMIT_DATA),
            "RLIMIT_STACK" => Ok(Self::RLIMIT_STACK),
            "RLIMIT_CORE" => Ok(Self::RLIMIT_CORE),
            "RLIMIT_RSS" => Ok(Self::RLIMIT_RSS),
            "RLIMIT_NPROC" => Ok(Self::RLIMIT_NPROC),
            "RLIMIT_NOFILE" => Ok(Self::RLIMIT_NOFILE),
            "RLIMIT_MEMLOCK" => Ok(Self::RLIMIT_MEMLOCK),
            "RLIMIT_AS" => Ok(Self::RLIMIT_AS),
            "RLIMIT_LOCKS" => Ok(Self::RLIMIT_LOCKS),
            "RLIMIT_SIGPENDING" => Ok(Self::RLIMIT_SIGPENDING),
            "RLIMIT_MSGQUEUE" => Ok(Self::RLIMIT_MSGQUEUE),
            "RLIMIT_NICE" => Ok(Self::RLIMIT_NICE),
            "RLIMIT_RTPRIO" => Ok(Self::RLIMIT_RTPRIO),
            "RLIMIT_RTTIME" => Ok(Self::RLIMIT_RTTIME),
            _ => Err(MicrosandboxError::InvalidRLimitResource(s.to_string())),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRLimitResource 的 Display 实现
//--------------------------------------------------------------------------------------------------

/// 实现 Display trait，支持使用 `{}` 格式化输出
///
/// 输出格式为资源名称字符串（如 `RLIMIT_CPU`）。
impl fmt::Display for LinuxRLimitResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RLIMIT_CPU => write!(f, "RLIMIT_CPU"),
            Self::RLIMIT_FSIZE => write!(f, "RLIMIT_FSIZE"),
            Self::RLIMIT_DATA => write!(f, "RLIMIT_DATA"),
            Self::RLIMIT_STACK => write!(f, "RLIMIT_STACK"),
            Self::RLIMIT_CORE => write!(f, "RLIMIT_CORE"),
            Self::RLIMIT_RSS => write!(f, "RLIMIT_RSS"),
            Self::RLIMIT_NPROC => write!(f, "RLIMIT_NPROC"),
            Self::RLIMIT_NOFILE => write!(f, "RLIMIT_NOFILE"),
            Self::RLIMIT_MEMLOCK => write!(f, "RLIMIT_MEMLOCK"),
            Self::RLIMIT_AS => write!(f, "RLIMIT_AS"),
            Self::RLIMIT_LOCKS => write!(f, "RLIMIT_LOCKS"),
            Self::RLIMIT_SIGPENDING => write!(f, "RLIMIT_SIGPENDING"),
            Self::RLIMIT_MSGQUEUE => write!(f, "RLIMIT_MSGQUEUE"),
            Self::RLIMIT_NICE => write!(f, "RLIMIT_NICE"),
            Self::RLIMIT_RTPRIO => write!(f, "RLIMIT_RTPRIO"),
            Self::RLIMIT_RTTIME => write!(f, "RLIMIT_RTTIME"),
        }
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRlimit 的 FromStr 实现
//--------------------------------------------------------------------------------------------------

/// 从字符串解析 LinuxRlimit
///
/// ## 支持的格式
///
/// 1. **名称格式**: `<RESOURCE_NAME>=<soft>:<hard>`
///    ```text
///    RLIMIT_CPU=10:20
///    RLIMIT_NOFILE=1000:2000
///    ```
///
/// 2. **数字格式**: `<RESOURCE_NUM>=<soft>:<hard>`
///    ```text
///    0=10:20          # 0 对应 RLIMIT_CPU
///    7=1000:2000      # 7 对应 RLIMIT_NOFILE
///    ```
///
/// ## 解析流程
///
/// ```text
/// from_str(s)
///   │
///   ├─> 1. 按 '=' 分割为两部分
///   │     └─ 如果分割失败，返回 InvalidRLimitFormat
///   │
///   ├─> 2. 解析资源类型
///   │     ├─ 尝试解析为 u32（数字格式）
///   │     └─ 如果失败，尝试解析为字符串（名称格式）
///   │
///   ├─> 3. 按 ':' 分割限制值
///   │     └─ 如果分割失败，返回 InvalidRLimitFormat
///   │
///   ├─> 4. 解析软限制和硬限制
///   │     └─ 如果解析失败，返回 InvalidRLimitValue
///   │
///   └─> 5. 创建 LinuxRlimit 实例
/// ```
///
/// ## 错误处理
///
/// | 错误 | 触发条件 |
/// |------|----------|
/// | `InvalidRLimitFormat` | 格式不是 `XXX=YYY:ZZZ` |
/// | `InvalidRLimitResource` | 资源名称或编号无效 |
/// | `InvalidRLimitValue` | 软/硬限制值不是有效数字 |
impl FromStr for LinuxRlimit {
    type Err = MicrosandboxError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // 按 '=' 分割资源名和限制值
        let parts: Vec<&str> = s.split('=').collect();
        if parts.len() != 2 {
            return Err(MicrosandboxError::InvalidRLimitFormat(s.to_string()));
        }

        // 解析资源类型（支持数字或名称）
        let resource = if let Ok(resource_num) = parts[0].parse::<u32>() {
            // 数字格式：尝试转换为枚举
            LinuxRLimitResource::try_from(resource_num)?
        } else {
            // 名称格式：尝试从字符串解析
            parts[0].parse()?
        };

        // 按 ':' 分割软限制和硬限制
        let limits: Vec<&str> = parts[1].split(':').collect();
        if limits.len() != 2 {
            return Err(MicrosandboxError::InvalidRLimitFormat(s.to_string()));
        }

        // 解析软限制和硬限制
        let soft = limits[0]
            .parse()
            .map_err(|_| MicrosandboxError::InvalidRLimitValue(limits[0].to_string()))?;
        let hard = limits[1]
            .parse()
            .map_err(|_| MicrosandboxError::InvalidRLimitValue(limits[1].to_string()))?;

        Ok(Self::new(resource, soft, hard))
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRlimit 的 Display 实现
//--------------------------------------------------------------------------------------------------

/// 实现 Display trait，支持使用 `{}` 格式化输出
///
/// 输出格式为：`<resource_int>=<soft>:<hard>`
///
/// ## 示例
///
/// ```
/// use microsandbox_core::vm::{LinuxRlimit, LinuxRLimitResource};
///
/// let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_CPU, 10, 20);
/// assert_eq!(rlimit.to_string(), "0=10:20");
///
/// let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_NOFILE, 1000, 2000);
/// assert_eq!(rlimit.to_string(), "7=1000:2000");
/// ```
impl fmt::Display for LinuxRlimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 使用资源的整数值格式化输出
        write!(f, "{}={}:{}", self.resource.as_int(), self.soft, self.hard)
    }
}

//--------------------------------------------------------------------------------------------------
// LinuxRlimit 的序列化实现
//--------------------------------------------------------------------------------------------------

/// 实现 Serialize trait，支持序列化为 JSON
///
/// 序列化为字符串格式（如 `"0=10:20"`）。
impl Serialize for LinuxRlimit {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // 序列化为字符串
        serializer.serialize_str(&self.to_string())
    }
}

/// 实现 Deserialize trait，支持从 JSON 反序列化
///
/// 从字符串格式解析（如 `"0=10:20"`）。
impl<'de> Deserialize<'de> for LinuxRlimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // 先反序列化为字符串
        let s = String::deserialize(deserializer)?;
        // 然后使用 FromStr 解析
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
    fn test_linux_rlimit_resource_from_u32() -> anyhow::Result<()> {
        assert_eq!(
            LinuxRLimitResource::try_from(0)?,
            LinuxRLimitResource::RLIMIT_CPU
        );
        assert_eq!(
            LinuxRLimitResource::try_from(7)?,
            LinuxRLimitResource::RLIMIT_NOFILE
        );
        assert_eq!(
            LinuxRLimitResource::try_from(15)?,
            LinuxRLimitResource::RLIMIT_RTTIME
        );
        assert!(LinuxRLimitResource::try_from(16).is_err());
        Ok(())
    }

    #[test]
    fn test_linux_rlimit_resource_as_int() {
        assert_eq!(LinuxRLimitResource::RLIMIT_CPU.as_int(), 0);
        assert_eq!(LinuxRLimitResource::RLIMIT_NOFILE.as_int(), 7);
        assert_eq!(LinuxRLimitResource::RLIMIT_RTTIME.as_int(), 15);
    }

    #[test]
    fn test_linux_rlimit_resource_from_str() -> anyhow::Result<()> {
        assert_eq!(
            "RLIMIT_CPU".parse::<LinuxRLimitResource>()?,
            LinuxRLimitResource::RLIMIT_CPU
        );
        assert_eq!(
            "RLIMIT_NOFILE".parse::<LinuxRLimitResource>()?,
            LinuxRLimitResource::RLIMIT_NOFILE
        );
        assert_eq!(
            "RLIMIT_RTTIME".parse::<LinuxRLimitResource>()?,
            LinuxRLimitResource::RLIMIT_RTTIME
        );
        assert!("RLIMIT_INVALID".parse::<LinuxRLimitResource>().is_err());
        Ok(())
    }

    #[test]
    fn test_linux_rlimit_resource_display() {
        assert_eq!(LinuxRLimitResource::RLIMIT_CPU.to_string(), "RLIMIT_CPU");
        assert_eq!(
            LinuxRLimitResource::RLIMIT_NOFILE.to_string(),
            "RLIMIT_NOFILE"
        );
        assert_eq!(
            LinuxRLimitResource::RLIMIT_RTTIME.to_string(),
            "RLIMIT_RTTIME"
        );
    }

    #[test]
    fn test_linux_rlimit_new() {
        let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_CPU, 10, 20);
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_CPU);
        assert_eq!(rlimit.soft, 10);
        assert_eq!(rlimit.hard, 20);

        let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_NOFILE, 1000, 2000);
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_NOFILE);
        assert_eq!(rlimit.soft, 1000);
        assert_eq!(rlimit.hard, 2000);
    }

    #[test]
    fn test_linux_rlimit_from_str_with_rlimit_syntax() -> anyhow::Result<()> {
        let rlimit: LinuxRlimit = "RLIMIT_CPU=10:20".parse()?;
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_CPU);
        assert_eq!(rlimit.soft, 10);
        assert_eq!(rlimit.hard, 20);

        let rlimit: LinuxRlimit = "RLIMIT_NOFILE=1000:2000".parse()?;
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_NOFILE);
        assert_eq!(rlimit.soft, 1000);
        assert_eq!(rlimit.hard, 2000);

        let rlimit: LinuxRlimit = "RLIMIT_AS=1048576:2097152".parse()?;
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_AS);
        assert_eq!(rlimit.soft, 1048576);
        assert_eq!(rlimit.hard, 2097152);

        assert!("RLIMIT_INVALID=10:20".parse::<LinuxRlimit>().is_err());
        assert!("RLIMIT_CPU=10".parse::<LinuxRlimit>().is_err());
        assert!("RLIMIT_CPU=10:".parse::<LinuxRlimit>().is_err());
        assert!("RLIMIT_CPU=:20".parse::<LinuxRlimit>().is_err());
        Ok(())
    }

    #[test]
    fn test_linux_rlimit_from_str_mixed_syntax() -> anyhow::Result<()> {
        let rlimit: LinuxRlimit = "0=10:20".parse()?;
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_CPU);
        assert_eq!(rlimit.soft, 10);
        assert_eq!(rlimit.hard, 20);

        let rlimit: LinuxRlimit = "RLIMIT_NOFILE=1000:2000".parse()?;
        assert_eq!(rlimit.resource, LinuxRLimitResource::RLIMIT_NOFILE);
        assert_eq!(rlimit.soft, 1000);
        assert_eq!(rlimit.hard, 2000);

        Ok(())
    }

    #[test]
    fn test_linux_rlimit_display() {
        let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_CPU, 10, 20);
        assert_eq!(rlimit.to_string(), "0=10:20");

        let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_NOFILE, 1000, 2000);
        assert_eq!(rlimit.to_string(), "7=1000:2000");
    }

    #[test]
    fn test_linux_rlimit_serialize_deserialize() -> anyhow::Result<()> {
        let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_CPU, 10, 20);
        let serialized = serde_json::to_string(&rlimit)?;
        assert_eq!(serialized, "\"0=10:20\"");

        let deserialized: LinuxRlimit = serde_json::from_str(&serialized)?;
        assert_eq!(deserialized, rlimit);

        let rlimit = LinuxRlimit::new(LinuxRLimitResource::RLIMIT_NOFILE, 1000, 2000);
        let serialized = serde_json::to_string(&rlimit)?;
        assert_eq!(serialized, "\"7=1000:2000\"");

        let deserialized: LinuxRlimit = serde_json::from_str(&serialized)?;
        assert_eq!(deserialized, rlimit);

        Ok(())
    }
}
