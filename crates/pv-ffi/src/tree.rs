//! SAF のフォルダ（ツリー URI）を書庫として扱う。
//! 列挙とファイルを開く処理は Kotlin 側（ContentResolver）が提供し、Rust は fd を pread で読む。

use std::io::{self, Read};
use std::sync::Arc;

use bytes::Bytes;
use pv_container::{
    kind_from_path, natural_cmp, normalize_path, Container, ContainerError, EntryId, EntryKind,
    EntryMeta, Limits, Result, Source, SourceReader,
};

use crate::fd::source_from_fd;
use crate::types::PvError;

#[derive(Debug, Clone, uniffi::Record)]
pub struct PvTreeEntry {
    /// ツリー根からの相対パス（区切りは `/`）
    pub path: String,
    pub size: u64,
}

/// Kotlin が実装する。`index` は `entries()` が返した並び順の添字。
#[uniffi::export(with_foreign)]
pub trait PvTree: Send + Sync {
    fn entries(&self) -> std::result::Result<Vec<PvTreeEntry>, PvError>;
    /// 読み取り専用 fd を返す。所有権は Rust に移る（Kotlin は detachFd() で渡す）。
    fn open_fd(&self, index: u32) -> std::result::Result<i32, PvError>;
}

type Opener = Box<dyn Fn(u32) -> io::Result<Arc<dyn Source>> + Send + Sync>;

pub struct TreeContainer {
    entries: Vec<EntryMeta>,
    /// EntryId -> 列挙時の添字
    handles: Vec<u32>,
    opener: Opener,
    limits: Limits,
}

impl TreeContainer {
    pub fn from_tree(tree: Arc<dyn PvTree>) -> Result<Self> {
        let listing = tree
            .entries()
            .map_err(|e| ContainerError::Archive(e.to_string()))?;
        Self::new(
            listing,
            Box::new(move |i| {
                let fd = tree.open_fd(i).map_err(io::Error::other)?;
                source_from_fd(fd)
            }),
        )
    }

    fn new(listing: Vec<PvTreeEntry>, opener: Opener) -> Result<Self> {
        let limits = Limits::default();
        if listing.len() > limits.max_entries * 4 {
            return Err(ContainerError::LimitExceeded("entry count"));
        }
        let mut found: Vec<(String, u64, u32)> = listing
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let path = normalize_path(&e.path)?;
                (kind_from_path(&path) == EntryKind::Image).then_some((path, e.size, i as u32))
            })
            .collect();
        if found.len() > limits.max_entries {
            return Err(ContainerError::LimitExceeded("entry count"));
        }
        found.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let (mut entries, mut handles) = (Vec::new(), Vec::new());
        for (id, (path, size, handle)) in found.into_iter().enumerate() {
            entries.push(EntryMeta {
                id: EntryId(id as u32),
                kind: EntryKind::Image,
                path,
                size,
            });
            handles.push(handle);
        }
        Ok(Self {
            entries,
            handles,
            opener,
            limits,
        })
    }

    /// 本の識別子用の「パス:サイズ」一覧
    pub fn listing(&self) -> Vec<(String, u64)> {
        self.entries
            .iter()
            .map(|e| (e.path.clone(), e.size))
            .collect()
    }

    fn source(&self, id: EntryId) -> Result<Arc<dyn Source>> {
        let handle = *self
            .handles
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))?;
        Ok((self.opener)(handle)?)
    }
}

impl Container for TreeContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        let mut buf = Vec::new();
        self.open_reader(id)?.read_to_end(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        let src = self.source(id)?;
        Ok(Box::new(
            SourceReader::new(src).take(self.limits.max_entry_size),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pv_container::FileSource;
    use std::collections::HashMap;

    fn container(files: &[(&str, &[u8])]) -> (tempfile::TempDir, TreeContainer) {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = HashMap::new();
        let mut listing = Vec::new();
        for (i, (name, data)) in files.iter().enumerate() {
            let p = dir.path().join(format!("f{i}"));
            std::fs::write(&p, data).unwrap();
            paths.insert(i as u32, p);
            listing.push(PvTreeEntry {
                path: (*name).into(),
                size: data.len() as u64,
            });
        }
        let c = TreeContainer::new(
            listing,
            Box::new(move |i| Ok(Arc::new(FileSource::open(&paths[&i])?) as Arc<dyn Source>)),
        )
        .unwrap();
        (dir, c)
    }

    #[test]
    fn keeps_images_in_natural_order_and_maps_handles() {
        let (_d, c) = container(&[
            ("p10.png", b"ten"),
            ("readme.txt", b"no"),
            ("p2.png", b"two"),
            ("../evil.png", b"evil"),
        ]);
        let names: Vec<_> = c.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["evil.png", "p2.png", "p10.png"]);
        assert_eq!(&c.read(EntryId(1)).unwrap()[..], b"two");
        assert_eq!(&c.read(EntryId(2)).unwrap()[..], b"ten");
        assert_eq!(&c.read_prefix(EntryId(2), 2).unwrap()[..], b"te");
    }

    #[test]
    fn listing_is_stable_for_identification() {
        let (_d, a) = container(&[("a.png", b"1"), ("b.png", b"22")]);
        let (_e, b) = container(&[("b.png", b"22"), ("a.png", b"1")]);
        assert_eq!(a.listing(), b.listing());
    }

    #[test]
    fn opener_failure_is_an_error() {
        let c = TreeContainer::new(
            vec![PvTreeEntry {
                path: "a.png".into(),
                size: 1,
            }],
            Box::new(|_| Err(io::Error::new(io::ErrorKind::NotFound, "gone"))),
        )
        .unwrap();
        assert!(c.read(EntryId(0)).is_err());
        assert!(c.read(EntryId(7)).is_err());
    }
}
