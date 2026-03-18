//! # 构建器模式模块
//!
//! 这个模块实现了 Rust 中常见的"构建器模式"（Builder Pattern），用于方便地
//! 创建和配置 [`SandboxOptions`] 结构体。
//!
//! ## 什么是构建器模式？
//!
//! 构建器模式是一种创建型设计模式，它允许你分步骤地创建复杂对象。
//! 相比于直接使用构造函数，构建器模式有以下优点：
//!
//! 1. **链式调用**：可以流畅地链式设置多个选项
//! 2. **可选参数**：不需要为所有参数提供值，未设置的字段使用默认值
//! 3. **类型安全**：编译器会检查所有必需的字段
//! 4. **不可变性**：每次调用返回新实例，避免意外修改
//!
//! ## 使用示例
//!
//! ```rust
//! use microsandbox_sdk::SandboxOptions;
//!
//! // 使用构建器创建配置
//! let options = SandboxOptions::builder()
//!     .server_url("http://localhost:5555")
//!     .name("my-sandbox")
//!     .api_key("my-secret-key")
//!     .build();
//!
//! // 只设置部分选项（其他使用默认值）
//! let options = SandboxOptions::builder()
//!     .name("simple-sandbox")
//!     .build();
//!
//! // 完全不设置（全部使用默认值）
//! let options = SandboxOptions::builder().build();
//! ```
//!
//! ## 配置优先级
//!
//! 最终生效的配置来源优先级为：
//! 1. 通过 builder 设置的值
//! 2. 环境变量（`MSB_SERVER_URL`、`MSB_API_KEY`）
//! 3. 默认值

/// # 沙箱配置选项
///
/// `SandboxOptions` 包含创建沙箱时需要的配置信息。
/// 这个结构体设计为通过 [`SandboxOptionsBuilder`] 来创建，而不是直接构造。
///
/// ## 字段可见性
///
/// 所有字段都标记为 `pub(crate)`，这意味着：
/// - crate 内部的代码可以直接访问这些字段
/// - crate 外部的代码只能通过公共方法（如 `SandboxBase::new`）间接使用
///
/// 这种设计确保了配置的封装性和一致性。
///
/// ## 字段说明
#[derive(Debug, Clone)]
pub struct SandboxOptions {
    /// Microsandbox 服务器的 URL 地址
    ///
    /// 可选字段，如果为 `None`，将尝试从环境变量 `MSB_SERVER_URL` 获取，
    /// 如果环境变量也不存在，则使用默认值 `http://127.0.0.1:5555`。
    pub(crate) server_url: Option<String>,

    /// 沙箱的名称
    ///
    /// 可选字段，如果为 `None`，系统将自动生成一个带随机前缀的名称，
    /// 如 `sandbox-a1b2c3d4`。名称用于在服务器上唯一标识沙箱。
    pub(crate) name: Option<String>,

    /// 用于 Microsandbox 服务器身份验证的 API 密钥
    ///
    /// 可选字段，如果为 `None`，将尝试从环境变量 `MSB_API_KEY` 获取。
    /// 如果环境变量也不存在，请求将以未认证方式发送。
    pub(crate) api_key: Option<String>,
}

/// # 沙箱配置选项构建器
///
/// `SandboxOptionsBuilder` 是用于构建 [`SandboxOptions`] 的辅助类型。
/// 它实现了流式的链式调用 API，让配置创建更加优雅。
///
/// ## 设计原理
///
/// ### 为什么使用构建器？
///
/// 如果直接使用结构体字面量创建 `SandboxOptions`，代码会是这样：
///
/// ```rust,ignore
/// // 不使用构建器 - 较繁琐
/// let options = SandboxOptions {
///     server_url: Some("http://localhost:5555".to_string()),
///     name: Some("my-sandbox".to_string()),
///     api_key: Some("my-key".to_string()),
/// };
/// ```
///
/// 使用构建器后：
///
/// ```rust
/// # use microsandbox_sdk::SandboxOptions;
/// // 使用构建器 - 更简洁
/// let options = SandboxOptions::builder()
///     .server_url("http://localhost:5555")
///     .name("my-sandbox")
///     .api_key("my-key")
///     .build();
/// ```
///
/// ### `#[derive(Default)]` 的作用
///
/// `SandboxOptionsBuilder` 派生了 `Default` trait，这意味着：
/// - 可以使用 `SandboxOptionsBuilder::default()` 创建空构建器
/// - 所有字段初始化为 `None`
///
/// `SandboxOptions::builder()` 方法实际上就是返回 `Default::default()`。
#[derive(Debug, Clone, Default)]
pub struct SandboxOptionsBuilder {
    /// 服务器 URL 配置
    server_url: Option<String>,
    /// 沙箱名称配置
    name: Option<String>,
    /// API 密钥配置
    api_key: Option<String>,
}

impl SandboxOptions {
    /// # 创建新的构建器
    ///
    /// 这是创建 `SandboxOptions` 的推荐方式。
    /// 返回一个空的 `SandboxOptionsBuilder` 实例。
    ///
    /// ## 返回
    ///
    /// 返回一个所有字段都为 `None` 的 `SandboxOptionsBuilder`。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::SandboxOptions;
    ///
    /// let builder = SandboxOptions::builder();
    /// // 然后可以链式调用设置方法...
    /// ```
    pub fn builder() -> SandboxOptionsBuilder {
        SandboxOptionsBuilder::default()
    }
}

impl SandboxOptionsBuilder {
    /// # 设置服务器 URL
    ///
    /// 指定 Microsandbox 服务器的地址。
    ///
    /// ## 参数
    ///
    /// * `url` - 服务器 URL，可以是任何可转换为 `String` 的类型
    ///   - `&str`: `"http://localhost:5555"`
    ///   - `String`: `my_string_variable`
    ///   - `Cow<str>` 等
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// # use microsandbox_sdk::SandboxOptions;
    /// let options = SandboxOptions::builder()
    ///     .server_url("http://localhost:5555")
    ///     .build();
    /// ```
    ///
    /// ## 注意
    ///
    /// 如果未设置，将使用默认值 `http://127.0.0.1:5555`。
    pub fn server_url(mut self, url: impl Into<String>) -> Self {
        self.server_url = Some(url.into());
        self
    }

    /// # 设置沙箱名称
    ///
    /// 为沙箱指定一个唯一的名称。名称用于在服务器上标识和管理沙箱。
    ///
    /// ## 参数
    ///
    /// * `name` - 沙箱名称，可以是任何可转换为 `String` 的类型
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 命名建议
    ///
    /// - 使用有意义的名称，便于调试和日志分析
    /// - 避免特殊字符，推荐使用字母、数字和连字符
    /// - 如果未设置，系统会生成类似 `sandbox-a1b2c3d4` 的随机名称
    ///
    /// ## 示例
    ///
    /// ```rust
    /// # use microsandbox_sdk::SandboxOptions;
    /// let options = SandboxOptions::builder()
    ///     .name("production-python-sandbox")
    ///     .build();
    /// ```
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// # 设置 API 密钥
    ///
    /// 设置用于服务器身份验证的 API 密钥。
    ///
    /// ## 参数
    ///
    /// * `api_key` - API 密钥字符串，可以是任何可转换为 `String` 的类型
    ///
    /// ## 返回值
    ///
    /// 返回更新后的构建器实例，支持链式调用。
    ///
    /// ## 安全提示
    ///
    /// - 永远不要将 API 密钥硬编码在源代码中
    /// - 从环境变量或安全的配置管理工具中获取
    /// - 使用 `.env` 文件时，确保将其添加到 `.gitignore`
    ///
    /// ## 示例
    ///
    /// ```rust
    /// # use microsandbox_sdk::SandboxOptions;
    /// let api_key = std::env::var("MSB_API_KEY").unwrap();
    /// let options = SandboxOptions::builder()
    ///     .api_key(api_key)
    ///     .build();
    /// ```
    pub fn api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// # 构建 SandboxOptions
    ///
    /// 完成配置并创建 `SandboxOptions` 实例。
    ///
    /// ## 返回值
    ///
    /// 返回一个配置好的 `SandboxOptions` 实例，可以在创建沙箱时使用。
    ///
    /// ## 示例
    ///
    /// ```rust
    /// use microsandbox_sdk::SandboxOptions;
    ///
    /// let options = SandboxOptions::builder()
    ///     .server_url("http://localhost:5555")
    ///     .name("my-sandbox")
    ///     .api_key("secret-key")
    ///     .build();
    /// ```
    ///
    /// ## 下一步
    ///
    /// 创建 `SandboxOptions` 后，可以将其传递给沙箱创建方法：
    ///
    /// ```rust,no_run
    /// # use microsandbox_sdk::{PythonSandbox, SandboxOptions};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let options = SandboxOptions::builder()
    ///     .name("my-sandbox")
    ///     .build();
    ///
    /// let sandbox = PythonSandbox::create_with_options(options).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> SandboxOptions {
        SandboxOptions {
            server_url: self.server_url,
            name: self.name,
            api_key: self.api_key,
        }
    }
}
