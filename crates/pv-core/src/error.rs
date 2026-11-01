#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("container: {0}")]
    Container(#[from] pv_container::ContainerError),
    #[error("decode: {0}")]
    Decode(#[from] pv_decode::DecodeError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings: {0}")]
    Settings(String),
    #[error("no readable pages")]
    NoPages,
    #[error("cancelled")]
    Cancelled,
    #[error("internal panic: {0}")]
    Panic(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
