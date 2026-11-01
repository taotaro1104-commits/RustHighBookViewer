use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

/// ランダムアクセス可能な入力元（ローカルファイル・Android の fd・SMB 等を共通化）。
pub trait Source: Send + Sync {
    fn len(&self) -> u64;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize>;
}

pub struct FileSource {
    file: std::fs::File,
    len: u64,
}

impl FileSource {
    pub fn open(path: &Path) -> io::Result<Self> {
        Self::from_file(std::fs::File::open(path)?)
    }

    /// 既に開いている File から作る（Android の content:// から得た fd など）。
    pub fn from_file(file: std::fs::File) -> io::Result<Self> {
        let len = file.metadata()?.len();
        Ok(Self { file, len })
    }
}

impl Source for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    #[cfg(unix)]
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        std::os::unix::fs::FileExt::read_at(&self.file, buf, off)
    }

    #[cfg(windows)]
    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        std::os::windows::fs::FileExt::seek_read(&self.file, buf, off)
    }
}

pub struct MemorySource(pub bytes::Bytes);

impl Source for MemorySource {
    fn len(&self) -> u64 {
        self.0.len() as u64
    }

    fn read_at(&self, off: u64, buf: &mut [u8]) -> io::Result<usize> {
        let Ok(start) = usize::try_from(off) else {
            return Ok(0);
        };
        if start >= self.0.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.0.len() - start);
        buf[..n].copy_from_slice(&self.0[start..start + n]);
        Ok(n)
    }
}

/// `Source` を `Read + Seek` に見せるアダプタ（zip クレート用）。
pub struct SourceReader {
    src: Arc<dyn Source>,
    pos: u64,
}

impl SourceReader {
    pub fn new(src: Arc<dyn Source>) -> Self {
        Self { src, pos: 0 }
    }
}

impl Read for SourceReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.src.read_at(self.pos, buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for SourceReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let len = self.src.len() as i128;
        let next = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::End(d) => len + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if next < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = next as u64;
        Ok(self.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_source_reads_ranges() {
        let s = MemorySource(bytes::Bytes::from_static(b"abcdef"));
        let mut buf = [0u8; 4];
        assert_eq!(s.read_at(4, &mut buf).unwrap(), 2);
        assert_eq!(&buf[..2], b"ef");
        assert_eq!(s.read_at(10, &mut buf).unwrap(), 0);
    }

    #[test]
    fn reader_seeks_and_reads() {
        let s: Arc<dyn Source> = Arc::new(MemorySource(bytes::Bytes::from_static(b"0123456789")));
        let mut r = SourceReader::new(s);
        r.seek(SeekFrom::End(-3)).unwrap();
        let mut out = String::new();
        r.read_to_string(&mut out).unwrap();
        assert_eq!(out, "789");
        assert!(r.seek(SeekFrom::Current(-100)).is_err());
    }

    #[test]
    fn file_source_reads_at_offset() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.bin");
        std::fs::write(&p, b"hello world").unwrap();
        let s = FileSource::open(&p).unwrap();
        assert_eq!(s.len(), 11);
        let mut buf = [0u8; 5];
        assert_eq!(s.read_at(6, &mut buf).unwrap(), 5);
        assert_eq!(&buf, b"world");
    }
}
