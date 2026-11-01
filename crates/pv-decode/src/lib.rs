//! 画像デコード。寸法はヘッダだけで取得し、表示サイズに近い解像度で縮小デコードする。

mod bitmap;
mod image_decoder;

pub use bitmap::{Bitmap, Size};
pub use image_decoder::ImageDecoder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Jpeg,
    Png,
    WebP,
    Gif,
    Bmp,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("unsupported or unrecognized format")]
    Unsupported,
    #[error("image too large: {0} pixels")]
    TooLarge(u64),
    #[error("decode failed: {0}")]
    Image(#[from] image::ImageError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DecodeError>;

pub trait Decoder: Send + Sync {
    fn formats(&self) -> &[Format];
    /// ヘッダだけで寸法を取得する。
    fn probe_size(&self, data: &[u8]) -> Result<Size>;
    /// `target` が指定されたら、それを下回らない範囲で縮小して返す。
    fn decode(&self, data: &[u8], target: Option<Size>) -> Result<Bitmap>;
}
