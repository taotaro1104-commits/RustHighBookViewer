use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use zip::ZipArchive;

use crate::container::{Container, ContainerProvider, Limits, Probe};
use crate::entry::{kind_from_path, natural_cmp, EntryId, EntryKind, EntryMeta};
use crate::epub::EpubContainer;
use crate::error::{ContainerError, Result};
use crate::path::{decode_name, normalize_path};
use crate::source::{MemorySource, Source, SourceReader};
use crate::text_epub::TextEpubContainer;

/// ローカルファイルヘッダ / 空アーカイブの終端レコード
const MAGICS: [&[u8; 4]; 2] = [b"PK\x03\x04", b"PK\x05\x06"];

#[derive(Default)]
pub struct ZipProvider {
    pub limits: Option<Limits>,
}

impl ContainerProvider for ZipProvider {
    fn probe(&self, src: &dyn Source) -> Probe {
        let mut head = [0u8; 4];
        match src.read_at(0, &mut head) {
            Ok(4) if MAGICS.iter().any(|m| **m == head) => Probe::Match,
            _ => Probe::NoMatch,
        }
    }

    fn open(&self, src: Arc<dyn Source>) -> Result<Arc<dyn Container>> {
        let zip = ZipContainer::open(src, self.limits.unwrap_or_default())?;
        // EPUB（コミック・写真集）なら、spine の順に画像を並べる。読めなければ通常の ZIP として扱う
        Ok(match EpubContainer::from_zip(zip) {
            Ok(epub) => Arc::new(epub),
            // 画像の無い EPUB は、文章の本として読む（フォントが無ければエラー）
            Err(zip) => match TextEpubContainer::try_open(zip)? {
                Ok(text) => Arc::new(text),
                Err(zip) => Arc::new(zip),
            },
        })
    }
}

/// ZIP/CBZ。セントラルディレクトリのみ先読みし、エントリは要求時に展開する。
pub struct ZipContainer {
    archive: Mutex<ZipArchive<SourceReader>>,
    entries: Vec<EntryMeta>,
    /// EntryId -> zip 内の添字
    zip_index: Vec<usize>,
    limits: Limits,
}

impl ZipContainer {
    pub fn open(src: Arc<dyn Source>, limits: Limits) -> Result<Self> {
        let mut archive = ZipArchive::new(SourceReader::new(src))?;
        if archive.len() > limits.max_entries {
            return Err(ContainerError::LimitExceeded("entry count"));
        }

        let mut found: Vec<(String, u64, usize)> = Vec::with_capacity(archive.len());
        let mut total: u64 = 0;
        for i in 0..archive.len() {
            let f = archive.by_index_raw(i)?;
            if f.is_dir() {
                continue;
            }
            let Some(path) = normalize_path(&decode_name(f.name_raw())) else {
                continue;
            };
            total = total.saturating_add(f.size());
            if total > limits.max_total_size {
                return Err(ContainerError::LimitExceeded("total size"));
            }
            found.push((path, f.size(), i));
        }
        found.sort_by(|a, b| natural_cmp(&a.0, &b.0));

        let mut entries = Vec::with_capacity(found.len());
        let mut zip_index = Vec::with_capacity(found.len());
        for (n, (path, size, idx)) in found.into_iter().enumerate() {
            entries.push(EntryMeta {
                id: EntryId(n as u32),
                kind: kind_from_path(&path),
                path,
                size,
            });
            zip_index.push(idx);
        }
        Ok(Self {
            archive: Mutex::new(archive),
            entries,
            zip_index,
            limits,
        })
    }
}

impl Container for ZipContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        let idx = *self
            .zip_index
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))?;
        let mut archive = self.archive.lock().unwrap_or_else(|e| e.into_inner());
        let file = archive.by_index(idx)?;
        self.limits
            .check_entry(file.size(), file.compressed_size())?;

        // ヘッダの申告サイズを信用せず、実際の読み出し量にも上限をかける
        let cap = self.limits.max_entry_size;
        let mut buf = Vec::with_capacity(file.size().min(cap) as usize);
        file.take(cap + 1).read_to_end(&mut buf)?;
        if buf.len() as u64 > cap {
            return Err(ContainerError::LimitExceeded("entry size"));
        }
        Ok(Bytes::from(buf))
    }

    fn read_prefix(&self, id: EntryId, n: usize) -> Result<Bytes> {
        let idx = *self
            .zip_index
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))?;
        let mut archive = self.archive.lock().unwrap_or_else(|e| e.into_inner());
        let file = archive.by_index(idx)?;
        let mut buf = Vec::new();
        file.take(n as u64).read_to_end(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        // zip の借用の都合上、現状は展開済みバイト列から読む
        Ok(Box::new(Cursor::new(self.read(id)?)))
    }
}

/// 入れ子書庫（EntryKind::Archive）を再帰的に開く。
pub fn open_nested(parent: &dyn Container, id: EntryId) -> Result<Arc<dyn Container>> {
    let meta = parent
        .entries()
        .get(id.0 as usize)
        .ok_or(ContainerError::NotFound(id.0))?;
    if meta.kind != EntryKind::Archive {
        return Err(ContainerError::Unsupported);
    }
    let src: Arc<dyn Source> = Arc::new(MemorySource(parent.read(id)?));
    ZipProvider::default().open(src)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    fn build_zip(files: &[(&str, &[u8])], method: CompressionMethod) -> Bytes {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(method);
        for (name, data) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        Bytes::from(w.finish().unwrap().into_inner())
    }

    fn open(bytes: Bytes, limits: Limits) -> Result<ZipContainer> {
        ZipContainer::open(Arc::new(MemorySource(bytes)), limits)
    }

    #[test]
    fn lists_sorted_and_reads() {
        let z = build_zip(
            &[
                ("p10.png", b"ten"),
                ("p2.png", b"two"),
                ("dir/", b""),
                ("p1.png", b"one"),
            ],
            CompressionMethod::Deflated,
        );
        let c = open(z, Limits::default()).unwrap();
        let names: Vec<_> = c.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["p1.png", "p2.png", "p10.png"]);
        assert_eq!(&c.read(EntryId(2)).unwrap()[..], b"ten");
    }

    #[test]
    fn traversal_names_are_sanitized() {
        let z = build_zip(&[("../../evil.png", b"x")], CompressionMethod::Stored);
        let c = open(z, Limits::default()).unwrap();
        assert_eq!(c.entries()[0].path, "evil.png");
    }

    #[test]
    fn rejects_oversized_entry() {
        let z = build_zip(&[("big.png", &vec![0u8; 4096])], CompressionMethod::Stored);
        let limits = Limits {
            max_entry_size: 1024,
            ..Limits::default()
        };
        let c = open(z, limits).unwrap();
        assert!(matches!(
            c.read(EntryId(0)),
            Err(ContainerError::LimitExceeded(_))
        ));
    }

    #[test]
    fn rejects_zip_bomb_ratio() {
        let z = build_zip(
            &[("bomb.png", &vec![0u8; 8 << 20])],
            CompressionMethod::Deflated,
        );
        let c = open(z, Limits::default()).unwrap();
        assert!(matches!(
            c.read(EntryId(0)),
            Err(ContainerError::LimitExceeded(_))
        ));
    }

    #[test]
    fn rejects_total_size_over_limit() {
        let z = build_zip(
            &[("a.png", &[1u8; 600]), ("b.png", &[2u8; 600])],
            CompressionMethod::Stored,
        );
        let limits = Limits {
            max_total_size: 1000,
            ..Limits::default()
        };
        assert!(matches!(
            open(z, limits),
            Err(ContainerError::LimitExceeded(_))
        ));
    }

    #[test]
    fn nested_archive_opens() {
        let inner = build_zip(&[("1.png", b"inner")], CompressionMethod::Stored);
        let outer = build_zip(&[("vol.cbz", &inner)], CompressionMethod::Stored);
        let c = open(outer, Limits::default()).unwrap();
        assert_eq!(c.entries()[0].kind, EntryKind::Archive);
        let nested = open_nested(&c, EntryId(0)).unwrap();
        assert_eq!(&nested.read(EntryId(0)).unwrap()[..], b"inner");
    }

    #[test]
    fn read_prefix_returns_only_head() {
        let z = build_zip(&[("a.png", b"0123456789")], CompressionMethod::Deflated);
        let c = open(z, Limits::default()).unwrap();
        assert_eq!(&c.read_prefix(EntryId(0), 4).unwrap()[..], b"0123");
        assert_eq!(c.read_prefix(EntryId(0), 100).unwrap().len(), 10);
    }

    #[test]
    fn probe_detects_magic() {
        let z = build_zip(&[("a.png", b"x")], CompressionMethod::Stored);
        let p = ZipProvider::default();
        assert_eq!(p.probe(&MemorySource(z)), Probe::Match);
        assert_eq!(
            p.probe(&MemorySource(Bytes::from_static(b"not a zip"))),
            Probe::NoMatch
        );
    }

    #[test]
    fn garbage_input_is_error_not_panic() {
        let r = open(Bytes::from_static(b"PK\x03\x04garbage"), Limits::default());
        assert!(r.is_err());
    }
}
