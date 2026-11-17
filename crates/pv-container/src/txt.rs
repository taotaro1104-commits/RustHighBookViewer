//! テキストファイル（.txt）。内容を文章の EPUB に組み直し、EPUB と同じ描画で読めるようにする。
//! 拡張子は見ず、内容（NUL を含まない・文字コードが判定できる）で判定する。他の形式の判定より後に試す。

use std::io::{Cursor, Write};
use std::sync::Arc;

use bytes::Bytes;

use crate::container::{Container, ContainerProvider, Limits, Probe};
use crate::entry::{kind_from_path, EntryKind};
use crate::error::{ContainerError, Result};
use crate::source::{MemorySource, Source};
use crate::text_epub::TextEpubContainer;
use crate::text_style::TextStyle;
use crate::zip_container::ZipContainer;

/// 読み込むテキストの大きさの上限
const MAX_TXT_BYTES: u64 = 64 << 20;
/// 先頭のこの範囲に NUL があればバイナリとみなす
const SNIFF_BYTES: usize = 8192;

const CONTAINER_XML: &str =
    r#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#;
/// 1 章（XHTML）の本文の大きさの目安。EPUB の XHTML は先頭 `MAX_XML` までしか読まれないので、それより十分小さく区切る
const CHAPTER_BYTES: usize = 512 << 10;

#[derive(Default)]
pub struct TxtProvider {
    pub text_style: TextStyle,
}

impl ContainerProvider for TxtProvider {
    fn probe(&self, src: &dyn Source) -> Probe {
        // 書庫・PDF・画像の拡張子なのに中身が読めないファイルを、文字化けした本にしない
        if src
            .extension_hint()
            .is_some_and(|e| e != "txt" && kind_from_path(&format!("x.{e}")) != EntryKind::Other)
        {
            return Probe::NoMatch;
        }
        if src.is_empty() || src.len() > MAX_TXT_BYTES {
            return Probe::NoMatch;
        }
        let mut head = vec![0u8; SNIFF_BYTES.min(src.len() as usize)];
        match src.read_at(0, &mut head) {
            Ok(n) if n > 0 && !head[..n].contains(&0) => Probe::Match,
            _ => Probe::NoMatch,
        }
    }

    fn open(&self, src: Arc<dyn Source>) -> Result<Arc<dyn Container>> {
        let mut raw = vec![0u8; src.len() as usize];
        let mut off = 0usize;
        while off < raw.len() {
            let n = src.read_at(off as u64, &mut raw[off..])?;
            if n == 0 {
                break;
            }
            off += n;
        }
        raw.truncate(off);
        let zip = text_to_epub(&decode_text(&raw))?;
        match TextEpubContainer::try_open_with(zip, self.text_style.clone())? {
            Ok(text) => Ok(Arc::new(text)),
            Err(_) => Err(ContainerError::Unsupported),
        }
    }
}

/// 文字コード（UTF-8 / UTF-16 / Shift_JIS など）を判定して文字列にする
fn decode_text(raw: &[u8]) -> String {
    if let Some((enc, bom)) = encoding_rs::Encoding::for_bom(raw) {
        return enc.decode_without_bom_handling(&raw[bom..]).0.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_owned();
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Allow);
    det.feed(raw, true);
    let enc = det.guess(None, chardetng::Utf8Detection::Deny);
    enc.decode_without_bom_handling(raw).0.into_owned()
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// 1 行を 1 段落とし、一定の大きさごとに章（XHTML）へ区切って、最小の EPUB（無圧縮の ZIP）に包む
fn text_to_epub(text: &str) -> Result<ZipContainer> {
    let mut chapters: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
        cur.push_str("<p>");
        cur.push_str(&escape(line));
        cur.push_str("</p>");
        if cur.len() >= CHAPTER_BYTES {
            chapters.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() || chapters.is_empty() {
        chapters.push(cur);
    }
    let manifest: String = (0..chapters.len())
        .map(|i| {
            format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)
        })
        .collect();
    let spine: String = (0..chapters.len())
        .map(|i| format!(r#"<itemref idref="c{i}"/>"#))
        .collect();
    let opf = format!("<package><manifest>{manifest}</manifest><spine>{spine}</spine></package>");
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    // 無圧縮にして、読み出し時の圧縮率の上限に掛からないようにする
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut put = |name: &str, data: &str| -> Result<()> {
        z.start_file(name, opts)?;
        z.write_all(data.as_bytes())?;
        Ok(())
    };
    put("META-INF/container.xml", CONTAINER_XML)?;
    put("c.opf", &opf)?;
    for (i, body) in chapters.iter().enumerate() {
        put(
            &format!("c{i}.xhtml"),
            &format!("<html><body>{body}</body></html>"),
        )?;
    }
    let bytes = z.finish()?.into_inner();
    ZipContainer::open(
        Arc::new(MemorySource(Bytes::from(bytes))),
        Limits::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::EntryKind;
    use crate::font::font_for;
    use crate::text_style::Typeface;

    #[test]
    fn decodes_utf8_sjis_and_utf16() {
        assert_eq!(decode_text("こんにちは".as_bytes()), "こんにちは");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("吾輩は猫である。名前はまだ無い。");
        assert_eq!(decode_text(&sjis), "吾輩は猫である。名前はまだ無い。");
        let mut u16le = vec![0xff, 0xfe];
        u16le.extend("abc".encode_utf16().flat_map(|c| c.to_le_bytes()));
        assert_eq!(decode_text(&u16le), "abc");
    }

    #[test]
    fn binary_and_empty_are_not_matched() {
        let p = TxtProvider::default();
        assert_eq!(
            p.probe(&MemorySource(Bytes::from_static(b"a\0b"))),
            Probe::NoMatch
        );
        assert_eq!(p.probe(&MemorySource(Bytes::new())), Probe::NoMatch);
        assert_eq!(
            p.probe(&MemorySource(Bytes::from_static(b"hello"))),
            Probe::Match
        );
    }

    #[test]
    fn text_becomes_pages() {
        if font_for(&Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let text = "本文です。<&>\n\n".repeat(600);
        let c = TxtProvider::default()
            .open(Arc::new(MemorySource(Bytes::from(text))))
            .unwrap();
        assert!(c.is_text());
        assert!(!c.entries().is_empty());
        assert!(c.entries().iter().all(|e| e.kind == EntryKind::Image));
        assert_eq!(&c.read(c.entries()[0].id).unwrap()[1..4], b"PNG");
    }

    /// ZIP の中に txt だけが入っているときは、その txt を本として開く
    #[test]
    fn txt_inside_zip_is_opened_as_book() {
        if font_for(&Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("novel.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(
            "本文です。
"
            .repeat(500)
            .as_bytes(),
        )
        .unwrap();
        let bytes = z.finish().unwrap().into_inner();
        let c = crate::open_source(Arc::new(MemorySource(Bytes::from(bytes)))).unwrap();
        assert!(c.is_text());
        assert_eq!(&c.read(c.entries()[0].id).unwrap()[1..4], b"PNG");
    }

    /// 画像のある書庫に README.txt が混ざっていても、画像の本のまま
    #[test]
    fn readme_in_image_zip_does_not_hijack() {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        z.start_file("1.png", o).unwrap();
        z.write_all(b"x").unwrap();
        z.start_file("readme.txt", o).unwrap();
        z.write_all(b"hello").unwrap();
        let bytes = z.finish().unwrap().into_inner();
        let c = crate::open_source(Arc::new(MemorySource(Bytes::from(bytes)))).unwrap();
        assert!(!c.is_text());
        assert!(c.entries().iter().any(|e| e.path == "1.png"));
    }

    /// 4MiB を超える長いテキストでも、末尾まで章に分かれて残る
    #[test]
    fn long_text_is_split_into_chapters_without_loss() {
        let line = "あ".repeat(200);
        let text = format!(
            "{line}
"
        )
        .repeat(20_000); // UTF-8 で約 12MB
        let zip = text_to_epub(&text).unwrap();
        let xhtml = zip
            .entries()
            .iter()
            .filter(|e| e.path.ends_with(".xhtml"))
            .count();
        assert!(xhtml > 10, "{xhtml}");
        let total: usize = zip
            .entries()
            .iter()
            .filter(|e| e.path.ends_with(".xhtml"))
            .map(|e| zip.read(e.id).unwrap().len())
            .sum();
        assert!(total > text.len());
    }
}
