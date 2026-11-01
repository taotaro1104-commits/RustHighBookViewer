//! 文章主体の EPUB（Web 小説など、画像の無いもの）。
//! 本文をページに割り付け、各ページを 1 枚の画像（グレースケール PNG）として描いて返す。
//! ページ数は開くときに決まり、描画は読み出し時に行う（全ページを先に描かない）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::Arc;

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};
use bytes::Bytes;

use crate::container::Container;
use crate::entry::{EntryId, EntryKind, EntryMeta};
use crate::epub::{
    dir_of, is_markup, parse_xml, read_text, resolve, rootfile_path, Item, PathIndex,
    CONTAINER_XML, MAX_SPINE,
};
use crate::error::{ContainerError, Result};
use crate::font::font_for;
use crate::text_layout::{paginate, parse_xhtml, Block, Page, PageSpec};
use crate::text_style::TextStyle;
use crate::text_vertical::{draw_vertical, Canvas};
use crate::zip_container::ZipContainer;

/// 描画中は「文字の濃さ」（0 = 背景、255 = 文字色）だけを持ち、最後に色を混ぜる
const INK: u8 = 255;
/// ページ番号は本文より薄く
const FOOTER_INK: u8 = 140;
const FOOTER_PX: f32 = 28.0;
/// エントリの大きさの目安（実際の PNG の大きさではない。展開サイズの上限判定などに使われるだけ）
const NOMINAL_SIZE: u64 = 80_000;

pub struct TextEpubContainer {
    entries: Vec<EntryMeta>,
    pages: Vec<Page>,
    spec: PageSpec,
    font: Arc<FontVec>,
    style: TextStyle,
}

impl TextEpubContainer {
    /// 文章の EPUB なら成功する。EPUB でない・本文が無いときは ZIP をそのまま返す。
    /// 文章の EPUB なのに日本語フォントが見つからないときだけエラー。
    /// 見た目（文字の大きさ・書体）を指定して開く。
    pub fn try_open_with(
        zip: ZipContainer,
        style: TextStyle,
    ) -> Result<std::result::Result<Self, ZipContainer>> {
        let chapters = spine_chapters(&zip);
        if chapters.iter().all(Vec::is_empty) {
            return Ok(Err(zip));
        }
        let font = font_for(style.typeface).ok_or(ContainerError::NoFont)?;
        let spec = PageSpec {
            font_px: style.font_px as f32,
            line_gap: style.line_spacing as f32 / 100.0,
            ..PageSpec::default()
        };
        let cache: RefCell<HashMap<(char, u32), f32>> = RefCell::new(HashMap::new());
        let adv = |c: char, size: f32| -> f32 {
            *cache
                .borrow_mut()
                .entry((c, size.to_bits()))
                .or_insert_with(|| advance(&font, c, size))
        };
        let pages = paginate(&chapters, spec, &adv);
        if pages.is_empty() {
            return Ok(Err(zip));
        }
        let entries = (0..pages.len())
            .map(|i| EntryMeta {
                id: EntryId(i as u32),
                path: format!("page_{:05}.png", i + 1),
                size: NOMINAL_SIZE,
                kind: EntryKind::Image,
            })
            .collect();
        Ok(Ok(Self {
            entries,
            pages,
            spec,
            font,
            style: style.clamped(),
        }))
    }

    fn page(&self, id: EntryId) -> Result<&Page> {
        self.pages
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))
    }

    fn render(&self, id: EntryId) -> Result<Vec<u8>> {
        let page = self.page(id)?;
        let (w, h) = (self.spec.width as usize, self.spec.height as usize);
        let mut buf = vec![0u8; w * h];
        for line in &page.lines {
            if self.spec.vertical {
                let mut canvas = Canvas {
                    buf: &mut buf,
                    w,
                    h,
                };
                draw_vertical(
                    &self.font,
                    &mut canvas,
                    line.x,
                    line.y,
                    line.size,
                    &line.text,
                    INK,
                );
                continue;
            }
            let ascent = self.font.as_scaled(PxScale::from(line.size)).ascent();
            draw_text(
                &self.font,
                &mut buf,
                w,
                h,
                line.x,
                line.y + ascent,
                line.size,
                &line.text,
                INK,
            );
        }
        let footer = format!("{} / {}", id.0 + 1, self.pages.len());
        let fw: f32 = footer
            .chars()
            .map(|c| advance(&self.font, c, FOOTER_PX))
            .sum();
        let fy = self.spec.height - self.spec.margin_bottom / 2.0 + FOOTER_PX / 2.0;
        draw_text(
            &self.font,
            &mut buf,
            w,
            h,
            (self.spec.width - fw) / 2.0,
            fy,
            FOOTER_PX,
            &footer,
            FOOTER_INK,
        );
        encode_png(&colorize(&buf, self.style), w as u32, h as u32)
    }
}

fn advance(font: &FontVec, c: char, size: f32) -> f32 {
    font.as_scaled(PxScale::from(size))
        .h_advance(font.glyph_id(c))
}

/// 文字を `baseline` に沿って描く（はみ出す部分は捨てる）。
#[allow(clippy::too_many_arguments)]
fn draw_text(
    font: &FontVec,
    buf: &mut [u8],
    w: usize,
    h: usize,
    x: f32,
    baseline: f32,
    size: f32,
    text: &str,
    ink: u8,
) {
    let scaled = font.as_scaled(PxScale::from(size));
    let mut pen = x;
    for c in text.chars() {
        let id = font.glyph_id(c);
        let glyph = id.with_scale_and_position(PxScale::from(size), point(pen, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let px = b.min.x as i64 + i64::from(gx);
                let py = b.min.y as i64 + i64::from(gy);
                if px < 0 || py < 0 || px >= w as i64 || py >= h as i64 {
                    return;
                }
                let a = (cov.clamp(0.0, 1.0) * 255.0) as u32;
                let dst = &mut buf[py as usize * w + px as usize];
                *dst = ((u32::from(*dst) * (255 - a) + u32::from(ink) * a) / 255) as u8;
            });
        }
        pen += scaled.h_advance(id);
    }
}

/// 文字の濃さ（0〜255）を、背景色から文字色へのグラデーションとして RGB にする。
fn colorize(coverage: &[u8], style: TextStyle) -> Vec<u8> {
    let channel = |c: u32, shift: u32| ((c >> shift) & 0xFF) as i32;
    // 濃さごとの色を先に表にしておく（画素数より、濃さの段階の方がずっと少ない）
    let mut lut = [[0u8; 3]; 256];
    for (cov, px) in lut.iter_mut().enumerate() {
        for (k, shift) in [16u32, 8, 0].into_iter().enumerate() {
            let (p, i) = (channel(style.paper, shift), channel(style.ink, shift));
            px[k] = (p + (i - p) * cov as i32 / 255) as u8;
        }
    }
    let mut rgb = Vec::with_capacity(coverage.len() * 3);
    for &c in coverage {
        rgb.extend_from_slice(&lut[c as usize]);
    }
    rgb
}

fn encode_png(rgb: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(96 * 1024);
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let io = |e: png::EncodingError| ContainerError::Archive(e.to_string());
    let mut writer = enc.write_header().map_err(io)?;
    writer.write_image_data(rgb).map_err(io)?;
    writer.finish().map_err(io)?;
    Ok(out)
}

/// 寸法の確認（先頭だけを読む処理）に答えるための、署名 + IHDR + 空の IDAT だけの PNG の先頭部分。
/// 本物のページを描かずに済ませるためのもの（全ページの寸法確認で全ページを描くと遅い）。
fn png_head(w: u32, h: u32) -> Vec<u8> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32fast::hash(&body).to_be_bytes());
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 ビット・RGB
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &[]);
    out
}

/// spine（読む順）の XHTML を読み、章ごとの本文にする。
fn spine_chapters(zip: &ZipContainer) -> Vec<Vec<Block>> {
    let index = PathIndex::new(zip.entries());
    let Some(container) = index.get(CONTAINER_XML).and_then(|id| read_text(zip, id)) else {
        return Vec::new();
    };
    let Some(opf_path) = rootfile_path(&container) else {
        return Vec::new();
    };
    let Some(opf_text) = index.get(&opf_path).and_then(|id| read_text(zip, id)) else {
        return Vec::new();
    };
    let Some(opf) = parse_xml(&opf_text) else {
        return Vec::new();
    };
    let base = dir_of(&opf_path);
    let mut manifest: HashMap<&str, Item> = HashMap::new();
    for n in opf
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "item")
    {
        if let (Some(id), Some(href)) = (n.attribute("id"), n.attribute("href")) {
            manifest.insert(
                id,
                Item {
                    href: href.to_owned(),
                    media_type: n.attribute("media-type").unwrap_or("").to_owned(),
                    properties: String::new(),
                },
            );
        }
    }
    opf.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
        .take(MAX_SPINE)
        .filter_map(|r| manifest.get(r.attribute("idref")?))
        .filter_map(|item| {
            let path = resolve(base, &item.href)?;
            is_markup(item, &path).then_some(())?;
            let id = index.get(&path)?;
            Some(parse_xhtml(&read_text(zip, id)?))
        })
        .collect()
}

impl Container for TextEpubContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        Ok(Bytes::from(self.render(id)?))
    }

    fn is_text(&self) -> bool {
        true
    }

    fn read_prefix(&self, id: EntryId, n: usize) -> Result<Bytes> {
        self.page(id)?;
        let mut head = png_head(self.spec.width as u32, self.spec.height as u32);
        head.truncate(n);
        Ok(Bytes::from(head))
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        Ok(Box::new(Cursor::new(self.render(id)?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Limits;
    use crate::source::MemorySource;
    use crate::text_style::Typeface;
    use std::io::Write;

    fn zip_bytes(files: &[(&str, &str)]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    fn novel() -> ZipContainer {
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;
        let opf = r#"<package><manifest>
<item id="c" href="c1.xhtml" media-type="application/xhtml+xml"/>
<item id="d" href="c2.xhtml" media-type="application/xhtml+xml"/>
<item id="css" href="s.css" media-type="text/css"/></manifest>
<spine><itemref idref="c"/><itemref idref="d"/></spine></package>"#;
        let ch = |t: &str| {
            format!(
                "<html><body><h2>{t}</h2><p>{}</p></body></html>",
                "本文です。".repeat(400)
            )
        };
        let files = zip_bytes(&[
            ("META-INF/container.xml", container),
            ("OEBPS/content.opf", opf),
            ("OEBPS/c1.xhtml", &ch("第1話")),
            ("OEBPS/c2.xhtml", &ch("第2話")),
            ("OEBPS/s.css", "p{}"),
        ]);
        ZipContainer::open(
            Arc::new(MemorySource(Bytes::from(files))),
            Limits::default(),
        )
        .unwrap()
    }

    #[test]
    fn png_head_has_signature_and_dimensions() {
        let h = png_head(1080, 1620);
        assert_eq!(&h[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&h[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(h[16..20].try_into().unwrap()), 1080);
        assert_eq!(u32::from_be_bytes(h[20..24].try_into().unwrap()), 1620);
    }

    #[test]
    fn spine_text_is_split_into_chapters() {
        let ch = spine_chapters(&novel());
        assert_eq!(ch.len(), 2);
        assert_eq!(ch[0][0], Block::Heading(2, "第1話".into()));
    }

    /// 文字色と背景色が、描いたページに反映される
    #[test]
    fn page_uses_the_chosen_colors() {
        if font_for(Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let style = TextStyle {
            ink: 0xFF_0000,
            paper: 0x00_00FF,
            ..TextStyle::default()
        };
        let Ok(Ok(c)) = TextEpubContainer::try_open_with(novel(), style) else {
            panic!("text epub should open");
        };
        let data = c.read(EntryId(0)).unwrap();
        let mut dec = png::Decoder::new(std::io::Cursor::new(data.to_vec()))
            .read_info()
            .unwrap();
        let mut img = vec![0u8; dec.output_buffer_size().unwrap()];
        let info = dec.next_frame(&mut img).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgb);
        let px: &[[u8; 3]] = img.as_chunks::<3>().0;
        assert_eq!(px[0], [0, 0, 255], "角は背景色");
        // 文字のある場所には、背景色（青）から文字色（赤）への途中の色や、文字色そのものがある
        assert!(
            px.iter().any(|p| p[0] > 200 && p[2] < 60),
            "文字色の画素がある"
        );
        // 文字色と背景色以外の成分（緑）は出ない
        assert!(px.iter().all(|p| p[1] == 0));
    }

    /// 文字を大きくするとページ数が増え、書体を変えても開ける（明朝体が無い環境ではゴシック体で代用）
    #[test]
    fn style_changes_pagination() {
        if font_for(Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let pages = |px, face| {
            let style = TextStyle {
                font_px: px,
                typeface: face,
                ..TextStyle::default()
            };
            let Ok(Ok(c)) = TextEpubContainer::try_open_with(novel(), style) else {
                panic!("text epub should open");
            };
            c.entries().len()
        };
        let (small, normal, large) = (
            pages(32, Typeface::Gothic),
            pages(44, Typeface::Gothic),
            pages(64, Typeface::Gothic),
        );
        assert!(small < normal && normal < large, "{small} {normal} {large}");
        assert!(pages(44, Typeface::Mincho) >= 2);
    }

    /// フォントがある環境でだけ、実際に描いて確かめる
    #[test]
    fn pages_render_when_a_font_is_available() {
        let Some(_) = font_for(Typeface::Gothic) else {
            eprintln!("skipped: no CJK font on this machine");
            return;
        };
        let Ok(Ok(c)) = TextEpubContainer::try_open_with(novel(), TextStyle::default()) else {
            panic!("text epub should open");
        };
        assert!(
            c.entries().len() >= 4,
            "two long chapters make several pages"
        );
        let png = c.read(EntryId(0)).unwrap();
        assert_eq!(&png[..4], b"\x89PNG");
        assert!(png.len() > 1000);
        assert_eq!(
            c.read_prefix(EntryId(1), 64).unwrap().len(),
            64.min(png_head(1, 1).len())
        );
        assert!(c.read(EntryId(9999)).is_err());
    }
}

/// 計測用: `PV_TEXT_EPUB` の EPUB を開き、レイアウトと描画の時間を表示する（通常は動かさない）。
#[cfg(test)]
#[test]
#[ignore]
fn bench_real_text_epub() {
    use crate::source::FileSource;
    let path = std::env::var("PV_TEXT_EPUB").expect("PV_TEXT_EPUB");
    let t = std::time::Instant::now();
    let zip = ZipContainer::open(
        Arc::new(FileSource::open(std::path::Path::new(&path)).unwrap()),
        crate::container::Limits::default(),
    )
    .unwrap();
    eprintln!("zip open: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let _ = font_for(crate::text_style::Typeface::Gothic).expect("font");
    eprintln!("font load: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    let chapters = spine_chapters(&zip);
    eprintln!("parse {} chapters: {:?}", chapters.len(), t.elapsed());
    let t = std::time::Instant::now();
    let Ok(Ok(c)) = TextEpubContainer::try_open_with(zip, TextStyle::default()) else {
        panic!("not text epub")
    };
    eprintln!(
        "open (parse+layout) -> {} pages: {:?}",
        c.entries().len(),
        t.elapsed()
    );
    let t = std::time::Instant::now();
    for i in 0..5 {
        c.read(EntryId(i)).unwrap();
    }
    eprintln!("render 5 pages: {:?}", t.elapsed());
    if let Ok(dir) = std::env::var("PV_OUT") {
        for i in [0u32, 3, 7] {
            std::fs::write(format!("{dir}/p{i}.png"), c.read(EntryId(i)).unwrap()).unwrap();
        }
    }
}
