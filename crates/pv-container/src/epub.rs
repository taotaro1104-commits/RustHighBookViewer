//! EPUB（コミック・写真集のように、1 ページが 1 枚の画像でできているもの）。
//!
//! EPUB の中身は ZIP。画像を名前順に並べるだけでは読む順にならないので、
//! `META-INF/container.xml` → OPF → spine の順にたどり、各ページ（XHTML）が参照する画像を
//! 出てきた順に並べる。画像が 1 枚も見つからない（テキスト主体の）EPUB は通常の ZIP として扱う。

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::OnceLock;

use bytes::Bytes;
use regex::Regex;

use crate::container::Container;
use crate::entry::{kind_from_path, EntryId, EntryKind, EntryMeta};
use crate::error::Result;
use crate::zip_container::ZipContainer;

pub(crate) const CONTAINER_XML: &str = "META-INF/container.xml";
/// OPF・XHTML を読む上限（壊れた/悪意あるファイルでメモリを使い切らない）
pub(crate) const MAX_XML: usize = 4 << 20;
/// spine の項目数の上限
pub(crate) const MAX_SPINE: usize = 100_000;

pub struct EpubContainer {
    zip: ZipContainer,
    /// 読む順に並べた画像だけ
    entries: Vec<EntryMeta>,
    /// entries[i] に対応する、内側の ZIP でのエントリ
    zip_ids: Vec<EntryId>,
}

impl EpubContainer {
    /// EPUB として画像を 1 枚以上たどれたときだけ成功する。そうでなければ ZIP をそのまま返す。
    pub fn from_zip(zip: ZipContainer) -> std::result::Result<Self, ZipContainer> {
        let Some(order) = spine_images(&zip) else {
            return Err(zip);
        };
        let entries = order
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let m = &zip.entries()[id.0 as usize];
                EntryMeta {
                    id: EntryId(i as u32),
                    path: m.path.clone(),
                    size: m.size,
                    kind: EntryKind::Image,
                }
            })
            .collect();
        Ok(Self {
            zip,
            entries,
            zip_ids: order,
        })
    }

    fn inner(&self, id: EntryId) -> Result<EntryId> {
        self.zip_ids
            .get(id.0 as usize)
            .copied()
            .ok_or(crate::error::ContainerError::NotFound(id.0))
    }
}

impl Container for EpubContainer {
    fn entries(&self) -> &[EntryMeta] {
        &self.entries
    }

    fn read(&self, id: EntryId) -> Result<Bytes> {
        self.zip.read(self.inner(id)?)
    }

    fn read_prefix(&self, id: EntryId, n: usize) -> Result<Bytes> {
        self.zip.read_prefix(self.inner(id)?, n)
    }

    fn open_reader(&self, id: EntryId) -> Result<Box<dyn Read + Send>> {
        self.zip.open_reader(self.inner(id)?)
    }
}

/// ZIP 内のパス → エントリ。大文字小文字の違いは、完全一致が無いときだけ許す。
pub(crate) struct PathIndex {
    exact: HashMap<String, EntryId>,
    lower: HashMap<String, EntryId>,
}

impl PathIndex {
    pub(crate) fn new(entries: &[EntryMeta]) -> Self {
        let mut exact = HashMap::with_capacity(entries.len());
        let mut lower = HashMap::with_capacity(entries.len());
        for e in entries {
            exact.insert(e.path.clone(), e.id);
            lower.entry(e.path.to_lowercase()).or_insert(e.id);
        }
        Self { exact, lower }
    }

    pub(crate) fn get(&self, path: &str) -> Option<EntryId> {
        self.exact
            .get(path)
            .or_else(|| self.lower.get(&path.to_lowercase()))
            .copied()
    }
}

pub(crate) struct Item {
    pub(crate) href: String,
    pub(crate) media_type: String,
    pub(crate) properties: String,
}

pub(crate) fn read_text(zip: &ZipContainer, id: EntryId) -> Option<String> {
    let bytes = zip.read_prefix(id, MAX_XML).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    Some(text.trim_start_matches('\u{feff}').to_owned())
}

pub(crate) fn parse_xml(text: &str) -> Option<roxmltree::Document<'_>> {
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true, // XHTML 由来の DOCTYPE を持つ OPF がある
        ..Default::default()
    };
    roxmltree::Document::parse_with_options(text, opts).ok()
}

pub(crate) fn rootfile_path(container_xml: &str) -> Option<String> {
    let doc = parse_xml(container_xml)?;
    let node = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "rootfile")?;
    node.attribute("full-path").map(str::to_owned)
}

pub(crate) fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// `base_dir`（ZIP 内のディレクトリ）から見た相対参照を、ZIP 内のパスにする。
/// 外部 URL・data URI・ZIP の外へ出る参照は None。
pub(crate) fn resolve(base_dir: &str, href: &str) -> Option<String> {
    let href = href.split(['#', '?']).next()?.trim();
    if href.is_empty() || href.contains(':') {
        return None; // http: / data: / mailto: など
    }
    let decoded = percent_decode(href);
    let mut segs: Vec<&str> = if decoded.starts_with('/') {
        Vec::new() // ZIP のルートからの絶対パス
    } else {
        base_dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for s in decoded.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop()?; // ルートより上へは出られない
            }
            s => segs.push(s),
        }
    }
    (!segs.is_empty()).then(|| segs.join("/"))
}

/// XHTML の中で参照されている画像（`<img src>` / SVG の `<image href|xlink:href>`）を出てきた順に返す。
fn image_refs(xhtml: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r#"(?is)<(?:img|image)\b[^>]*?\b(?:src|href)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
            .expect("valid regex")
    });
    re.captures_iter(xhtml)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)))
        .map(|m| m.as_str().to_owned())
        .collect()
}

pub(crate) fn is_markup(item: &Item, path: &str) -> bool {
    let mt = item.media_type.to_ascii_lowercase();
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    mt.contains("html")
        || mt.contains("xml")
        || matches!(ext.as_deref(), Some("xhtml" | "html" | "htm"))
}

/// 読む順の画像。1 枚も無ければ None（EPUB として扱わない）。
fn spine_images(zip: &ZipContainer) -> Option<Vec<EntryId>> {
    let index = PathIndex::new(zip.entries());
    let container = read_text(zip, index.get(CONTAINER_XML)?)?;
    let opf_path = rootfile_path(&container)?;
    let opf_text = read_text(zip, index.get(&opf_path)?)?;
    let opf = parse_xml(&opf_text)?;
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
                    properties: n.attribute("properties").unwrap_or("").to_owned(),
                },
            );
        }
    }

    let mut order: Vec<EntryId> = Vec::new();
    let mut seen: HashSet<EntryId> = HashSet::new();
    let mut push = |id: EntryId, order: &mut Vec<EntryId>| {
        if seen.insert(id) {
            order.push(id);
        }
    };

    for itemref in opf
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "itemref")
        .take(MAX_SPINE)
    {
        let Some(item) = itemref.attribute("idref").and_then(|r| manifest.get(r)) else {
            continue;
        };
        let Some(path) = resolve(base, &item.href) else {
            continue;
        };
        let Some(id) = index.get(&path) else {
            continue;
        };
        if item.media_type.starts_with("image/") {
            push(id, &mut order);
        } else if is_markup(item, &path) {
            let Some(text) = read_text(zip, id) else {
                continue;
            };
            for r in image_refs(&text) {
                let Some(p) = resolve(dir_of(&path), &r) else {
                    continue;
                };
                if let Some(img) = index.get(&p) {
                    if kind_from_path(&zip.entries()[img.0 as usize].path) == EntryKind::Image {
                        push(img, &mut order);
                    }
                }
            }
        }
    }
    if order.is_empty() {
        return None;
    }

    // 表紙がページ（spine）に含まれていないときは、先頭に置く
    let cover_id = manifest
        .values()
        .find(|i| i.properties.split_whitespace().any(|p| p == "cover-image"))
        .map(|i| i.href.as_str())
        .or_else(|| {
            let cover = opf
                .descendants()
                .find(|n| {
                    n.is_element()
                        && n.tag_name().name() == "meta"
                        && n.attribute("name") == Some("cover")
                })?
                .attribute("content")?;
            manifest.get(cover).map(|i| i.href.as_str())
        })
        .and_then(|href| resolve(base, href))
        .and_then(|p| index.get(&p))
        .filter(|id| kind_from_path(&zip.entries()[id.0 as usize].path) == EntryKind::Image);
    if let Some(cover) = cover_id {
        if !order.contains(&cover) {
            order.insert(0, cover);
        }
    }
    Some(order)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::Limits;
    use crate::source::MemorySource;
    use std::io::Write;
    use std::sync::Arc;

    fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    fn open(files: &[(&str, &[u8])]) -> ZipContainer {
        let src = Arc::new(MemorySource(Bytes::from(zip_bytes(files))));
        ZipContainer::open(src, Limits::default()).unwrap()
    }

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

    fn opf(manifest: &str, spine: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>t</dc:title></metadata>
  <manifest>{manifest}</manifest>
  <spine page-progression-direction="rtl">{spine}</spine>
</package>"#
        )
    }

    fn page(img: &str) -> String {
        format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="{img}" alt=""/></body></html>"#
        )
    }

    fn paths(c: &EpubContainer) -> Vec<&str> {
        c.entries().iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn follows_spine_order_not_file_name_order() {
        let opf = opf(
            r#"<item id="pb" href="text/b.xhtml" media-type="application/xhtml+xml"/>
               <item id="pc" href="text/c.xhtml" media-type="application/xhtml+xml"/>
               <item id="pa" href="text/a.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<itemref idref="pb"/><itemref idref="pc"/><itemref idref="pa"/>"#,
        );
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="../images/c.png" width="1" height="1"/></svg>"#;
        let zip = open(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/text/b.xhtml", page("../images/b.png").as_bytes()),
            ("OEBPS/text/c.xhtml", svg.as_bytes()),
            ("OEBPS/text/a.xhtml", page("../images/a.png").as_bytes()),
            ("OEBPS/images/a.png", b"A"),
            ("OEBPS/images/b.png", b"B"),
            ("OEBPS/images/c.png", b"C"),
        ]);
        let c = EpubContainer::from_zip(zip).ok().expect("epub");
        assert_eq!(
            paths(&c),
            [
                "OEBPS/images/b.png",
                "OEBPS/images/c.png",
                "OEBPS/images/a.png"
            ]
        );
        assert_eq!(&c.read(EntryId(0)).unwrap()[..], b"B");
        assert_eq!(&c.read_prefix(EntryId(2), 0).unwrap()[..], b"");
        assert!(c.read(EntryId(9)).is_err());
        assert!(c.entries().iter().all(|e| e.kind == EntryKind::Image));
    }

    #[test]
    fn resolves_percent_encoded_names_and_skips_bad_references() {
        let opf = opf(
            r#"<item id="p" href="p.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<itemref idref="p"/>"#,
        );
        let html = r#"<body>
            <img src="img%201.png"/>
            <img src='img%201.png#frag'/>
            <img src="missing.png"/>
            <img src="http://example.com/x.png"/>
            <img src="data:image/png;base64,AAAA"/>
            <img src="../../../etc/evil.png"/>
            <img src="notes.txt"/>
            <img src="IMG2.PNG"/>
        </body>"#;
        let zip = open(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/p.xhtml", html.as_bytes()),
            ("OEBPS/img 1.png", b"1"),
            ("OEBPS/img2.png", b"2"),
            ("OEBPS/notes.txt", b"not an image"),
        ]);
        let c = EpubContainer::from_zip(zip).ok().expect("epub");
        // 重複は 1 回、外部・欠落・ZIP の外・画像でないものは無視、大文字小文字違いは許す
        assert_eq!(paths(&c), ["OEBPS/img 1.png", "OEBPS/img2.png"]);
    }

    #[test]
    fn cover_not_in_spine_goes_first() {
        let opf = opf(
            r#"<item id="p" href="p.xhtml" media-type="application/xhtml+xml"/>
               <item id="cov" href="cover.png" media-type="image/png" properties="cover-image"/>"#,
            r#"<itemref idref="p"/>"#,
        );
        let zip = open(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/p.xhtml", page("one.png").as_bytes()),
            ("OEBPS/one.png", b"1"),
            ("OEBPS/cover.png", b"C"),
        ]);
        let c = EpubContainer::from_zip(zip).ok().expect("epub");
        assert_eq!(paths(&c), ["OEBPS/cover.png", "OEBPS/one.png"]);
    }

    #[test]
    fn cover_already_in_spine_is_not_duplicated() {
        let opf = opf(
            r#"<item id="p" href="p.xhtml" media-type="application/xhtml+xml"/>
               <item id="cov" href="cover.png" media-type="image/png"/>"#,
            r#"<itemref idref="p"/>"#,
        )
        .replace(
            "</metadata>",
            r#"<meta name="cover" content="cov"/></metadata>"#,
        );
        let zip = open(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/p.xhtml", page("cover.png").as_bytes()),
            ("OEBPS/cover.png", b"C"),
        ]);
        let c = EpubContainer::from_zip(zip).ok().expect("epub");
        assert_eq!(paths(&c), ["OEBPS/cover.png"]);
    }

    #[test]
    fn image_items_directly_in_spine_are_pages() {
        let opf = opf(
            r#"<item id="i2" href="2.png" media-type="image/png"/><item id="i1" href="1.png" media-type="image/png"/>"#,
            r#"<itemref idref="i2"/><itemref idref="i1"/>"#,
        );
        let zip = open(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            ("OEBPS/1.png", b"1"),
            ("OEBPS/2.png", b"2"),
        ]);
        let c = EpubContainer::from_zip(zip).ok().expect("epub");
        assert_eq!(paths(&c), ["OEBPS/2.png", "OEBPS/1.png"]);
    }

    #[test]
    fn falls_back_to_plain_zip_when_not_a_readable_epub() {
        // container.xml が無い
        let zip = open(&[("a.png", b"a"), ("b.png", b"b")]);
        assert!(EpubContainer::from_zip(zip).is_err());
        // OPF が壊れている
        let zip = open(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", b"<package><unclosed>"),
            ("OEBPS/a.png", b"a"),
        ]);
        assert!(EpubContainer::from_zip(zip).is_err());
        // 画像が 1 枚も無い（テキスト主体）
        let opf = opf(
            r#"<item id="p" href="p.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<itemref idref="p"/>"#,
        );
        let zip = open(&[
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            (
                "OEBPS/p.xhtml",
                b"<html><body><p>text only</p></body></html>",
            ),
        ]);
        assert!(EpubContainer::from_zip(zip).is_err());
    }

    #[test]
    fn resolve_handles_dots_roots_and_escapes() {
        assert_eq!(resolve("a/b", "../c.png").as_deref(), Some("a/c.png"));
        assert_eq!(resolve("a/b", "./c.png").as_deref(), Some("a/b/c.png"));
        assert_eq!(resolve("a/b", "/c.png").as_deref(), Some("c.png"));
        assert_eq!(resolve("a", "../../x.png"), None);
        assert_eq!(resolve("", "x%2Fy%20z.png").as_deref(), Some("x/y z.png"));
        assert_eq!(resolve("", "https://h/x.png"), None);
        assert_eq!(resolve("", "#top"), None);
        assert_eq!(resolve("", "100%.png").as_deref(), Some("100%.png"));
    }
}
