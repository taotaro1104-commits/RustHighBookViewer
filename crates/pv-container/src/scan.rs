use std::fs;
use std::path::{Path, PathBuf};

use crate::entry::{kind_from_path, natural_cmp, EntryKind};
use crate::error::{ContainerError, Result};

/// 入れ子の深さの上限（循環や異常に深い構造への備え）
const MAX_DEPTH: usize = 32;

/// フォルダを探して見つけた「1 冊」の単位
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanKind {
    /// フォルダ直下の画像をまとめて 1 冊（サブフォルダの画像は含めない）
    Images,
    /// 書庫ファイル（EPUB・ZIP/CBZ・7z・RAR）1 つで 1 冊
    Archive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanItem {
    pub path: PathBuf,
    pub kind: ScanKind,
}

/// `root` 以下を探し、本の単位に分ける。
/// 各フォルダで、直下の画像をまとめて 1 冊、書庫ファイルは 1 つにつき 1 冊とし、サブフォルダも同様に辿る。
/// 並びはフォルダごとに「画像の本 → 書庫（自然順）→ サブフォルダ（自然順）」。
/// 読めないフォルダは飛ばす。ただし `root` が読めないときはエラー。
pub fn scan_books(root: &Path, max_items: usize) -> Result<Vec<ScanItem>> {
    fs::read_dir(root)?;
    let mut out = Vec::new();
    scan_dir(root, 0, max_items, &mut out)?;
    Ok(out)
}

fn scan_dir(dir: &Path, depth: usize, max_items: usize, out: &mut Vec<ScanItem>) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let Ok(read) = fs::read_dir(dir) else {
        return Ok(());
    };
    let mut archives: Vec<PathBuf> = Vec::new();
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut has_images = false;
    for item in read {
        let Ok(item) = item else { continue };
        let Ok(ft) = item.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        if ft.is_dir() {
            if !name.starts_with('.') {
                subdirs.push(item.path());
            }
        } else if ft.is_file() {
            match kind_from_path(&name) {
                EntryKind::Image => has_images = true,
                EntryKind::Archive => archives.push(item.path()),
                _ => {}
            }
        }
    }
    let by_name = |a: &PathBuf, b: &PathBuf| {
        natural_cmp(
            &a.file_name().unwrap_or_default().to_string_lossy(),
            &b.file_name().unwrap_or_default().to_string_lossy(),
        )
    };
    archives.sort_by(by_name);
    subdirs.sort_by(by_name);

    let mut push = |path: PathBuf, kind: ScanKind| -> Result<()> {
        if out.len() >= max_items {
            return Err(ContainerError::LimitExceeded("entry count"));
        }
        out.push(ScanItem { path, kind });
        Ok(())
    };
    if has_images {
        push(dir.to_path_buf(), ScanKind::Images)?;
    }
    for a in archives {
        push(a, ScanKind::Archive)?;
    }
    for d in subdirs {
        scan_dir(&d, depth + 1, max_items, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn names(items: &[ScanItem], root: &Path) -> Vec<String> {
        items
            .iter()
            .map(|i| {
                let rel = i
                    .path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                format!("{:?}:{rel}", i.kind)
            })
            .collect()
    }

    #[test]
    fn images_and_epubs_become_separate_books_recursively() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        touch(&r.join("1.png"));
        touch(&r.join("2.png"));
        touch(&r.join("b.epub"));
        touch(&r.join("a.epub"));
        touch(&r.join("memo.txt"));
        touch(&r.join("vol10/1.jpg"));
        touch(&r.join("vol2/1.jpg"));
        touch(&r.join("vol2/inner/x.cbz"));
        touch(&r.join("vol2/inner/deeper/p.png"));
        touch(&r.join("empty/readme.txt"));
        touch(&r.join(".hidden/h.png"));
        let items = scan_books(r, 100).unwrap();
        assert_eq!(
            names(&items, r),
            [
                "Images:",
                "Archive:a.epub",
                "Archive:b.epub",
                "Images:vol2",
                "Archive:vol2/inner/x.cbz",
                "Images:vol2/inner/deeper",
                "Images:vol10",
            ]
        );
    }

    #[test]
    fn limit_and_missing_root() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..3 {
            touch(&d.path().join(format!("{i}.epub")));
        }
        assert!(matches!(
            scan_books(d.path(), 2),
            Err(ContainerError::LimitExceeded("entry count"))
        ));
        assert!(scan_books(&d.path().join("none"), 10).is_err());
    }
}
