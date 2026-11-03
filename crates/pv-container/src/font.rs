//! 文章の EPUB を描くための日本語フォント。端末に入っているものを探して使う
//! （CJK フォントは数 MB〜数十 MB あり、アプリに同梱すると大きくなりすぎるため）。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use ab_glyph::{Font, FontVec};

use crate::text_style::Typeface;

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

/// よくあるインストール先。先に見つかったものを使う。
fn candidates(typeface: Typeface) -> Vec<PathBuf> {
    let mut v = match typeface {
        Typeface::Gothic => paths(&[
            // Android（Noto Sans CJK は先頭のフェイスが日本語）
            "/system/fonts/NotoSansCJK-Regular.ttc",
            "/system/fonts/NotoSansJP-Regular.otf",
            "/system/fonts/NotoSansCJKjp-Regular.otf",
            "/system/fonts/SourceHanSansJP-Regular.otf",
            "/system/fonts/DroidSansJapanese.ttf",
            // Windows
            "C:/Windows/Fonts/YuGothM.ttc",
            "C:/Windows/Fonts/meiryo.ttc",
            "C:/Windows/Fonts/msgothic.ttc",
            // macOS
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
            // Linux
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
        ]),
        Typeface::Mincho => paths(&[
            // Android
            "/system/fonts/NotoSerifCJK-Regular.ttc",
            "/system/fonts/NotoSerifJP-Regular.otf",
            "/system/fonts/SourceHanSerifJP-Regular.otf",
            // Windows
            "C:/Windows/Fonts/yumin.ttf",
            "C:/Windows/Fonts/msmincho.ttc",
            // macOS
            "/System/Library/Fonts/ヒラギノ明朝 ProN.ttc",
            // Linux
            "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc",
            "/usr/share/fonts/truetype/fonts-japanese-mincho.ttf",
        ]),
    };
    if typeface == Typeface::Gothic {
        // 環境変数での指定を最優先にする（動作確認用）
        if let Some(p) = std::env::var_os("PV_TEXT_FONT") {
            v.insert(0, PathBuf::from(p));
        }
    }
    // Android: 上の名前に無い場合は /system/fonts から名前で探す
    if let Ok(rd) = fs::read_dir("/system/fonts") {
        let (any, none): (&[&str], &[&str]) = match typeface {
            Typeface::Gothic => (&["CJK", "JP", "Japanese"], &["Serif", "Mincho"]),
            Typeface::Mincho => (&["SerifCJK", "SerifJP", "Mincho"], &[]),
        };
        let mut extra: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    any.iter().any(|k| n.contains(k)) && !none.iter().any(|k| n.contains(k))
                })
            })
            .collect();
        extra.sort();
        v.extend(extra);
    }
    v
}

fn load(typeface: Typeface) -> Option<Arc<FontVec>> {
    for path in candidates(typeface) {
        let Ok(data) = fs::read(&path) else { continue };
        // TTC は先頭のフェイス（日本語）を使う。Noto CJK のような CFF 形式の輪郭も描ける
        if let Ok(font) = FontVec::try_from_vec_and_index(data, 0) {
            return Some(Arc::new(font));
        }
    }
    None
}

/// 記号・絵文字などの補助フォントの候補。主フォント（日本語）に字形が無い文字のために使う。
fn fallback_candidates() -> Vec<PathBuf> {
    let mut v = paths(&[
        // Android
        "/system/fonts/NotoSansSymbols-Regular-Subsetted.ttf",
        "/system/fonts/NotoSansSymbols2-Regular.ttf",
        "/system/fonts/NotoSansSymbols-Regular.ttf",
        "/system/fonts/NotoEmoji-Regular.ttf",
        "/system/fonts/NotoSansMath-Regular.ttf",
        "/system/fonts/DroidSansFallback.ttf",
        // Windows
        "C:/Windows/Fonts/seguisym.ttf",
        "C:/Windows/Fonts/seguiemj.ttf",
        "C:/Windows/Fonts/arialuni.ttf",
        // macOS
        "/System/Library/Fonts/Apple Symbols.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        // Linux
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    ]);
    // Android: 上に無い名前の記号フォントも拾う（色付き絵文字は輪郭を持たないので除く）
    if let Ok(rd) = fs::read_dir("/system/fonts") {
        let mut extra: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    ["Symbols", "Emoji", "Math"].iter().any(|k| n.contains(k))
                        && !n.contains("Color")
                })
            })
            .collect();
        extra.sort();
        v.extend(extra);
    }
    v
}

fn load_fallbacks() -> Vec<Arc<FontVec>> {
    fallback_candidates()
        .into_iter()
        .filter_map(|p| {
            let data = fs::read(p).ok()?;
            FontVec::try_from_vec_and_index(data, 0).ok().map(Arc::new)
        })
        .collect()
}

/// 主フォントと、記号などのための補助フォント。字形は主フォント → 補助フォントの順に探す。
/// 補助フォントは、主フォントに無い文字が出てきたときに初めて読み込む。
pub struct FontSet {
    primary: Arc<FontVec>,
    fallbacks: OnceLock<Vec<Arc<FontVec>>>,
}

fn has_glyph(f: &FontVec, c: char) -> bool {
    f.glyph_id(c).0 != 0
}

impl FontSet {
    pub fn new(primary: Arc<FontVec>) -> Self {
        Self {
            primary,
            fallbacks: OnceLock::new(),
        }
    }

    /// 主フォント（行の高さなどの基準）。
    pub fn primary(&self) -> &FontVec {
        &self.primary
    }

    /// `c` の字形を持つフォント。どれにも無ければ `None`。
    pub fn find(&self, c: char) -> Option<&FontVec> {
        if has_glyph(&self.primary, c) {
            return Some(&self.primary);
        }
        self.fallbacks
            .get_or_init(load_fallbacks)
            .iter()
            .map(|f| &**f)
            .find(|f| has_glyph(f, c))
    }
}

/// その書体のフォントが端末にあるか（読み込みはしない）。
pub fn has_typeface(typeface: Typeface) -> bool {
    candidates(typeface).iter().any(|p| p.is_file())
}

/// その書体のフォント。明朝体が端末に無いときはゴシック体で代用する。どちらも無ければ None。
/// 見つからなかった結果も覚える（読み込みは書体ごとに 1 回だけ）。
pub fn font_for(typeface: Typeface) -> Option<Arc<FontVec>> {
    static CACHE: OnceLock<Mutex<HashMap<Typeface, Option<Arc<FontVec>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let get = |t: Typeface| {
        let mut map = cache.lock().ok()?;
        map.entry(t).or_insert_with(|| load(t)).clone()
    };
    get(typeface).or_else(|| {
        (typeface != Typeface::Gothic)
            .then(|| get(Typeface::Gothic))
            .flatten()
    })
}

/// その書体のフォントセット（`font_for` と同じ主フォントに、補助フォントを添える）。書体ごとに 1 つを使い回す。
pub fn font_set_for(typeface: Typeface) -> Option<Arc<FontSet>> {
    static CACHE: OnceLock<Mutex<HashMap<Typeface, Arc<FontSet>>>> = OnceLock::new();
    let primary = font_for(typeface)?;
    let mut map = CACHE.get_or_init(Mutex::default).lock().ok()?;
    Some(Arc::clone(
        map.entry(typeface)
            .or_insert_with(|| Arc::new(FontSet::new(primary))),
    ))
}
