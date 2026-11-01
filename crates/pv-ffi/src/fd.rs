//! Android の content:// から得たファイルディスクリプタを Source にする。
//! 読み出しは pread（FileSource::read_at）なので、SAF 経由でもシーク位置を共有しない。

use std::io;
use std::sync::Arc;

#[cfg(unix)]
use pv_core::FileSource;
use pv_core::Source;

/// `fd` の所有権を受け取る（Kotlin 側は ParcelFileDescriptor.detachFd() で渡す）。
/// 以降は Rust 側の File が閉じる。
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn source_from_fd(fd: i32) -> io::Result<Arc<dyn Source>> {
    use std::os::fd::FromRawFd;
    if fd < 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid fd"));
    }
    // SAFETY: 呼び出し元が所有権を手放した有効な fd であることが契約。二重 close を避けるため
    // ここ以外では閉じない。
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    Ok(Arc::new(FileSource::from_file(file)?))
}

#[cfg(not(unix))]
pub fn source_from_fd(_fd: i32) -> io::Result<Arc<dyn Source>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "file descriptors are only supported on unix",
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::fd::IntoRawFd;

    #[test]
    fn reads_through_a_raw_fd() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"0123456789").unwrap();
        let fd = std::fs::File::open(&p).unwrap().into_raw_fd();
        let src = source_from_fd(fd).unwrap();
        let mut buf = [0u8; 4];
        assert_eq!(src.len(), 10);
        assert_eq!(src.read_at(3, &mut buf).unwrap(), 4);
        assert_eq!(&buf, b"3456");
    }

    #[test]
    fn negative_fd_is_rejected() {
        assert!(source_from_fd(-1).is_err());
    }
}

#[cfg(all(test, not(unix)))]
mod tests {
    use super::*;

    #[test]
    fn fd_unsupported_on_this_platform() {
        assert!(source_from_fd(3).is_err());
    }
}
