//! # 错误处理模块
//!
//! 本模块定义了 microsandbox 项目的统一错误类型和错误处理机制。
//!
//! ## 核心类型
//!
//! ### [`MicrosandboxUtilsError`]
//! 这是本库的主要错误类型，是一个枚举类型，可以表示多种不同的错误：
//! - 路径验证错误 (`PathValidation`)
//! - 文件未找到错误 (`FileNotFound`)
//! - IO 错误 (`IoError`)
//! - 运行时错误 (`Runtime`)
//! - nix 库错误 (`NixError`)
//! - 自定义错误 (`Custom`)
//!
//! ### [`MicrosandboxUtilsResult<T>`]
//! 这是一个类型别名，定义为 `Result<T, MicrosandboxUtilsError>`。
//! 使用这个别名可以让函数签名更简洁，语义更清晰。
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_utils::{MicosandboxUtilsResult, MicrosandboxUtilsError};
//! use std::fs;
//!
//! // 函数返回统一的结果类型
//! fn read_config_file() -> MicrosandboxUtilsResult<String> {
//!     // IO 错误会自动转换为 MicrosandboxUtilsError
//!     let content = fs::read_to_string("config.yaml")?;
//!     Ok(content)
//! }
//!
//! // 手动创建错误
//! fn validate_path(path: &str) -> MicrosandboxUtilsResult<()> {
//!     if path.is_empty() {
//!         return Err(MicrosandboxUtilsError::PathValidation(
//!             "路径不能为空".to_string()
//!         ));
//!     }
//!     Ok(())
//! }
//!
//! // 使用自定义错误
//! fn custom_operation() -> MicrosandboxUtilsResult<()> {
//!     Err(MicrosandboxUtilsError::custom("发生了自定义错误"))
//! }
//! ```
//!
//! ## 错误转换机制（From trait）
//!
//! 本模块实现了多个 `From` trait，允许其他错误类型自动转换为 `MicosandboxUtilsError`：
//!
//! ```rust,ignore
//! // std::io::Error -> MicrosandboxUtilsError
//! let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
//! let utils_err: MicrosandboxUtilsError = io_err.into();
//!
//! // nix::Error -> MicrosandboxUtilsError
//! let nix_err = nix::Error::ENOENT;
//! let utils_err: MicrosandboxUtilsError = nix_err.into();
//!
//! // anyhow::Error -> MicrosandboxUtilsError (通过 Custom 变体)
//! let anyhow_err = anyhow::anyhow!("something went wrong");
//! let utils_err: MicrosandboxUtilsError = anyhow_err.into();
//! ```
//!
//! ## ? 操作符的工作原理
//!
//! 当函数返回 `MicosandboxUtilsResult<T>` 时，可以使用 `?` 操作符：
//!
//! ```rust,ignore
//! fn my_function() -> MicrosandboxUtilsResult<()> {
//!     // 如果 read_to_string 返回 Err(io::Error)，? 会自动将其转换为
//!     // MicrosandboxUtilsError::IoError，然后从函数中返回
//!     let content = std::fs::read_to_string("file.txt")?;
//!     Ok(())
//! }
//! ```

use std::{
    error::Error,
    fmt::{self, Display},
};

use thiserror::Error;

//--------------------------------------------------------------------------------------------------
// 类型定义
//--------------------------------------------------------------------------------------------------

/// ### microsandbox 工具库的结果类型
///
/// 这是一个类型别名（type alias），定义为：
/// ```rust,ignore
/// pub type MicrosandboxUtilsResult<T> = Result<T, MicrosandboxUtilsError>;
/// ```
///
/// ## 为什么使用类型别名？
/// 1. **简洁性**: `MicosandboxUtilsResult<T>` 比 `Result<T, MicrosandboxUtilsError>` 更短
/// 2. **一致性**: 所有函数使用统一的结果类型
/// 3. **可维护性**: 如果要修改错误类型，只需在一处修改
///
/// ## 泛型参数 T
/// `T` 是成功时的返回值类型，可以是任何类型：
/// - `MicosandboxUtilsResult<()>`: 没有返回值，只表示成功或失败
/// - `MicosandboxUtilsResult<String>`: 成功时返回 String
/// - `MicosandboxUtilsResult<Vec<u8>>`: 成功时返回字节向量
pub type MicrosandboxUtilsResult<T> = Result<T, MicrosandboxUtilsError>;

/// ### microsandbox 工具库的错误类型
///
/// 这是一个枚举类型，使用 `thiserror` 派生宏来自动生成错误处理的样板代码。
///
/// ## thiserror 简介
/// `thiserror` 是一个用于定义错误类型的过程宏（procedural macro），它：
/// - 自动实现 `std::error::Error` trait
/// - 自动实现 `Display` trait（根据 `#[error("...")]` 属性）
/// - 自动处理错误来源（通过 `#[from]` 属性）
///
/// ## 变体说明
///
/// ### `PathValidation(String)`
/// 路径验证失败时的错误。例如：路径为空、路径遍历攻击等。
///
/// ### `FileNotFound(String, String)`
/// 文件未找到的错误。第一个参数是文件路径，第二个参数是来源说明。
///
/// ### `IoError(std::io::Error)`
/// 标准 IO 错误的包装。使用 `#[from]` 属性，可以从 `std::io::Error` 自动转换。
///
/// ### `Runtime(String)`
/// 运行时错误的通用类型。用于表示不适合其他变体的运行时错误。
///
/// ### `NixError(nix::Error)`
/// nix 库错误的包装。nix 是一个 Rust 的 Unix 系统 API 绑定库。
///
/// ### `Custom(AnyError)`
/// 自定义错误的包装。使用 `anyhow::Error` 作为底层错误类型。
#[derive(pretty_error_debug::Debug, Error)]
pub enum MicrosandboxUtilsError {
    /// ### 路径验证错误
    ///
    /// 当路径验证失败时返回此错误。
    ///
    /// ## 常见原因
    /// - 路径为空字符串
    /// - 路径尝试遍历到根目录之上（如 `../../../etc/passwd`）
    /// - 路径类型不符合要求（要求绝对路径但提供了相对路径）
    ///
    /// ## 显示格式
    /// ```text
    /// path validation error: {错误详情}
    /// ```
    #[error("path validation error: {0}")]
    PathValidation(String),

    /// ### 文件未找到错误
    ///
    /// 当尝试访问的文件不存在时返回此错误。
    ///
    /// ## 参数说明
    /// - 第一个 `String`: 未找到的文件路径
    /// - 第二个 `String`: 文件来源说明（如"环境变量"、"默认路径"）
    ///
    /// ## 显示格式
    /// ```text
    /// file not found at: {文件路径}
    /// Source: {来源说明}
    /// ```
    #[error("file not found at: {0}\nSource: {1}")]
    FileNotFound(String, String),

    /// ### IO 错误
    ///
    /// 包装标准库的 IO 错误。
    ///
    /// ## `#[from]` 属性的作用
    /// 这个属性告诉 `thiserror` 自动生成 `From<std::io::Error>` 的实现，
    /// 使得 `std::io::Error` 可以自动转换为 `MicosandboxUtilsError`。
    ///
    /// ## 使用示例
    /// ```rust,ignore
    /// fn read_file() -> MicrosandboxUtilsResult<String> {
    ///     // std::io::Error 会自动转换为 IoError 变体
    ///     let content = std::fs::read_to_string("file.txt")?;
    ///     Ok(content)
    /// }
    /// ```
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    /// ### 运行时错误
    ///
    /// 用于表示运行时发生的错误，通常用于不适合其他变体的情况。
    ///
    /// ## 使用场景
    /// - 进程启动失败
    /// - 资源分配失败
    /// - 状态不一致
    #[error("runtime error: {0}")]
    Runtime(String),

    /// ### nix 库错误
    ///
    /// 包装 nix 库的错误类型。
    ///
    /// ## 关于 nix 库
    /// nix 是一个 Rust 库，提供了 Unix 系统 API（如 pthread、socket、ioctl 等）
    /// 的安全绑定。当这些系统调用失败时，会返回 `nix::Error`。
    ///
    /// ## 常见错误
    /// - `ENOENT`: 文件或目录不存在
    /// - `EACCES`: 权限不足
    /// - `EBUSY`: 资源繁忙
    #[error("nix error: {0}")]
    NixError(#[from] nix::Error),

    /// ### 自定义错误
    ///
    /// 使用 `anyhow::Error` 作为底层错误类型，可以包装任何错误。
    ///
    /// ## 关于 anyhow
    /// `anyhow` 是一个用于应用层的错误处理库，提供了灵活的错误包装机制。
    /// `anyhow::Error` 可以包装任何实现了 `std::error::Error` 的类型。
    #[error("Custom error: {0}")]
    Custom(#[from] AnyError),
}

/// ### 通用错误包装器
///
/// `AnyError` 是一个可以包装任何错误的类型，内部使用 `anyhow::Error`。
///
/// ## 设计目的
/// 提供一个统一的接口来包装和处理各种不同类型的错误，
/// 特别是那些无法预定义具体类型的错误。
///
/// ## 字段说明
/// - `error: anyhow::Error`: 底层包装的实际错误
#[derive(Debug)]
pub struct AnyError {
    error: anyhow::Error,
}

//--------------------------------------------------------------------------------------------------
// 方法实现
//--------------------------------------------------------------------------------------------------

impl MicrosandboxUtilsError {
    /// ### 创建自定义错误
    ///
    /// 这是一个便捷方法，用于创建 `Custom` 变体的错误。
    ///
    /// ## 参数
    /// - `error`: 任何可以转换为 `anyhow::Error` 的类型
    ///
    /// ## 返回值
    /// 返回一个 `MicosandboxUtilsError::Custom` 变体
    ///
    /// ## 参数类型说明
    /// `impl Into<anyhow::Error>` 表示任何可以实现 `Into<anyhow::Error>` 的类型都可以作为参数。
    /// 这包括：
    /// - `&str` 和 `String`: 会自动创建为消息错误
    /// - 任何实现 `std::error::Error` 的类型
    /// - `anyhow::Error` 本身
    ///
    /// ## 使用示例
    /// ```rust
    /// use microsandbox_utils::MicosandboxUtilsError;
    ///
    /// // 使用字符串创建
    /// let err = MicrosandboxUtilsError::custom("发生了错误");
    ///
    /// // 使用 anyhow::anyhow! 创建
    /// let err = MicrosandboxUtilsError::custom(
    ///     anyhow::anyhow!("带上下文的错误")
    /// );
    /// ```
    pub fn custom(error: impl Into<anyhow::Error>) -> MicrosandboxUtilsError {
        MicrosandboxUtilsError::Custom(AnyError {
            error: error.into(),
        })
    }
}

impl AnyError {
    /// ### 向下转型错误类型
    ///
    /// 尝试将内部包装的错误向下转型为具体的错误类型 `T`。
    ///
    /// ## 参数
    /// 无（方法通过 `&self` 访问内部错误）
    ///
    /// ## 返回值
    /// - `Some(&T)`: 如果内部错误确实是类型 `T`
    /// - `None`: 如果内部错误不是类型 `T`
    ///
    /// ## 泛型约束说明
    /// ```rust,ignore
    /// where
    ///     T: Display + fmt::Debug + Send + Sync + 'static,
    /// ```
    /// 这些约束是 `anyhow::Error::downcast_ref()` 要求的：
    /// - `Display`: 可以格式化为人类可读的消息
    /// - `Debug`: 可以格式化为调试信息
    /// - `Send + Sync`: 可以在线程间安全传递和共享
    /// - `'static`: 错误类型不包含非静态引用
    ///
    /// ## 使用示例
    /// ```rust
    /// use microsandbox_utils::AnyError;
    /// use std::io;
    ///
    /// // 创建一个包装了 io::Error 的 AnyError
    /// let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    /// let any_err = AnyError { error: anyhow::Error::from(io_err) };
    ///
    /// // 尝试向下转型
    /// if let Some(io_err) = any_err.downcast::<io::Error>() {
    ///     println!("IO 错误：{}", io_err);
    /// }
    /// ```
    pub fn downcast<T>(&self) -> Option<&T>
    where
        T: Display + fmt::Debug + Send + Sync + 'static,
    {
        self.error.downcast_ref::<T>()
    }
}

//--------------------------------------------------------------------------------------------------
// 辅助函数
//--------------------------------------------------------------------------------------------------

/// ### 创建成功的结果
///
/// 这是一个便捷函数，用于创建 `Ok` 变体的 `MicosandboxUtilsResult`。
///
/// ## 注意
/// 这个函数使用 `#[allow(non_snake_case)]` 属性来允许使用大写字母开头，
/// 目的是与 Rust 标准库的 `Result::Ok` 保持一致的命名风格。
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::error::Ok;
///
/// // 创建一个成功的结果
/// let result: MicrosandboxUtilsResult<i32> = Ok(42);
/// ```
///
/// ## 注意
/// 在实际使用中，通常直接使用 `Ok()`（从作用域内导入时）或 `Result::Ok()`，
/// 这个函数主要是为了模块内部的统一性。
#[allow(non_snake_case)]
pub fn Ok<T>(value: T) -> MicrosandboxUtilsResult<T> {
    Result::Ok(value)
}

//--------------------------------------------------------------------------------------------------
// Trait 实现
//--------------------------------------------------------------------------------------------------

/// ### AnyError 的 PartialEq 实现
///
/// 比较两个 `AnyError` 是否相等。
///
/// ## 比较逻辑
/// 通过比较两个错误的显示字符串（`to_string()`）来判断是否相等。
/// 这意味着如果两个错误的消息相同，它们就被认为是相等的。
///
/// ## 注意
/// 这种比较方式可能不够精确，因为不同的错误可能产生相同的消息字符串。
/// 但对于大多数用例来说已经足够。
impl PartialEq for AnyError {
    fn eq(&self, other: &Self) -> bool {
        self.error.to_string() == other.error.to_string()
    }
}

/// ### AnyError 的 Display 实现
///
/// 实现 `Display` trait 使得 `AnyError` 可以被格式化为人类可读的字符串。
///
/// ## 使用示例
/// ```rust
/// use microsandbox_utils::AnyError;
/// use std::fmt::Display;
///
/// let any_err = AnyError { error: anyhow::anyhow!("测试错误") };
/// println!("{}", any_err);  // 输出：测试错误
/// ```
impl Display for AnyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 将内部错误的显示结果写入格式化器
        write!(f, "{}", self.error)
    }
}

/// ### AnyError 的 Error 实现
///
/// 实现 `std::error::Error` trait，使得 `AnyError` 可以作为标准错误类型使用。
///
/// ## 注意
/// 这里使用空实现（不包含任何方法），因为 `thiserror` 或其他宏
/// 会自动提供默认实现。只需要声明实现即可。
impl Error for AnyError {}
