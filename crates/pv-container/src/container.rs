use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;

use crate::entry::{EntryId, EntryKind, EntryMeta};
use crate::error::{ContainerError, Result};
use crate::extract::ExtractGuard;
use crate::folder::FolderContainer;
use crate::pdf::PdfProvider;
use crate::rar::RarProvider;
use crate::sevenz::SevenZProvider;
use crate::source::{FileSource, MemorySource, Source};
use crate::text_style::TextStyle;
use crate::txt::TxtProvider;
use crate::zip_container::ZipProvider;

/// 目次の 1 項目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub title: String,
    /// 移動先のページ（`entries()` の添字）
    pub page: usize,
    /// 階層の深さ（0 が最上位）
    pub level: u8,
}

pub trait Container: Send + Sync {
    /// 自然順ソート済み
    fn entries(&self) -> &[EntryMeta];
    /// 全体読み出し
    fn read(&self, id: EntryId) -> Result<Bytes>;
    /// 巨大エントリ用
    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>>;
    /// 先頭 `n` バイトだけ読む（画像ヘッダからの寸法取得用）。
    /// 目次（EPUB の目次）。無ければ空。
    fn toc(&self) -> Vec<TocEntry> {
        Vec::new()
    }
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
    open_path_styled(path, TextStyle::default())
}

/// `open_path` に加えて、文章の EPUB の見た目（文字の大きさ・書体）を指定する。
pub fn open_path_styled(path: &Path, text_style: TextStyle) -> Result<Arc<dyn Container>> {
    if path.is_dir() {
        return Ok(Arc::new(FolderContainer::open(path)?));
    }
    open_source_styled(
        Arc::new(FileSource::open(path)?),
        &std::env::temp_dir(),
        &ExtractGuard::default(),
        text_style,
    )
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
    open_source_styled(src, scratch, guard, TextStyle::default())
}

/// `open_source_with` に加えて、文章の EPUB の見た目（文字の大きさ・書体）を指定する。
pub fn open_source_styled(
    src: Arc<dyn Source>,
    scratch: &Path,
    guard: &ExtractGuard,
    text_style: TextStyle,
) -> Result<Arc<dyn Container>> {
    open_source_depth(src, scratch, guard, text_style, 0)
}

/// 入れ子の書庫をたどる深さの上限
const MAX_NEST: usize = 3;

/// 画像が 1 枚も無く書庫（EPUB・CBZ など）だけを含む書庫は、最初の書庫を本として開く。
fn open_source_depth(
    src: Arc<dyn Source>,
    scratch: &Path,
    guard: &ExtractGuard,
    text_style: TextStyle,
    depth: usize,
) -> Result<Arc<dyn Container>> {
    let outer = open_flat(src, scratch, guard, text_style.clone())?;
    if depth >= MAX_NEST || outer.entries().iter().any(|e| e.kind == EntryKind::Image) {
        return Ok(outer);
    }
    let Some(inner) = outer
        .entries()
        .iter()
        .find(|e| e.kind == EntryKind::Archive)
    else {
        return Ok(outer);
    };
    let bytes = outer.read(inner.id)?;
    let nested = open_source_depth(
        Arc::new(MemorySource(bytes)),
        scratch,
        guard,
        text_style,
        depth + 1,
    )?;
    Ok(nested)
}

fn open_flat(
    src: Arc<dyn Source>,
    scratch: &Path,
    guard: &ExtractGuard,
    text_style: TextStyle,
) -> Result<Arc<dyn Container>> {
    let txt = TxtProvider {
        text_style: text_style.clone(),
    };
    let zip = ZipProvider {
        text_style,
        ..ZipProvider::default()
    };
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
    // テキストは目印が無く何でも通りうるので、必ず最後に試す
    let providers: [&dyn ContainerProvider; 5] = [&zip, &sevenz, &rar, &PdfProvider, &txt];
    for p in providers {
        if p.probe(src.as_ref()) == Probe::Match {
            return p.open(src);
        }
    }
    Err(ContainerError::Unsupported)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::font::font_for;
    use crate::text_style::Typeface;

    fn write_novel_epub(path: &Path) {
        let container =
            r#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#;
        let opf = r#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c"/></spine></package>"#;
        let page = format!(
            "<html><body><p>{}</p></body></html>",
            "本文です。".repeat(800)
        );
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in [
            ("META-INF/container.xml", container),
            ("c.opf", opf),
            ("c.xhtml", page.as_str()),
        ] {
            z.start_file(name, opts).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    /// 書庫（ZIP）の中に EPUB だけが入っているときは、その EPUB を本として開く
    #[test]
    fn epub_inside_zip_is_opened_as_book() {
        if font_for(&Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let epub = dir.path().join("novel.epub");
        write_novel_epub(&epub);
        let outer = dir.path().join("books.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&outer).unwrap());
        z.start_file("novel.epub", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(&std::fs::read(&epub).unwrap()).unwrap();
        z.finish().unwrap();

        let direct = open_path(&epub).unwrap().entries().len();
        let c = open_path(&outer).unwrap();
        assert!(c.entries().iter().any(|e| e.kind == EntryKind::Image));
        assert_eq!(c.entries().len(), direct);
    }

    /// 外側・内側の ZIP がどの圧縮方式でも、中の EPUB を開ける
    #[test]
    fn epub_inside_zip_opens_for_every_compression_method() {
        use zip::CompressionMethod as M;
        if font_for(&Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let container =
            r#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#;
        let opf = r#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c"/></spine></package>"#;
        let page = format!(
            "<html><body><p>{}</p></body></html>",
            "本文です。".repeat(800)
        );
        let build = |files: &[(&str, &[u8])], m: M| {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let o = zip::write::SimpleFileOptions::default().compression_method(m);
            for (n, d) in files {
                z.start_file(*n, o).unwrap();
                z.write_all(d).unwrap();
            }
            z.finish().unwrap().into_inner()
        };
        let methods = [M::Stored, M::Deflated, M::Bzip2, M::Zstd];
        for inner_m in methods {
            let epub = build(
                &[
                    ("META-INF/container.xml", container.as_bytes()),
                    ("c.opf", opf.as_bytes()),
                    ("c.xhtml", page.as_bytes()),
                ],
                inner_m,
            );
            for outer_m in methods {
                let outer = build(&[("novel.epub", &epub)], outer_m);
                let c = open_source(Arc::new(MemorySource(Bytes::from(outer))));
                let c = c.unwrap_or_else(|e| panic!("{inner_m:?}/{outer_m:?}: {e}"));
                assert!(
                    c.entries().iter().any(|e| e.kind == EntryKind::Image),
                    "{inner_m:?}/{outer_m:?}"
                );
            }
        }
    }

    /// 非対応の圧縮方式の EPUB は、画像なしの本ではなく圧縮方式のエラーで知らせる
    #[test]
    fn epub_with_unsupported_compression_reports_unsupported_compression() {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        z.start_file("META-INF/container.xml", o).unwrap();
        z.write_all(
            br#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#,
        )
        .unwrap();
        z.start_file("c.opf", o).unwrap();
        z.write_all(b"<package/>").unwrap();
        let mut bytes = z.finish().unwrap().into_inner();
        // 圧縮方式を 1（Shrink: 非対応）に書き換える。ローカル(+8)・セントラル(+10)の両方
        for (sig, off) in [(&b"PK\x03\x04"[..], 8), (&b"PK\x01\x02"[..], 10)] {
            let mut i = 0;
            while i + 4 <= bytes.len() {
                if &bytes[i..i + 4] == sig {
                    bytes[i + off] = 1;
                    bytes[i + off + 1] = 0;
                }
                i += 1;
            }
        }
        match open_source(Arc::new(MemorySource(Bytes::from(bytes)))) {
            Err(ContainerError::UnsupportedCompression(_)) => {}
            other => panic!(
                "expected unsupported-compression error, got {:?}",
                other.map(|c| c.entries().len())
            ),
        }
    }

    /// パスから開くときも、渡した文字スタイルでページが割り付けられる
    #[test]
    fn open_path_styled_applies_text_style() {
        if font_for(&Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("novel.epub");
        write_novel_epub(&p);
        let pages = |px| {
            let style = TextStyle {
                font_px: px,
                ..TextStyle::default()
            };
            open_path_styled(&p, style).unwrap().entries().len()
        };
        let (small, large) = (pages(32), pages(96));
        assert!(small < large, "{small} {large}");
        assert_eq!(open_path(&p).unwrap().entries().len(), pages(44));
    }
}
