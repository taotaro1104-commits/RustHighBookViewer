use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;

use crate::entry::{EntryId, EntryMeta};
use crate::error::{ContainerError, Result};
use crate::extract::ExtractGuard;
use crate::folder::FolderContainer;
use crate::rar::RarProvider;
use crate::sevenz::SevenZProvider;
use crate::source::{FileSource, Source};
use crate::zip_container::ZipProvider;

pub trait Container: Send + Sync {
    /// 自然順ソート済み
    fn entries(&self) -> &[EntryMeta];
    /// 全体読み出し
    fn read(&self, id: EntryId) -> Result<Bytes>;
    /// 巨大エントリ用
    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>>;
    /// 先頭 `n` バイトだけ読む（画像ヘッダからの寸法取得用）。
    /// 文章を描いたページでできた本か（見開きより単ページで読むのが向く）。
    fn is_text(&self) -> bool {
        false
    }
    fn read_prefix(&self, id: EntryId, n: usize) -> Result<Bytes> {
        let mut buf = Vec::new();
        self.open_reader(id)?.take(n as u64).read_to_end(&mut buf)?;
        Ok(Bytes::from(buf))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Match,
    NoMatch,
}

pub trait ContainerProvider: Send + Sync {
    /// マジックバイトで判定する。
    fn probe(&self, src: &dyn Source) -> Probe;
    fn open(&self, src: Arc<dyn Source>) -> Result<Arc<dyn Container>>;
}

/// Zip bomb 等への安全策の上限値。
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// 1 エントリの展開後サイズ上限
    pub max_entry_size: u64,
    /// 全エントリの展開後サイズ合計の上限
    pub max_total_size: u64,
    /// 圧縮率の上限（展開後 / 圧縮後）。小さいエントリは対象外
    pub max_ratio: u64,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entry_size: 512 << 20,
            max_total_size: 16 << 30,
            max_ratio: 1000,
            max_entries: 100_000,
        }
    }
}

impl Limits {
    pub(crate) fn check_entry(&self, size: u64, compressed: u64) -> Result<()> {
        if size > self.max_entry_size {
            return Err(ContainerError::LimitExceeded("entry size"));
        }
        // 1MiB 未満の高圧縮率（白紙画像など）は正当なので許容する
        if size > (1 << 20) && size / compressed.max(1) > self.max_ratio {
            return Err(ContainerError::LimitExceeded("compression ratio"));
        }
        Ok(())
    }
}

/// パスから開く。ディレクトリならフォルダ、ファイルなら登録済みプロバイダで判定する。
pub fn open_path(path: &Path) -> Result<Arc<dyn Container>> {
    if path.is_dir() {
        return Ok(Arc::new(FolderContainer::open(path)?));
    }
    open_source(Arc::new(FileSource::open(path)?))
}

/// 7z / RAR を作業領域へ展開するときの展開後サイズ合計の上限
const EXTRACT_TOTAL_LIMIT: u64 = 4 << 30;

/// 任意の Source（Android の fd 等）から、登録済みプロバイダで判定して開く。
/// 7z / RAR の展開先は OS の一時ディレクトリ。Android など専用領域が要る環境は `open_source_in`。
pub fn open_source(src: Arc<dyn Source>) -> Result<Arc<dyn Container>> {
    open_source_in(src, &std::env::temp_dir())
}

/// `scratch` は 7z / RAR を展開する作業領域（コンテナを手放すと中身は消える）。
pub fn open_source_in(src: Arc<dyn Source>, scratch: &Path) -> Result<Arc<dyn Container>> {
    open_source_with(src, scratch, &ExtractGuard::default())
}

/// `guard` で 7z / RAR の展開を途中で止めたり、空き容量の下限を指定したりできる。
pub fn open_source_with(
    src: Arc<dyn Source>,
    scratch: &Path,
    guard: &ExtractGuard,
) -> Result<Arc<dyn Container>> {
    let zip = ZipProvider::default();
    // 7z / RAR は端末の保存領域へ展開するため、ZIP（読み出し時に展開）より厳しい合計上限にする
    let limits = Limits {
        max_total_size: EXTRACT_TOTAL_LIMIT,
        ..Limits::default()
    };
    let sevenz = SevenZProvider {
        scratch: scratch.to_path_buf(),
        limits,
        guard: guard.clone(),
    };
    let rar = RarProvider {
        scratch: scratch.to_path_buf(),
        limits,
        guard: guard.clone(),
    };
    let providers: [&dyn ContainerProvider; 3] = [&zip, &sevenz, &rar];
    for p in providers {
        if p.probe(src.as_ref()) == Probe::Match {
            return p.open(src);
        }
    }
    Err(ContainerError::Unsupported)
}
