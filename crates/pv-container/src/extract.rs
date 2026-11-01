//! 7z / RAR のような「先頭から順にしか読めない」書庫の共通基盤。
//! 画像エントリだけを作業ディレクトリへ一度展開し、以後はファイルとして読む。
//! 展開先はコンテナを手放した時点で削除される。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;

use crate::container::{Container, Limits};
use crate::entry::{kind_from_path, natural_cmp, EntryId, EntryKind, EntryMeta};
use crate::error::{ContainerError, Result};
use crate::path::normalize_path;
use crate::source::Source;

/// 展開中に空き容量を確かめる間隔（書き込んだバイト数）
const DISK_CHECK_INTERVAL: u64 = 64 << 20;
/// 書き込み・キャンセル確認の単位
const CHUNK: usize = 1 << 20;
/// RAR のようにファイルパスしか受け付けない書庫を、作業領域へ書き出すときの元ファイルの上限
pub(crate) const MAX_SPILL: u64 = 2 << 30;

/// 展開の制御。UI からのキャンセルと、端末の空き容量を使い切らないための下限。
#[derive(Debug, Clone)]
pub struct ExtractGuard {
    /// true になると、次のエントリ（または次のチャンク）で `Cancelled` を返して止まる
    pub cancel: Arc<AtomicBool>,
    /// 作業領域に最低限残す空き容量（バイト）
    pub min_free: u64,
}

impl Default for ExtractGuard {
    fn default() -> Self {
        Self {
            cancel: Arc::default(),
            min_free: 256 << 20,
        }
    }
}

impl ExtractGuard {
    fn check_cancel(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(ContainerError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// `dir` のあるファイルシステムに、`need` を書いたあとも `min_free` 残るか。
    /// 空き容量が取れない環境では、確認できないものとして通す（ENOSPC は書き込み時にエラーになる）。
    pub(crate) fn check_space(&self, dir: &Path, need: u64) -> Result<()> {
        match fs4::available_space(dir) {
            Ok(free) if free < need.saturating_add(self.min_free) => {
                Err(ContainerError::LimitExceeded("disk space"))
            }
            _ => Ok(()),
        }
    }
}

pub struct ExtractedContainer {
    entries: Vec<EntryMeta>,
    files: Vec<PathBuf>,
    limits: Limits,
    /// Drop で展開物ごと消える
    _dir: tempfile::TempDir,
}

/// 展開中に画像エントリを受け取る。上限超過はここでエラーにする。
pub struct Extractor {
    dir: PathBuf,
    limits: Limits,
    guard: ExtractGuard,
    found: Vec<(String, PathBuf, u64)>,
    total: u64,
    since_space_check: u64,
}

impl Extractor {
    /// `raw_name` は書庫内パス。画像以外・不正なパスは黙って読み飛ばす。
    pub fn add(&mut self, raw_name: &str, reader: &mut dyn Read) -> Result<()> {
        // 画像でないエントリが延々と続く solid 書庫でも、エントリごとに止められるようにする
        self.guard.check_cancel()?;
        let Some(name) = normalize_path(raw_name) else {
            return Ok(());
        };
        if kind_from_path(&name) != EntryKind::Image {
            return Ok(());
        }
        if self.found.len() >= self.limits.max_entries {
            return Err(ContainerError::LimitExceeded("entry count"));
        }
        let path = self.dir.join(format!("{}.bin", self.found.len()));
        let mut file = fs::File::create(&path)?;
        let cap = self.limits.max_entry_size;
        let written = self.copy_limited(reader, &mut file, cap)?;
        self.total += written;
        if self.total > self.limits.max_total_size {
            return Err(ContainerError::LimitExceeded("total size"));
        }
        self.found.push((name, path, written));
        Ok(())
    }

    /// `cap` バイトまで書く。超えたらエラー。チャンクごとにキャンセルと空き容量を確認する。
    fn copy_limited(&mut self, reader: &mut dyn Read, out: &mut fs::File, cap: u64) -> Result<u64> {
        let mut buf = vec![0u8; CHUNK];
        let mut written = 0u64;
        loop {
            self.guard.check_cancel()?;
            let n = reader.read(&mut buf)?;
            if n == 0 {
                return Ok(written);
            }
            written += n as u64;
            if written > cap {
                return Err(ContainerError::LimitExceeded("entry size"));
            }
            self.since_space_check += n as u64;
            if self.since_space_check >= DISK_CHECK_INTERVAL || written == n as u64 {
                self.guard.check_space(&self.dir, n as u64)?;
                self.since_space_check = 0;
            }
            out.write_all(&buf[..n])?;
        }
    }
}

impl ExtractedContainer {
    /// `scratch` 配下に作業ディレクトリを作り、`fill` で展開する。
    pub fn build(
        scratch: &Path,
        limits: Limits,
        guard: &ExtractGuard,
        fill: impl FnOnce(&mut Extractor) -> Result<()>,
    ) -> Result<Self> {
        guard.check_cancel()?;
        fs::create_dir_all(scratch)?;
        let dir = tempfile::Builder::new()
            .prefix("pv-x-")
            .tempdir_in(scratch)?;
        let mut ex = Extractor {
            dir: dir.path().to_path_buf(),
            limits,
            guard: guard.clone(),
            found: Vec::new(),
            total: 0,
            since_space_check: 0,
        };
        fill(&mut ex)?;
        let mut found = ex.found;
        found.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        let (mut entries, mut files) = (Vec::new(), Vec::new());
        for (i, (path, file, size)) in found.into_iter().enumerate() {
            entries.push(EntryMeta {
                id: EntryId(i as u32),
                kind: kind_from_path(&path),
                path,
                size,
            });
            files.push(file);
        }
        Ok(Self {
            entries,
            files,
            limits,
            _dir: dir,
        })
    }
}

impl Container for ExtractedContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        let mut buf = Vec::new();
        self.open_reader(id)?.read_to_end(&mut buf)?;
        Ok(Bytes::from(buf))
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        let path = self
            .files
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))?;
        Ok(Box::new(
            fs::File::open(path)?.take(self.limits.max_entry_size),
        ))
    }
}

/// Source の内容を一時ファイルへ書き出す（パスしか受け付けないライブラリ用）。
/// 元ファイルが `MAX_SPILL` を超える、または空き容量が足りない場合はエラー。
pub(crate) fn spill_to_file(
    src: &dyn Source,
    dir: &Path,
    name: &str,
    guard: &ExtractGuard,
) -> Result<PathBuf> {
    if src.len() > MAX_SPILL {
        return Err(ContainerError::LimitExceeded("archive size"));
    }
    guard.check_space(dir, src.len())?;
    let path = dir.join(name);
    let mut out = fs::File::create(&path)?;
    let mut buf = vec![0u8; CHUNK];
    let mut off = 0u64;
    loop {
        guard.check_cancel()?;
        let n = src.read_at(off, &mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        off += n as u64;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(entry: u64, total: u64, count: usize) -> Limits {
        Limits {
            max_entry_size: entry,
            max_total_size: total,
            max_ratio: 1000,
            max_entries: count,
        }
    }

    fn add(ex: &mut Extractor, name: &str, data: &[u8]) -> Result<()> {
        ex.add(name, &mut &data[..])
    }

    fn build(
        scratch: &Path,
        l: Limits,
        g: &ExtractGuard,
        fill: impl FnOnce(&mut Extractor) -> Result<()>,
    ) -> Result<ExtractedContainer> {
        ExtractedContainer::build(scratch, l, g, fill)
    }

    #[test]
    fn keeps_only_safe_images_in_natural_order() {
        let scratch = tempfile::tempdir().unwrap();
        let c = build(
            scratch.path(),
            limits(100, 1000, 10),
            &ExtractGuard::default(),
            |ex| {
                add(ex, "p10.png", b"ten")?;
                add(ex, "p2.png", b"two")?;
                add(ex, "notes.txt", b"skip")?;
                add(ex, "../../evil.png", b"evil")?;
                add(ex, "", b"empty name")
            },
        )
        .unwrap();
        let names: Vec<_> = c.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names, ["evil.png", "p2.png", "p10.png"]);
        assert_eq!(&c.read(EntryId(1)).unwrap()[..], b"two");
        // 展開先は作業ディレクトリの中だけ（../ で外へ出ない）
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 1);
    }

    #[test]
    fn oversized_entry_is_rejected_and_cleaned_up() {
        let scratch = tempfile::tempdir().unwrap();
        let r = build(
            scratch.path(),
            limits(4, 1000, 10),
            &ExtractGuard::default(),
            |ex| {
                add(ex, "a.png", b"1234")?;
                add(ex, "b.png", b"12345")
            },
        );
        assert!(matches!(
            r,
            Err(ContainerError::LimitExceeded("entry size"))
        ));
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[test]
    fn total_and_count_limits_are_enforced() {
        let scratch = tempfile::tempdir().unwrap();
        let g = ExtractGuard::default();
        let r = build(scratch.path(), limits(10, 7, 10), &g, |ex| {
            add(ex, "a.png", b"1234")?;
            add(ex, "b.png", b"1234")
        });
        assert!(matches!(
            r,
            Err(ContainerError::LimitExceeded("total size"))
        ));
        let r = build(scratch.path(), limits(10, 100, 1), &g, |ex| {
            add(ex, "a.png", b"1")?;
            add(ex, "b.png", b"2")
        });
        assert!(matches!(
            r,
            Err(ContainerError::LimitExceeded("entry count"))
        ));
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancel_stops_even_on_non_image_entries_and_cleans_up() {
        let scratch = tempfile::tempdir().unwrap();
        let g = ExtractGuard::default();
        let cancel = Arc::clone(&g.cancel);
        let r = build(scratch.path(), limits(100, 1000, 10), &g, |ex| {
            add(ex, "a.png", b"1")?;
            cancel.store(true, Ordering::Relaxed);
            add(ex, "readme.txt", b"not an image") // 画像でなくても確認する
        });
        assert!(matches!(r, Err(ContainerError::Cancelled)));
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
        // 最初から立っていれば、作業ディレクトリも作らない
        let r = build(scratch.path(), limits(100, 1000, 10), &g, |_| Ok(()));
        assert!(matches!(r, Err(ContainerError::Cancelled)));
    }

    #[test]
    fn insufficient_free_space_is_an_error_not_a_full_disk() {
        let scratch = tempfile::tempdir().unwrap();
        let g = ExtractGuard {
            min_free: u64::MAX,
            ..Default::default()
        };
        let r = build(scratch.path(), limits(100, 1000, 10), &g, |ex| {
            add(ex, "a.png", b"1")
        });
        assert!(matches!(
            r,
            Err(ContainerError::LimitExceeded("disk space"))
        ));
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }

    #[test]
    fn spill_respects_cancel_and_free_space() {
        let dir = tempfile::tempdir().unwrap();
        let src = crate::source::MemorySource(Bytes::from_static(b"0123456789"));
        let ok = spill_to_file(&src, dir.path(), "a.rar", &ExtractGuard::default()).unwrap();
        assert_eq!(fs::read(ok).unwrap(), b"0123456789");
        let g = ExtractGuard {
            min_free: u64::MAX,
            ..Default::default()
        };
        assert!(matches!(
            spill_to_file(&src, dir.path(), "b.rar", &g),
            Err(ContainerError::LimitExceeded("disk space"))
        ));
        let g = ExtractGuard::default();
        g.cancel.store(true, Ordering::Relaxed);
        assert!(matches!(
            spill_to_file(&src, dir.path(), "c.rar", &g),
            Err(ContainerError::Cancelled)
        ));
    }
}
