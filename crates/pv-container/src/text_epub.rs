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
use crate::entry::{kind_from_path, EntryId, EntryKind, EntryMeta};
use crate::epub::{
    dir_of, is_markup, parse_xml, read_text, resolve, rootfile_path, Item, PathIndex,
    CONTAINER_XML, MAX_SPINE,
};
use crate::error::{ContainerError, Result};
use crate::font::font_for;
use crate::text_layout::{paginate, parse_xhtml, Block, Page, PageSpec};
use crate::text_style::TextStyle;
use crate::text_vertical::{draw_vertical, Canvas, Column, Look};
use crate::zip_container::ZipContainer;

/// 描画中は「文字の濃さ」（0 = 背景、255 = 文字色）だけを持ち、最後に色を混ぜる
const INK: u8 = 255;
/// ページ番号は本文より薄く
const FOOTER_INK: u8 = 140;
const FOOTER_PX: f32 = 28.0;
/// エントリの大きさの目安（実際の PNG の大きさではない。展開サイズの上限判定などに使われるだけ）
const NOMINAL_SIZE: u64 = 80_000;

/// 1 ページの中身
enum PageKind {
    /// 文章のページ（開くときに割り付け、読み出し時に描く）
    Text(Page),
    /// 画像のページ（EPUB 内の画像をそのまま返す）
    Image(EntryId),
}

/// 画像のある EPUB で、これ以上の本文（文字数）があれば、画像と文章の混在した本として読む。
/// これ未満なら、コミック・写真集として画像だけを並べる（見出しや注記が少しあるだけの本のため）。
const MIXED_MIN_CHARS: usize = 300;

pub struct TextEpubContainer {
    zip: ZipContainer,
    entries: Vec<EntryMeta>,
    pages: Vec<PageKind>,
    spec: PageSpec,
    font: Arc<FontVec>,
    style: TextStyle,
}

impl TextEpubContainer {
    /// 文章の EPUB（画像の混ざったものを含む）なら成功する。EPUB でない・本文が無い・
    /// 画像が中心のコミックのときは ZIP をそのまま返す（呼び出し側が画像の本として開く）。
    /// 文章の EPUB なのに日本語フォントが見つからないときは、画像の無い本ならエラー、
    /// 画像のある本なら画像だけの本として扱う。
    /// 見た目（文字の大きさ・書体）を指定して開く。
    pub fn try_open_with(
        zip: ZipContainer,
        style: TextStyle,
    ) -> Result<std::result::Result<Self, ZipContainer>> {
        let chapters = spine_chapters(&zip);
        let images = chapters
            .iter()
            .flatten()
            .filter(|b| matches!(b, Block::Image(_)))
            .count();
        let chars: usize = chapters
            .iter()
            .flatten()
            .map(|b| match b {
                Block::Heading(_, t) | Block::Para { text: t, .. } => t.chars().count(),
                _ => 0,
            })
            .sum();
        if chapters.iter().all(Vec::is_empty) || (images > 0 && chars < MIXED_MIN_CHARS) {
            return Ok(Err(zip));
        }
        let Some(font) = font_for(style.typeface) else {
            // 画像のある本は、フォントが無くても画像だけで読めるようにする
            return if images > 0 {
                Ok(Err(zip))
            } else {
                Err(ContainerError::NoFont)
            };
        };
        let spec = PageSpec {
            font_px: style.font_px as f32,
            line_gap: style.line_spacing as f32 / 100.0,
            char_step: 1.0 + style.letter_spacing as f32 / 100.0,
            vertical: style.vertical,
            ..PageSpec::default()
        };
        let cache: RefCell<HashMap<(char, u32), f32>> = RefCell::new(HashMap::new());
        let adv = |c: char, size: f32| -> f32 {
            *cache
                .borrow_mut()
                .entry((c, size.to_bits()))
                .or_insert_with(|| advance(&font, c, size))
        };
        let index = PathIndex::new(zip.entries());
        // 文章は連続した範囲ごとに割り付け、画像は独立した 1 ページにして、読む順に並べる
        let mut pages: Vec<PageKind> = Vec::new();
        for chapter in &chapters {
            let mut run: Vec<Block> = Vec::new();
            let flush = |run: &mut Vec<Block>, pages: &mut Vec<PageKind>| {
                if run.is_empty() {
                    return;
                }
                let laid = paginate(std::slice::from_ref(run), spec, &adv);
                pages.extend(laid.into_iter().map(PageKind::Text));
                run.clear();
            };
            for b in chapter {
                if let Block::Image(path) = b {
                    flush(&mut run, &mut pages);
                    if let Some(id) = index.get(path) {
                        pages.push(PageKind::Image(id));
                    }
                } else {
                    run.push(b.clone());
                }
            }
            flush(&mut run, &mut pages);
        }
        if pages.is_empty() {
            return Ok(Err(zip));
        }
        let mut entries = Vec::with_capacity(pages.len());
        for (i, kind) in pages.iter().enumerate() {
            let (path, size) = match kind {
                PageKind::Text(_) => (format!("page_{:05}.png", i + 1), NOMINAL_SIZE),
                PageKind::Image(zid) => {
                    let m = &zip.entries()[zid.0 as usize];
                    (
                        format!(
                            "{:05}_{}",
                            i + 1,
                            m.path.rsplit('/').next().unwrap_or("img")
                        ),
                        m.size,
                    )
                }
            };
            entries.push(EntryMeta {
                id: EntryId(i as u32),
                path,
                size,
                kind: EntryKind::Image,
            });
        }
        Ok(Ok(Self {
            zip,
            entries,
            pages,
            spec,
            font,
            style: style.clamped(),
        }))
    }

    fn kind(&self, id: EntryId) -> Result<&PageKind> {
        self.pages
            .get(id.0 as usize)
            .ok_or(ContainerError::NotFound(id.0))
    }

    /// 文章のページ。画像のページなら None
    fn text_page(&self, id: EntryId) -> Result<Option<&Page>> {
        Ok(match self.kind(id)? {
            PageKind::Text(p) => Some(p),
            PageKind::Image(_) => None,
        })
    }

    fn render(&self, id: EntryId) -> Result<Vec<u8>> {
        let Some(page) = self.text_page(id)? else {
            return Err(ContainerError::NotFound(id.0));
        };
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
                    Column {
                        cx: line.x,
                        top: line.y,
                        size: line.size,
                        pitch: line.pitch,
                    },
                    &line.text,
                    INK,
                    Look {
                        bold: self.style.bold,
                        italic: self.style.italic,
                    },
                );
                continue;
            }
            let ascent = self.font.as_scaled(PxScale::from(line.size)).ascent();
            draw_text(
                &self.font,
                &mut buf,
                (w, h),
                (line.x, line.y + ascent),
                line.size,
                &line.text,
                INK,
                HLook {
                    extra: line.size * (self.spec.char_step - 1.0),
                    look: Look {
                        bold: self.style.bold,
                        italic: self.style.italic,
                    },
                },
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
            (w, h),
            ((self.spec.width - fw) / 2.0, fy),
            FOOTER_PX,
            &footer,
            FOOTER_INK,
            HLook::default(),
        );
        encode_png(&colorize(&buf, self.style), w as u32, h as u32)
    }
}

fn advance(font: &FontVec, c: char, size: f32) -> f32 {
    font.as_scaled(PxScale::from(size))
        .h_advance(font.glyph_id(c))
}

/// 横書きの字形の見た目（文字間の追加分と、太字・斜体）
#[derive(Debug, Clone, Copy, Default)]
struct HLook {
    /// 1 文字ごとに足す送り幅（画素）
    extra: f32,
    look: Look,
}

/// 斜体の傾き（高さ 1 に対する横ずれ）
const H_ITALIC_SLANT: f32 = 0.2;

/// 文字を `baseline` に沿って描く（はみ出す部分は捨てる）。`dims` は (幅, 高さ)、`origin` は (行頭の x, ベースライン)。
#[allow(clippy::too_many_arguments)]
fn draw_text(
    font: &FontVec,
    buf: &mut [u8],
    dims: (usize, usize),
    origin: (f32, f32),
    size: f32,
    text: &str,
    ink: u8,
    look: HLook,
) {
    let (w, h) = dims;
    let (x, baseline) = origin;
    let scaled = font.as_scaled(PxScale::from(size));
    let thick = if look.look.bold { size * 0.035 } else { 0.0 };
    let offsets: &[(f32, f32)] = if look.look.bold {
        &[(0.0, 0.0), (thick, 0.0), (0.0, thick), (thick, thick)]
    } else {
        &[(0.0, 0.0)]
    };
    let mut pen = x;
    for c in text.chars() {
        let id = font.glyph_id(c);
        for &(ox, oy) in offsets {
            let glyph =
                id.with_scale_and_position(PxScale::from(size), point(pen + ox, baseline + oy));
            let Some(outlined) = font.outline_glyph(glyph) else {
                continue;
            };
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let mut px = b.min.x as i64 + i64::from(gx);
                let py = b.min.y as i64 + i64::from(gy);
                if look.look.italic {
                    // ベースラインを軸に、上ほど右へ傾ける
                    px += ((baseline - py as f32) * H_ITALIC_SLANT).round() as i64;
                }
                if px < 0 || py < 0 || px >= w as i64 || py >= h as i64 {
                    return;
                }
                let a = (cov.clamp(0.0, 1.0) * 255.0) as u32;
                let dst = &mut buf[py as usize * w + px as usize];
                *dst = ((u32::from(*dst) * (255 - a) + u32::from(ink) * a) / 255) as u8;
            });
        }
        pen += scaled.h_advance(id) + look.extra;
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
            let dir = dir_of(&path).to_owned();
            let blocks = parse_xhtml(&read_text(zip, id)?)
                .into_iter()
                .filter_map(|b| match b {
                    // 参照を EPUB 内のパスにする。見つからない・画像でないものは捨てる
                    Block::Image(src) => {
                        let p = resolve(&dir, &src)?;
                        let img = index.get(&p)?;
                        (kind_from_path(&zip.entries()[img.0 as usize].path) == EntryKind::Image)
                            .then(|| Block::Image(zip.entries()[img.0 as usize].path.clone()))
                    }
                    other => Some(other),
                })
                .collect();
            Some(blocks)
        })
        .collect()
}

impl Container for TextEpubContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        match self.kind(id)? {
            PageKind::Text(_) => Ok(Bytes::from(self.render(id)?)),
            PageKind::Image(zid) => self.zip.read(*zid),
        }
    }

    fn is_text(&self) -> bool {
        true
    }

    fn read_prefix(&self, id: EntryId, n: usize) -> Result<Bytes> {
        match self.kind(id)? {
            PageKind::Text(_) => {
                let mut head = png_head(self.spec.width as u32, self.spec.height as u32);
                head.truncate(n);
                Ok(Bytes::from(head))
            }
            PageKind::Image(zid) => self.zip.read_prefix(*zid, n),
        }
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        match self.kind(id)? {
            PageKind::Text(_) => Ok(Box::new(Cursor::new(self.render(id)?))),
            PageKind::Image(zid) => self.zip.open_reader(*zid),
        }
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

    /// 太字・斜体・文字間は、描いたページの見た目を変える
    #[test]
    fn bold_italic_and_spacing_change_the_page() {
        if font_for(Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let render = |style: TextStyle| {
            let Ok(Ok(c)) = TextEpubContainer::try_open_with(novel(), style) else {
                panic!("text epub should open");
            };
            c.read(EntryId(0)).unwrap()
        };
        let plain = render(TextStyle::default());
        assert_ne!(
            plain,
            render(TextStyle {
                bold: true,
                ..TextStyle::default()
            }),
            "太字"
        );
        assert_ne!(
            plain,
            render(TextStyle {
                italic: true,
                ..TextStyle::default()
            }),
            "斜体"
        );
        assert_ne!(
            plain,
            render(TextStyle {
                letter_spacing: 30,
                ..TextStyle::default()
            }),
            "文字間"
        );
        // 太字は、文字の画素が増える（インクの量が増える）
        let ink = |data: &[u8]| {
            let mut dec = png::Decoder::new(std::io::Cursor::new(data.to_vec()))
                .read_info()
                .unwrap();
            let mut img = vec![0u8; dec.output_buffer_size().unwrap()];
            dec.next_frame(&mut img).unwrap();
            img.iter().map(|&v| 255 - u64::from(v)).sum::<u64>()
        };
        let bold = render(TextStyle {
            bold: true,
            ..TextStyle::default()
        });
        assert!(ink(&bold) > ink(&plain));
    }

    /// 横書きでも開けて描け、縦書きとは別のページになる
    #[test]
    fn horizontal_writing_renders_differently_from_vertical() {
        if font_for(Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let render = |style: TextStyle| {
            let Ok(Ok(c)) = TextEpubContainer::try_open_with(novel(), style) else {
                panic!("text epub should open");
            };
            (c.entries().len(), c.read(EntryId(0)).unwrap())
        };
        let (_, vertical) = render(TextStyle::default());
        let (_, horizontal) = render(TextStyle {
            vertical: false,
            ..TextStyle::default()
        });
        assert_ne!(vertical, horizontal);
        // 横書きでも、文字間・太字・斜体が効く
        let plain = horizontal;
        for (name, style) in [
            (
                "bold",
                TextStyle {
                    vertical: false,
                    bold: true,
                    ..TextStyle::default()
                },
            ),
            (
                "italic",
                TextStyle {
                    vertical: false,
                    italic: true,
                    ..TextStyle::default()
                },
            ),
            (
                "spacing",
                TextStyle {
                    vertical: false,
                    letter_spacing: 30,
                    ..TextStyle::default()
                },
            ),
        ] {
            assert_ne!(plain, render(style).1, "{name}");
        }
    }

    fn epub_with(chapters: &[(&str, String)], images: &[&str]) -> ZipContainer {
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#;
        let mut manifest = String::new();
        let mut spine = String::new();
        for (i, (name, _)) in chapters.iter().enumerate() {
            manifest +=
                &format!(r#"<item id="c{i}" href="{name}" media-type="application/xhtml+xml"/>"#);
            spine += &format!(r#"<itemref idref="c{i}"/>"#);
        }
        let opf =
            format!("<package><manifest>{manifest}</manifest><spine>{spine}</spine></package>");
        let mut files: Vec<(String, Vec<u8>)> = vec![
            (
                "META-INF/container.xml".into(),
                container.as_bytes().to_vec(),
            ),
            ("OEBPS/content.opf".into(), opf.into_bytes()),
        ];
        for (name, body) in chapters {
            files.push((format!("OEBPS/{name}"), body.clone().into_bytes()));
        }
        for img in images {
            files.push((format!("OEBPS/{img}"), format!("IMG:{img}").into_bytes()));
        }
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in &files {
            z.start_file(name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        let bytes = z.finish().unwrap().into_inner();
        ZipContainer::open(
            Arc::new(MemorySource(Bytes::from(bytes))),
            Limits::default(),
        )
        .unwrap()
    }

    /// 画像と文章が混ざった EPUB は、文章のページと画像のページを、読む順に並べる
    #[test]
    fn mixed_epub_interleaves_text_pages_and_images_in_reading_order() {
        if font_for(Typeface::Gothic).is_none() {
            eprintln!("skipped: no CJK font on this machine");
            return;
        }
        let body = |a: &str, img: &str, b: &str| {
            format!(
                "<html><body><h2>題</h2><p>{}</p><img src=\"{img}\"/><p>{}</p></body></html>",
                a.repeat(300),
                b.repeat(300)
            )
        };
        let zip = epub_with(
            &[
                ("c1.xhtml", body("前の文です。", "p1.png", "後の文です。")),
                (
                    "c2.xhtml",
                    "<html><body><img src=\"p2.png\"/></body></html>".to_owned(),
                ),
            ],
            &["p1.png", "p2.png"],
        );
        let Ok(Ok(c)) = TextEpubContainer::try_open_with(zip, TextStyle::default()) else {
            panic!("mixed epub should open as text + images");
        };
        let paths: Vec<&str> = c.entries().iter().map(|e| e.path.as_str()).collect();
        let first_img = paths
            .iter()
            .position(|p| p.ends_with("p1.png"))
            .expect("p1 present");
        let second_img = paths
            .iter()
            .position(|p| p.ends_with("p2.png"))
            .expect("p2 present");
        assert!(first_img > 0, "画像の前に文章のページがある");
        assert!(
            second_img > first_img + 1,
            "1 枚目の画像のあとにも文章のページがある"
        );
        assert_eq!(second_img, paths.len() - 1, "最後は 2 枚目の画像");
        // 画像のページは元のバイト列、文章のページは PNG
        assert_eq!(
            &c.read(EntryId(first_img as u32)).unwrap()[..],
            b"IMG:p1.png"
        );
        assert_eq!(&c.read(EntryId(0)).unwrap()[1..4], b"PNG");
        assert_eq!(
            &c.read_prefix(EntryId(second_img as u32), 4).unwrap()[..],
            b"IMG:"
        );
        assert!(c.entries().iter().all(|e| e.kind == EntryKind::Image));
    }

    /// 本文がほとんど無い（見出し程度の）画像の EPUB は、コミックとして画像だけで読む
    #[test]
    fn picture_epubs_with_little_text_stay_image_only() {
        let zip = epub_with(
            &[(
                "c1.xhtml",
                "<html><body><h1>第1話</h1><img src=\"p1.png\"/></body></html>".to_owned(),
            )],
            &["p1.png"],
        );
        // フォントの有無にかかわらず、文章の本としては扱わない（呼び出し側が画像の本として開く）
        assert!(matches!(
            TextEpubContainer::try_open_with(zip, TextStyle::default()),
            Ok(Err(_))
        ));
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

/// 計測・目視用: `PV_TEXT_EPUB` を各スタイルで開き、3 ページ目を `PV_OUT` へ書き出す（通常は動かさない）。
#[cfg(test)]
#[test]
#[ignore]
fn gallery_of_styles() {
    use crate::source::FileSource;
    let path = std::env::var("PV_TEXT_EPUB").expect("PV_TEXT_EPUB");
    let out = std::env::var("PV_OUT").expect("PV_OUT");
    let styles = [
        ("plain", TextStyle::default()),
        (
            "huge",
            TextStyle {
                font_px: 140,
                ..TextStyle::default()
            },
        ),
        (
            "tight",
            TextStyle {
                line_spacing: 100,
                font_px: 36,
                ..TextStyle::default()
            },
        ),
        (
            "wide",
            TextStyle {
                letter_spacing: 40,
                ..TextStyle::default()
            },
        ),
        (
            "bold",
            TextStyle {
                bold: true,
                ..TextStyle::default()
            },
        ),
        (
            "italic",
            TextStyle {
                italic: true,
                ..TextStyle::default()
            },
        ),
        (
            "all",
            TextStyle {
                bold: true,
                italic: true,
                letter_spacing: 15,
                line_spacing: 140,
                ..TextStyle::default()
            },
        ),
        (
            "h_plain",
            TextStyle {
                vertical: false,
                ..TextStyle::default()
            },
        ),
        (
            "h_wide",
            TextStyle {
                vertical: false,
                letter_spacing: 20,
                line_spacing: 200,
                ..TextStyle::default()
            },
        ),
        (
            "h_bold_italic",
            TextStyle {
                vertical: false,
                bold: true,
                italic: true,
                ..TextStyle::default()
            },
        ),
    ];
    for (name, style) in styles {
        let zip = ZipContainer::open(
            Arc::new(FileSource::open(std::path::Path::new(&path)).unwrap()),
            crate::container::Limits::default(),
        )
        .unwrap();
        let Ok(Ok(c)) = TextEpubContainer::try_open_with(zip, style) else {
            panic!("not text epub")
        };
        eprintln!("{name}: {} pages", c.entries().len());
        std::fs::write(format!("{out}/{name}.png"), c.read(EntryId(1)).unwrap()).unwrap();
    }
}
