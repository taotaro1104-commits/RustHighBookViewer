//! 文章の EPUB を描くための日本語フォント。端末に入っているものを探して使う
//! （CJK フォントは数 MB〜数十 MB あり、アプリに同梱すると大きくなりすぎるため）。

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use ab_glyph::FontVec;

/// よくあるインストール先。先に見つかったものを使う。
fn candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        // Android（Noto Sans CJK は先頭のフェイスが日本語）
        "/system/fonts/NotoSansCJK-Regular.ttc",
        "/system/fonts/NotoSerifCJK-Regular.ttc",
        "/system/fonts/NotoSansJP-Regular.otf",
        "/system/fonts/NotoSansCJKjp-Regular.otf",
        "/system/fonts/SourceHanSansJP-Regular.otf",
        "/system/fonts/DroidSansJapanese.ttf",
        // Windows
        "C:/Windows/Fonts/YuGothM.ttc",
        "C:/Windows/Fonts/meiryo.ttc",
        "C:/Windows/Fonts/msgothic.ttc",
        "C:/Windows/Fonts/yumin.ttf",
        // macOS
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    // 環境変数での指定を最優先にする
    if let Some(p) = std::env::var_os("PV_TEXT_FONT") {
        v.insert(0, PathBuf::from(p));
    }
    // Android: 上の名前に無い場合は /system/fonts から CJK / JP を含む名前を探す
    if let Ok(rd) = fs::read_dir("/system/fonts") {
        let mut extra: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n.contains("CJK") || n.contains("JP") || n.contains("Japanese")
                })
            })
            .collect();
        extra.sort();
        v.extend(extra);
    }
    v
}

fn load() -> Option<Arc<FontVec>> {
    for path in candidates() {
        let Ok(data) = fs::read(&path) else { continue };
        // TTC は先頭のフェイス（日本語）を使う。Noto CJK のような CFF 形式の輪郭も描ける
        if let Ok(font) = FontVec::try_from_vec_and_index(data, 0) {
            return Some(Arc::new(font));
        }
    }
    None
}

/// 日本語を描けるフォント。見つからなければ None（見つからなかった結果も覚える）。
pub fn shared_font() -> Option<Arc<FontVec>> {
    static FONT: OnceLock<Option<Arc<FontVec>>> = OnceLock::new();
    FONT.get_or_init(load).clone()
}
