//! 7z / RAR のような「先頭から順にしか読めない」書庫の共通基盤。
//! 画像エントリだけを作業ディレクトリへ一度展開し、以後はファイルとして読む。
//! 展開先はコンテナを手放した時点で削除される。

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use bytes::Bytes;

use crate::container::{Container, Limits};
use crate::entry::{kind_from_path, natural_cmp, EntryId, EntryKind, EntryMeta};
use crate::error::{ContainerError, Result};
use crate::path::normalize_path;
use crate::source::Source;

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
    found: Vec<(String, PathBuf, u64)>,
    total: u64,
}

impl Extractor {
    /// `raw_name` は書庫内パス。画像以外・不正なパスは黙って読み飛ばす。
    pub fn add(&mut self, raw_name: &str, reader: &mut dyn Read) -> Result<()> {
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
        let written = io::copy(&mut reader.take(cap + 1), &mut file)?;
        if written > cap {
            return Err(ContainerError::LimitExceeded("entry size"));
        }
        self.total += written;
        if self.total > self.limits.max_total_size {
            return Err(ContainerError::LimitExceeded("total size"));
        }
        self.found.push((name, path, written));
        Ok(())
    }
}

impl ExtractedContainer {
    /// `scratch` 配下に作業ディレクトリを作り、`fill` で展開する。
    pub fn build(
        scratch: &Path,
        limits: Limits,
        fill: impl FnOnce(&mut Extractor) -> Result<()>,
    ) -> Result<Self> {
        fs::create_dir_all(scratch)?;
        let dir = tempfile::Builder::new()
            .prefix("pv-x-")
            .tempdir_in(scratch)?;
        let mut ex = Extractor {
            dir: dir.path().to_path_buf(),
            limits,
            found: Vec::new(),
            total: 0,
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
pub(crate) fn spill_to_file(src: &dyn Source, dir: &Path, name: &str) -> Result<PathBuf> {
    let path = dir.join(name);
    let mut out = fs::File::create(&path)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut off = 0u64;
    loop {
        let n = src.read_at(off, &mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        off += n as u64;
    }
    Ok(path)
}
