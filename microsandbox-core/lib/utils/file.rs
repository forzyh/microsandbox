//! 文件处理工具函数
//!
//! 本模块提供了与文件操作相关的实用函数。
//! 主要用于文件哈希计算和文件内容处理。

use std::path::Path;

use oci_spec::image::DigestAlgorithm;
use sha2::{Digest, Sha256, Sha384, Sha512};
use tokio::{fs::File, io::AsyncReadExt};

use crate::{MicrosandboxError, MicrosandboxResult};

//--------------------------------------------------------------------------------------------------
// 函数实现
//--------------------------------------------------------------------------------------------------

/// 获取文件的哈希值
///
/// 此函数读取整个文件内容并使用指定的哈希算法计算摘要。
/// 主要用于 OCI 镜像层的验证。
///
/// ## 参数
/// * `path` - 要计算哈希的文件路径
/// * `algorithm` - 使用的哈希算法（SHA-256、SHA-384 或 SHA-512）
///
/// ## 返回值
/// * `Ok(Vec<u8>)` - 返回哈希值的字节向量
/// * `Err(MicrosandboxError)` - 文件读取失败或算法不支持
///
/// ## 支持的算法
/// * `Sha256` - SHA-256（256 位，最常用）
/// * `Sha384` - SHA-384（384 位）
/// * `Sha512` - SHA-512（512 位）
///
/// ## 注意事项
/// 此函数会读取整个文件到内存，对于大文件可能需要较多内存。
pub async fn get_file_hash(
    path: &Path,
    algorithm: &DigestAlgorithm,
) -> MicrosandboxResult<Vec<u8>> {
    let mut file = File::open(path).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;

    let hash = match algorithm {
        DigestAlgorithm::Sha256 => Sha256::digest(&buffer).to_vec(),
        DigestAlgorithm::Sha384 => Sha384::digest(&buffer).to_vec(),
        DigestAlgorithm::Sha512 => Sha512::digest(&buffer).to_vec(),
        _ => {
            return Err(MicrosandboxError::UnsupportedImageHashAlgorithm(format!(
                "Unsupported algorithm: {}",
                algorithm
            )));
        }
    };

    Ok(hash)
}
