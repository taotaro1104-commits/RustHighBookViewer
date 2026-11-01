//! 書庫の識別。パス＋サイズ＋先頭64KBのハッシュで、移動・リネーム後も同じ本とみなす。

use std::fs;
use std::io::{self, Read};
use std::path::Path;

const HEAD_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookKey {
    pub size: u64,
    pub head_hash: String,
}

impl BookKey {
    /// 先頭 64KB（以上渡しても先頭64KBだけ使う）とサイズから作る。
    pub fn from_head(size: u64, head: &[u8]) -> Self {
        let head = &head[..head.len().min(HEAD_BYTES as usize)];
        Self {
            size,
            head_hash: blake3::hash(head).to_hex().to_string(),
        }
    }
}

impl BookKey {
    /// 「相対パス:サイズ」の一覧から作る（フォルダや SAF ツリーなど、単一ファイルでない本）。
    pub fn from_listing(mut items: Vec<(String, u64)>) -> Self {
        items.sort();
        let mut h = blake3::Hasher::new();
        for (name, s) in &items {
            h.update(name.as_bytes());
            h.update(&s.to_le_bytes());
        }
        Self {
            size: items.iter().map(|(_, s)| s).sum(),
            head_hash: h.finalize().to_hex().to_string(),
        }
    }
}

pub const HEAD_LEN: usize = HEAD_BYTES as usize;

pub fn fingerprint(path: &Path) -> io::Result<BookKey> {
    if path.is_dir() {
        return folder_key(path);
    }
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut head = Vec::new();
    file.take(HEAD_BYTES).read_to_end(&mut head)?;
    Ok(BookKey::from_head(size, &head))
}

/// フォルダは「相対パス:サイズ」の一覧のハッシュで識別する（シンボリックリンクは辿らない）。
fn folder_key(root: &Path) -> io::Result<BookKey> {
    let mut items = Vec::new();
    collect(root, root, &mut items)?;
    Ok(BookKey::from_listing(items))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> io::Result<()> {
    for e in fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            collect(root, &e.path(), out)?;
        } else if ft.is_file() {
            let rel = e
                .path()
                .strip_prefix(root)
                .unwrap_or(&e.path())
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, e.metadata()?.len()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_content_same_key_even_if_renamed() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.cbz"), b"hello archive").unwrap();
        let k1 = fingerprint(&d.path().join("a.cbz")).unwrap();
        fs::rename(d.path().join("a.cbz"), d.path().join("renamed.cbz")).unwrap();
        assert_eq!(fingerprint(&d.path().join("renamed.cbz")).unwrap(), k1);
        assert_eq!(k1.size, 13);
    }

    #[test]
    fn different_head_differs() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a"), b"aaaa").unwrap();
        fs::write(d.path().join("b"), b"bbbb").unwrap();
        assert_ne!(
            fingerprint(&d.path().join("a")).unwrap(),
            fingerprint(&d.path().join("b")).unwrap()
        );
    }

    #[test]
    fn only_the_head_is_hashed() {
        let d = tempfile::tempdir().unwrap();
        let mut a = vec![1u8; 70_000];
        let mut b = a.clone();
        a[69_999] = 7;
        b[69_999] = 8; // 先頭64KBの外だけが違う
        fs::write(d.path().join("a"), &a).unwrap();
        fs::write(d.path().join("b"), &b).unwrap();
        assert_eq!(
            fingerprint(&d.path().join("a")).unwrap(),
            fingerprint(&d.path().join("b")).unwrap()
        );
    }

    #[test]
    fn folder_key_tracks_listing_and_ignores_location() {
        let d = tempfile::tempdir().unwrap();
        for dir in ["x", "y"] {
            fs::create_dir_all(d.path().join(dir)).unwrap();
            fs::write(d.path().join(dir).join("1.png"), b"abc").unwrap();
        }
        assert_eq!(
            fingerprint(&d.path().join("x")).unwrap(),
            fingerprint(&d.path().join("y")).unwrap()
        );
        fs::write(d.path().join("y/2.png"), b"z").unwrap();
        assert_ne!(
            fingerprint(&d.path().join("x")).unwrap(),
            fingerprint(&d.path().join("y")).unwrap()
        );
    }

    #[test]
    fn missing_path_is_error() {
        assert!(fingerprint(Path::new("/definitely/not/here.cbz")).is_err());
    }
}
