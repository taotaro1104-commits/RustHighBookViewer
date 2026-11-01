use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use bytes::Bytes;

use crate::container::{Container, Limits};
use crate::entry::{kind_from_path, natural_cmp, EntryId, EntryKind, EntryMeta};
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
        Self::open_with(root, Limits::default())
    }

    /// `limits.max_entries` は画像の枚数の上限（画像以外のファイルは数えない）。
    pub fn open_with(root: &Path, limits: Limits) -> Result<Self> {
        Self::open_inner(root, limits, true)
    }

    /// 直下のファイルだけを対象にする（サブフォルダは別の本として扱う）。
    pub fn open_flat(root: &Path) -> Result<Self> {
        Self::open_inner(root, Limits::default(), false)
    }

    fn open_inner(root: &Path, limits: Limits, recurse: bool) -> Result<Self> {
        let mut found: Vec<(String, PathBuf, u64)> = Vec::new();
        // ルート自体が読めないときだけエラーにする（途中の読めないフォルダ・ファイルは飛ばす）
        fs::read_dir(root)?;
        let mut walker = Walker {
            root,
            recurse,
            max_images: limits.max_entries,
            // 画像以外も保持するが、メモリを使い切らないよう上限を設ける
            max_total: limits.max_entries.saturating_mul(4),
            images: 0,
            out: &mut found,
        };
        walker.walk(root, 0)?;
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

/// 入れ子の深さの上限（循環や異常に深い構造への備え）
const MAX_DEPTH: usize = 32;

struct Walker<'a> {
    root: &'a Path,
    recurse: bool,
    max_images: usize,
    max_total: usize,
    images: usize,
    out: &'a mut Vec<(String, PathBuf, u64)>,
}

impl Walker<'_> {
    fn walk(&mut self, dir: &Path, depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Ok(());
        }
        // 読めないフォルダ（権限なし・消えた等）は、中身を飛ばして続ける
        let Ok(read) = fs::read_dir(dir) else {
            return Ok(());
        };
        for item in read {
            let Ok(item) = item else { continue };
            let Ok(ft) = item.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let abs = item.path();
            if ft.is_dir() {
                if !self.recurse {
                    continue;
                }
                self.walk(&abs, depth + 1)?;
            } else if ft.is_file() {
                let rel = abs
                    .strip_prefix(self.root)
                    .unwrap_or(&abs)
                    .to_string_lossy()
                    .into_owned();
                let Some(rel) = normalize_path(&rel) else {
                    continue;
                };
                let Ok(meta) = item.metadata() else { continue };
                if kind_from_path(&rel) == EntryKind::Image {
                    if self.images >= self.max_images {
                        return Err(ContainerError::LimitExceeded("entry count"));
                    }
                    self.images += 1;
                }
                if self.out.len() >= self.max_total {
                    return Err(ContainerError::LimitExceeded("entry count"));
                }
                self.out.push((rel, abs, meta.len()));
            }
        }
        Ok(())
    }
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
    fn flat_mode_ignores_subfolders() {
        let dir = make_tree();
        let c = FolderContainer::open_flat(dir.path()).unwrap();
        let names: Vec<_> = c.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["p2.png", "p10.png"]);
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

    fn limits(max_entries: usize) -> Limits {
        Limits {
            max_entries,
            ..Limits::default()
        }
    }

    #[test]
    fn non_image_files_do_not_count_toward_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..2 {
            fs::write(dir.path().join(format!("{i}.png")), b"i").unwrap();
        }
        for i in 0..5 {
            fs::write(dir.path().join(format!("note{i}.txt")), b"t").unwrap();
        }
        assert!(FolderContainer::open_with(dir.path(), limits(2)).is_ok());
        fs::write(dir.path().join("2.png"), b"i").unwrap();
        assert!(matches!(
            FolderContainer::open_with(dir.path(), limits(2)),
            Err(ContainerError::LimitExceeded("entry count"))
        ));
    }

    #[test]
    fn missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(FolderContainer::open(&dir.path().join("none")).is_err());
    }

    #[test]
    fn very_deep_nesting_is_cut_off_not_a_stack_overflow() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = dir.path().to_path_buf();
        for _ in 0..(MAX_DEPTH + 10) {
            p.push("d");
        }
        fs::create_dir_all(&p).unwrap();
        fs::write(dir.path().join("top.png"), b"i").unwrap();
        fs::write(p.join("deep.png"), b"i").unwrap();
        let c = FolderContainer::open(dir.path()).unwrap();
        let names: Vec<_> = c.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["top.png"]);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_subfolder_is_skipped_instead_of_failing_the_whole_book() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("ok")).unwrap();
        fs::create_dir_all(dir.path().join("locked")).unwrap();
        fs::write(dir.path().join("ok/a.png"), b"a").unwrap();
        fs::write(dir.path().join("locked/b.png"), b"b").unwrap();
        let locked = dir.path().join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let readable_anyway = fs::read_dir(&locked).is_ok(); // root では塞げない
        let c = FolderContainer::open(dir.path());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        let c = c.unwrap();
        if !readable_anyway {
            assert_eq!(c.entries().len(), 1);
        }
    }
}
