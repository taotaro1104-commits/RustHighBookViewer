use std::io;

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive error: {0}")]
    Archive(String),
    #[error("cancelled")]
    Cancelled,
    #[error("unsupported format")]
    Unsupported,
    #[error("entry {0} not found")]
    NotFound(u32),
    #[error("no usable font for text epub")]
    NoFont,
    #[error("limit exceeded: {0}")]
    LimitExceeded(&'static str),
}

pub type Result<T> = std::result::Result<T, ContainerError>;
