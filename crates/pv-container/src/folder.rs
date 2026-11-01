use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use bytes::Bytes;

use crate::container::{Container, Limits};
use crate::entry::{kind_from_path, natural_cmp, EntryId, EntryMeta};
use crate::error::{ContainerError, Result};
use crate::path::normalize_path;

/// ディレクトリを書庫として扱う。シンボリックリンクは辿らない。
pub struct FolderContainer {
    entries: Vec<EntryMeta>,
    paths: Vec<PathBuf>,
    limits: Limits,
}

impl FolderContainer {
    pub fn open(root: &Path) -> Result<Self> {
        let limits = Limits::default();
        let mut found: Vec<(String, PathBuf, u64)> = Vec::new();
        walk(root, root, &mut found, limits.max_entries)?;
        found.sort_by(|a, b| natural_cmp(&a.0, &b.0));

        let mut entries = Vec::with_capacity(found.len());
        let mut paths = Vec::with_capacity(found.len());
        for (i, (rel, abs, size)) in found.into_iter().enumerate() {
            entries.push(EntryMeta {
                id: EntryId(i as u32),
                kind: kind_from_path(&rel),
                path: rel,
                size,
            });
            paths.push(abs);
        }
        Ok(Self {
            entries,
            paths,
            limits,
        })
    }

    fn path_of(&self, id: EntryId) -> Result<&PathBuf> {
        self.paths
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf, u64)>,
    max_entries: usize,
) -> Result<()> {
    for item in fs::read_dir(dir)? {
        let item = item?;
        let ft = item.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let abs = item.path();
        if ft.is_dir() {
            walk(root, &abs, out, max_entries)?;
        } else if ft.is_file() {
            if out.len() >= max_entries {
                return Err(ContainerError::LimitExceeded("entry count"));
            }
            let rel = abs
                .strip_prefix(root)
                .unwrap_or(&abs)
                .to_string_lossy()
                .into_owned();
            if let Some(rel) = normalize_path(&rel) {
                out.push((rel, abs, item.metadata()?.len()));
            }
        }
    }
    Ok(())
}

impl Container for FolderContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        let mut buf = Vec::new();
        self.open_reader(id)?.read_to_end(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        let meta = self
            .entries
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))?;
        if meta.size > self.limits.max_entry_size {
            return Err(ContainerError::LimitExceeded("entry size"));
        }
        let file = fs::File::open(self.path_of(id)?)?;
        Ok(Box::new(file.take(self.limits.max_entry_size)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("p10.png"), b"ten").unwrap();
        fs::write(dir.path().join("p2.png"), b"two").unwrap();
        fs::write(dir.path().join("sub/a.jpg"), b"a").unwrap();
        dir
    }

    #[test]
    fn lists_files_in_natural_order() {
        let dir = make_tree();
        let c = FolderContainer::open(dir.path()).unwrap();
        let names: Vec<_> = c.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["p2.png", "p10.png", "sub/a.jpg"]);
        assert_eq!(c.entries()[0].id, EntryId(0));
    }

    #[test]
    fn reads_entry_bytes() {
        let dir = make_tree();
        let c = FolderContainer::open(dir.path()).unwrap();
        assert_eq!(&c.read(EntryId(1)).unwrap()[..], b"ten");
    }

    #[test]
    fn unknown_id_is_error_not_panic() {
        let dir = make_tree();
        let c = FolderContainer::open(dir.path()).unwrap();
        assert!(c.open_reader(EntryId(99)).is_err());
    }
}
