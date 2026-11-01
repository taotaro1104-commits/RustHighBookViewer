//! 文章の EPUB を描くための日本語フォント。端末に入っているものを探して使う
//! （CJK フォントは数 MB〜数十 MB あり、アプリに同梱すると大きくなりすぎるため）。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use ab_glyph::FontVec;

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
