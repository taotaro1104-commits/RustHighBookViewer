//! 文章主体の EPUB を、ページに割り付ける（縦書きではなく横書き）。
//! XHTML から見出し・段落を取り出し、禁則処理つきで行に折り返し、ページに積む。
//! 文字の幅は呼び出し側（フォント）から受け取るので、この層は描画に依存しない。

use std::sync::OnceLock;

use regex::Regex;

/// 文章の 1 まとまり
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// 見出し（レベル 1〜6）
    Heading(u8, String),
    /// 段落。`indent` が true なら字下げする
    Para { text: String, indent: bool },
    /// 空行（`<p><br/></p>` など）
    Blank,
}

/// ページ内の 1 行
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub x: f32,
    /// 行の上端
    pub y: f32,
    pub size: f32,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub lines: Vec<Line>,
}

/// ページの寸法と文字サイズ（すべて画素）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSpec {
    pub width: f32,
    pub height: f32,
    pub margin_x: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub font_px: f32,
}

impl Default for PageSpec {
    fn default() -> Self {
        Self {
            width: 1080.0,
            height: 1620.0,
            margin_x: 96.0,
            margin_top: 120.0,
            margin_bottom: 140.0,
            font_px: 44.0,
        }
    }
}

/// 行頭に置かない文字（句読点・閉じ括弧・長音など）
const NO_START: &str = "、。，．,.！？!?）」』】〕〉》〗’”ー・：；:;…‥ゝゞヽヾ々)]}";
/// 行末に置かない文字（開き括弧）
const NO_END: &str = "（「『【〔〈《〖‘“([{";
/// 字下げしない書き出し（会話文など）
const NO_INDENT: &str = "「『（【〈《“‘　 ";

const LINE_GAP: f32 = 1.75;
const MAX_PAGES: usize = 20_000;

fn decode_entities(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE
        .get_or_init(|| Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-zA-Z]+);").expect("valid regex"));
    re.replace_all(s, |c: &regex::Captures| {
        let name = &c[1];
        let ch = if let Some(h) = name.strip_prefix("#x") {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = name.strip_prefix('#') {
            d.parse().ok().and_then(char::from_u32)
        } else {
            match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => None,
            }
        };
        ch.map_or_else(|| c[0].to_owned(), |c| c.to_string())
    })
    .into_owned()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Para,
    Heading(u8),
}

struct Parser {
    out: Vec<Block>,
    buf: String,
    kind: Kind,
    /// `<br>` のあとの続き（字下げしない）
    cont: bool,
}

impl Parser {
    fn flush(&mut self) {
        let text = self
            .buf
            .trim_matches(|c| c == ' ' || c == '\u{a0}')
            .to_owned();
        self.buf.clear();
        if text.is_empty() {
            return;
        }
        match self.kind {
            Kind::Heading(n) => self.out.push(Block::Heading(n, text)),
            Kind::Para => {
                let indent = !self.cont && !text.starts_with(|c: char| NO_INDENT.contains(c));
                self.out.push(Block::Para { text, indent });
            }
        }
    }
}

/// XHTML から本文のブロックを取り出す。ルビ（`<rt>`）・スクリプト・スタイルは読み飛ばす。
pub fn parse_xhtml(src: &str) -> Vec<Block> {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    let token =
        TOKEN.get_or_init(|| Regex::new(r"(?s)<!--.*?-->|<[^>]*>|[^<]+").expect("valid regex"));
    let mut p = Parser {
        out: Vec::new(),
        buf: String::new(),
        kind: Kind::Para,
        cont: false,
    };
    let mut in_body = !src.to_ascii_lowercase().contains("<body");
    let mut skip = 0usize;
    for m in token.find_iter(src) {
        let t = m.as_str();
        if !t.starts_with('<') {
            if in_body && skip == 0 {
                let text: String = t
                    .chars()
                    .filter(|c| !matches!(c, '\n' | '\r' | '\t'))
                    .collect();
                p.buf.push_str(&decode_entities(&text));
            }
            continue;
        }
        if t.starts_with("<!--") || t.starts_with("<?") || t.starts_with("<!") {
            continue;
        }
        let inner = t.trim_start_matches('<').trim_end_matches('>');
        let closing = inner.starts_with('/');
        let self_closing = inner.ends_with('/');
        let name = inner
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        match name.as_str() {
            "body" => in_body = !closing,
            "rt" | "rp" | "script" | "style" if !self_closing => {
                skip = if closing {
                    skip.saturating_sub(1)
                } else {
                    skip + 1
                };
            }
            "br" => {
                if in_body && skip == 0 {
                    if p.buf.trim().is_empty() {
                        p.buf.clear();
                        p.out.push(Block::Blank);
                    } else {
                        p.flush();
                        p.cont = true;
                    }
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                p.flush();
                p.cont = false;
                p.kind = if closing {
                    Kind::Para
                } else {
                    Kind::Heading(name.as_bytes()[1] - b'0')
                };
            }
            "p" | "div" | "li" | "blockquote" | "section" | "tr" => {
                p.flush();
                p.cont = false;
                p.kind = Kind::Para;
            }
            _ => {}
        }
    }
    p.flush();
    // 連続する空行は 1 つにまとめる
    let mut blocks: Vec<Block> = Vec::with_capacity(p.out.len());
    for b in p.out {
        if b == Block::Blank && blocks.last() == Some(&Block::Blank) {
            continue;
        }
        blocks.push(b);
    }
    blocks
}

/// 文字幅（`size` 画素のとき）を返す関数
pub type Advance<'a> = &'a dyn Fn(char, f32) -> f32;

/// 1 つの段落を行に折り返す。戻り値は (その行の字下げ, 文字列)。禁則: 行頭禁止文字はぶら下げ、開き括弧は次の行へ送る。
fn wrap(text: &str, first_indent: f32, width: f32, size: f32, adv: Advance) -> Vec<(f32, String)> {
    let mut lines = Vec::new();
    let mut indent = first_indent;
    let mut cur = String::new();
    let mut cur_w = 0.0f32;
    for c in text.chars() {
        let w = adv(c, size);
        if cur_w + w <= width - indent + 0.01 {
            cur.push(c);
            cur_w += w;
            continue;
        }
        if NO_START.contains(c) {
            cur.push(c); // ぶら下げ
            lines.push((indent, std::mem::take(&mut cur)));
            cur_w = 0.0;
        } else if cur.chars().last().is_some_and(|l| NO_END.contains(l)) && cur.chars().count() > 1
        {
            let moved = cur.pop().expect("non-empty");
            lines.push((indent, std::mem::take(&mut cur)));
            cur.push(moved);
            cur.push(c);
            cur_w = adv(moved, size) + w;
        } else {
            lines.push((indent, std::mem::take(&mut cur)));
            cur.push(c);
            cur_w = w;
        }
        indent = 0.0;
    }
    if !cur.is_empty() {
        lines.push((indent, cur));
    }
    lines
}

fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.5,
        2 => 1.25,
        3 => 1.1,
        _ => 1.05,
    }
}

struct Pager<'a> {
    spec: PageSpec,
    pages: &'a mut Vec<Page>,
    cur: Page,
    y: f32,
}

impl Pager<'_> {
    fn at_top(&self) -> bool {
        self.cur.lines.is_empty()
    }

    fn new_page(&mut self) {
        if !self.cur.lines.is_empty() && self.pages.len() < MAX_PAGES {
            self.pages.push(std::mem::take(&mut self.cur));
        }
        self.cur = Page::default();
        self.y = self.spec.margin_top;
    }

    fn put(&mut self, x: f32, size: f32, pitch: f32, text: String) {
        if self.y + pitch > self.spec.height - self.spec.margin_bottom && !self.at_top() {
            self.new_page();
        }
        self.cur.lines.push(Line {
            x,
            y: self.y,
            size,
            text,
        });
        self.y += pitch;
    }
}

/// 章ごとのブロックを、ページに割り付ける。章の初めは必ず新しいページから始める。
pub fn paginate(chapters: &[Vec<Block>], spec: PageSpec, adv: Advance) -> Vec<Page> {
    let mut pages = Vec::new();
    let text_w = spec.width - spec.margin_x * 2.0;
    let mut pg = Pager {
        spec,
        pages: &mut pages,
        cur: Page::default(),
        y: spec.margin_top,
    };
    for blocks in chapters {
        pg.new_page();
        for b in blocks {
            match b {
                Block::Blank => {
                    if !pg.at_top() {
                        pg.y += spec.font_px * LINE_GAP * 0.6;
                    }
                }
                Block::Heading(level, text) => {
                    let size = spec.font_px * heading_scale(*level);
                    let pitch = size * 1.5;
                    if !pg.at_top() {
                        pg.y += pitch * 0.6;
                    }
                    for (_, line) in wrap(text, 0.0, text_w, size, adv) {
                        pg.put(spec.margin_x, size, pitch, line);
                    }
                    pg.y += pitch * 0.3;
                }
                Block::Para { text, indent } => {
                    let size = spec.font_px;
                    let pitch = size * LINE_GAP;
                    let first = if *indent { size } else { 0.0 };
                    for (ind, line) in wrap(text, first, text_w, size, adv) {
                        pg.put(spec.margin_x + ind, size, pitch, line);
                    }
                }
            }
        }
    }
    pg.new_page();
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全角は文字サイズ、半角（ASCII）は半分の幅
    fn adv(c: char, size: f32) -> f32 {
        if c.is_ascii() {
            size / 2.0
        } else {
            size
        }
    }

    #[test]
    fn extracts_headings_paragraphs_and_blank_lines() {
        let x = r#"<?xml version="1.0"?><!DOCTYPE html><html><head><title>題</title><style>p{}</style></head>
<body><h2>第2話 &amp; 始まり</h2>
<p>メシャアアアアッ！！！</p>
<p><br/></p>
<p><br/></p>
<p>「会話」です。</p>
<p>漢字<ruby>魔法<rt>まほう</rt></ruby>。&#x3042;&nbsp;</p></body></html>"#;
        assert_eq!(
            parse_xhtml(x),
            vec![
                Block::Heading(2, "第2話 & 始まり".into()),
                Block::Para {
                    text: "メシャアアアアッ！！！".into(),
                    indent: true
                },
                Block::Blank,
                Block::Para {
                    text: "「会話」です。".into(),
                    indent: false
                },
                Block::Para {
                    text: "漢字魔法。あ".into(),
                    indent: true
                },
            ]
        );
    }

    #[test]
    fn br_inside_a_paragraph_continues_without_indent() {
        let b = parse_xhtml("<body><p>一行目<br/>二行目</p></body>");
        assert_eq!(
            b,
            vec![
                Block::Para {
                    text: "一行目".into(),
                    indent: true
                },
                Block::Para {
                    text: "二行目".into(),
                    indent: false
                },
            ]
        );
    }

    #[test]
    fn wrap_hangs_closing_punctuation_and_moves_opening_brackets() {
        // 幅は 5 字分
        let w = 50.0;
        // 5 字ちょうどのあとの「。」はぶら下げる
        let l = wrap("あいうえお。かき", 0.0, w, 10.0, &adv);
        assert_eq!(
            l.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
            ["あいうえお。", "かき"]
        );
        // 行末に来る「は次の行へ
        let l = wrap("あいうえ「お", 0.0, w, 10.0, &adv);
        assert_eq!(
            l.iter().map(|x| x.1.as_str()).collect::<Vec<_>>(),
            ["あいうえ", "「お"]
        );
        // 字下げは最初の行だけ
        let l = wrap("あいうえおかきくけこ", 10.0, w, 10.0, &adv);
        assert_eq!(l[0], (10.0, "あいうえ".to_owned()));
        assert_eq!(l[1].0, 0.0);
    }

    #[test]
    fn chapters_start_on_new_pages_and_long_chapters_span_pages() {
        let spec = PageSpec {
            width: 200.0,
            height: 200.0,
            margin_x: 20.0,
            margin_top: 20.0,
            margin_bottom: 20.0,
            font_px: 20.0,
        };
        let long: Vec<Block> = (0..30)
            .map(|i| Block::Para {
                text: format!("段落{i}あいうえおかきくけこ"),
                indent: true,
            })
            .collect();
        let short = vec![Block::Heading(1, "終わり".into())];
        let pages = paginate(&[long, short], spec, &adv);
        assert!(pages.len() >= 3);
        assert_eq!(pages.last().unwrap().lines[0].text, "終わり");
        // どのページも下余白を越えない
        for p in &pages {
            for l in &p.lines {
                assert!(
                    l.y + l.size <= spec.height - spec.margin_bottom + 0.01,
                    "{l:?}"
                );
            }
        }
        assert!(paginate(&[vec![]], spec, &adv).is_empty());
    }
}
