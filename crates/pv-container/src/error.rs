use std::io;

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive error: {0}")]
    Archive(String),
    /// 書庫（EPUB など）の中身が、対応していない圧縮方式で入っている
    #[error("unsupported compression method: {0}")]
    UnsupportedCompression(String),
    #[error("cancelled")]
    Cancelled,
    #[error("unsupported format")]
    Unsupported,
    #[error("entry {0} not found")]
    NotFound(u32),
    #[error("no usable font for text epub")]
    NoFont,
    #[error("invalid font: {0}")]
    InvalidFont(String),
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),
}

pub type Result<T> = std::result::Result<T, ContainerError>;
