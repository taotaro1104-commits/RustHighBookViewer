//! 取り込んだフォントの保管庫。アプリ専用ディレクトリ（`set_font_dir`）に
//! `"{crc32:08x}-{名前}.{ttf|otf|ttc}"` というファイル名で置く。ID は拡張子を除いたファイル名。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{ContainerError, Result};
use crate::font::forget_custom;

/// 取り込めるファイルの上限
pub const MAX_FONT_BYTES: u64 = 64 << 20;
/// 名前（ファイル名の拡張子を除いた部分）の最大バイト数。
/// 文字数で切ると 4 バイト文字ばかりの名前が 255 バイトの上限を超えうる
const MAX_NAME_BYTES: usize = 64;
const EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];

/// 取り込んだフォント 1 つ分の情報
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontInfo {
    pub id: String,
    /// 表示用の名前
    pub name: String,
}

static FONT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// 取り込み先を決める（ディレクトリは取り込むときに作る）。
pub fn set_font_dir(dir: PathBuf) {
    if let Ok(mut d) = FONT_DIR.lock() {
        *d = Some(dir);
    }
    forget_custom(None);
}

fn font_dir() -> Option<PathBuf> {
    FONT_DIR.lock().ok()?.clone()
}

fn invalid(msg: &str) -> ContainerError {
    ContainerError::InvalidFont(msg.to_string())
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// ID として有効な文字だけか（パスの区切りなどを含まない）。
fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(is_name_char)
}

fn ext_of(file: &str) -> Option<String> {
    let ext = file.rsplit_once('.')?.1.to_ascii_lowercase();
    EXTENSIONS.contains(&ext.as_str()).then_some(ext)
}

/// 表示名: ID の先頭の `xxxxxxxx-` を除いたもの。
fn info_of(id: &str) -> FontInfo {
    FontInfo {
        id: id.to_string(),
        name: id.split_once('-').map_or(id, |(_, n)| n).to_string(),
    }
}

/// ID に対応するファイル（実在するもの）。ID が不正なら None。
pub(crate) fn path_for_id(id: &str) -> Option<PathBuf> {
    if !is_valid_id(id) {
        return None;
    }
    let dir = font_dir()?;
    EXTENSIONS
        .iter()
        .map(|e| dir.join(format!("{id}.{e}")))
        .find(|p| p.is_file())
}

pub(crate) fn has_custom(id: &str) -> bool {
    path_for_id(id).is_some()
}

/// フォントファイルの中身を検証して取り込む。同じ内容・同じ名前なら同じ ID を返す。
pub fn import_font(file_name: &str, data: &[u8]) -> Result<FontInfo> {
    // 区切り文字（/ と \）より後ろだけを名前として使う
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or("");
    let ext =
        ext_of(base).ok_or_else(|| invalid("unsupported file type (ttf / otf / ttc only)"))?;
    if data.len() as u64 > MAX_FONT_BYTES {
        return Err(invalid("file is too large"));
    }
    // 検証だけなので、コピーせずに借りたまま読む
    ab_glyph::FontRef::try_from_slice_and_index(data, 0)
        .map_err(|_| invalid("not a readable font file"))?;
    let dir = font_dir().ok_or_else(|| invalid("font directory is not set"))?;

    let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
    let mut name = String::new();
    for c in stem.chars().map(|c| if is_name_char(c) { c } else { '_' }) {
        if name.len() + c.len_utf8() > MAX_NAME_BYTES {
            break;
        }
        name.push(c);
    }
    if name.is_empty() {
        name = "font".to_string();
    }
    let prefix = format!("{:08x}-", crc32fast::hash(data));
    // 同じ中身は、名前や拡張子が違っても 1 つにまとめる（一覧に重複して出ない）
    if let Some(same) = list_fonts()
        .into_iter()
        .filter(|f| f.id.starts_with(&prefix))
        .find(|f| path_for_id(&f.id).and_then(|p| fs::read(p).ok()).as_deref() == Some(data))
    {
        return Ok(same);
    }
    let id = format!("{prefix}{name}");
    fs::create_dir_all(&dir)?;
    write_atomically(&dir.join(format!("{id}.{ext}")), data)?;
    forget_custom(Some(&id));
    Ok(info_of(&id))
}

/// 一時ファイルに書いてから名前を付け替える（途中で失敗しても半端なファイルを残さない）。
fn write_atomically(dest: &Path, data: &[u8]) -> Result<()> {
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = fs::write(&tmp, data).and_then(|()| fs::rename(&tmp, dest));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

/// 取り込み済みのフォント（名前順）。
pub fn list_fonts() -> Vec<FontInfo> {
    let Some(dir) = font_dir() else {
        return Vec::new();
    };
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<FontInfo> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            ext_of(path.file_name()?.to_str()?)?;
            let id = path.file_stem()?.to_str()?;
            is_valid_id(id).then(|| info_of(id))
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    v
}

/// 取り込んだフォントを消す。無い ID は何もしない。
pub fn remove_font(id: &str) -> Result<()> {
    // 拡張子違いが残っていても取りこぼさない
    while let Some(path) = path_for_id(id) {
        fs::remove_file(path)?;
    }
    forget_custom(Some(id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex, MutexGuard};

    use super::*;
    use crate::error::ContainerError;
    use crate::font::{font_for, font_set_for, FontSet};
    use crate::text_style::Typeface;

    /// フォントの置き場はプロセス全体で 1 つなので、テストを直列にする。
    static LOCK: Mutex<()> = Mutex::new(());

    struct Env {
        _guard: MutexGuard<'static, ()>,
        dir: tempfile::TempDir,
    }

    fn env() -> Env {
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        set_font_dir(dir.path().join("fonts"));
        Env { _guard: guard, dir }
    }

    /// 端末にある日本語フォントの中身。無ければ None（テストはスキップ）。
    fn test_font() -> Option<Vec<u8>> {
        let bytes =
            crate::font::first_existing_candidate(&Typeface::Gothic).and_then(|p| fs::read(p).ok());
        if bytes.is_none() {
            eprintln!("skipped: no CJK font on this machine");
        }
        bytes
    }

    fn files_in_font_dir(e: &Env) -> Vec<String> {
        fs::read_dir(e.dir.path().join("fonts"))
            .map(|rd| {
                rd.flatten()
                    .map(|f| f.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn import_then_list_then_remove() {
        let e = env();
        let Some(data) = test_font() else { return };
        let info = import_font("My Font.ttf", &data).unwrap();
        assert!(info.id.len() > 9 && info.id.as_bytes()[8] == b'-');
        assert!(info.id[..8].chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(info.name, "My_Font");
        assert_eq!(list_fonts(), vec![info.clone()]);
        remove_font(&info.id).unwrap();
        assert!(list_fonts().is_empty());
        assert!(files_in_font_dir(&e).is_empty());
        remove_font(&info.id).unwrap(); // 無い ID は成功扱い
    }

    #[test]
    fn import_same_file_twice_is_one_font() {
        let e = env();
        let Some(data) = test_font() else { return };
        let a = import_font("a.ttf", &data).unwrap();
        let b = import_font("a.ttf", &data).unwrap();
        assert_eq!(a, b);
        assert_eq!(list_fonts().len(), 1);
        assert_eq!(files_in_font_dir(&e).len(), 1);
    }

    #[test]
    fn rejects_non_font_bytes() {
        let e = env();
        for bytes in [&b"not a font"[..], &b""[..]] {
            assert!(matches!(
                import_font("a.ttf", bytes),
                Err(ContainerError::InvalidFont(_))
            ));
        }
        assert!(files_in_font_dir(&e).is_empty());
    }

    #[test]
    fn rejects_unsupported_extension_but_accepts_uppercase() {
        let _e = env();
        let Some(data) = test_font() else { return };
        assert!(matches!(
            import_font("a.png", &data),
            Err(ContainerError::InvalidFont(_))
        ));
        assert!(matches!(
            import_font("noext", &data),
            Err(ContainerError::InvalidFont(_))
        ));
        assert!(import_font("A.TTF", &data).is_ok());
    }

    #[test]
    fn rejects_oversized_input() {
        let e = env();
        let big = vec![0u8; MAX_FONT_BYTES as usize + 1];
        let err = import_font("big.ttf", &big).unwrap_err();
        assert!(matches!(&err, ContainerError::InvalidFont(m) if m.contains("large")));
        assert!(files_in_font_dir(&e).is_empty());
    }

    #[test]
    fn sanitizes_file_name() {
        let e = env();
        let Some(data) = test_font() else { return };
        let info = import_font(r"../../evil/..\x:y*.ttf", &data).unwrap();
        assert!(info
            .id
            .chars()
            .all(|c| c.is_alphanumeric() || "._-".contains(c)));
        assert_eq!(files_in_font_dir(&e).len(), 1);
        assert!(e
            .dir
            .path()
            .join("fonts")
            .join(format!("{}.ttf", info.id))
            .is_file());

        // 同じ中身は 1 つにまとまるので、名前の検査ごとに前のを消す
        remove_font(&info.id).unwrap();
        let long = format!("{}.ttf", "あ".repeat(300));
        let info = import_font(&long, &data).unwrap();
        assert_eq!(info.name.len(), 63, "64 バイト以内に収まる最大の文字数");

        // 4 バイト文字ばかりでも、ファイル名が 255 バイトを超えない
        remove_font(&info.id).unwrap();
        let wide = format!(
            "{}.ttf",
            char::from_u32(0x20BB7).unwrap().to_string().repeat(100)
        );
        let info = import_font(&wide, &data).unwrap();
        assert!(info.name.len() <= 64 && !info.name.is_empty());
        for f in files_in_font_dir(&e) {
            assert!(f.len() <= 255, "{f}");
        }
    }

    #[test]
    fn same_bytes_under_other_name_or_extension_is_one_font() {
        let e = env();
        let Some(data) = test_font() else { return };
        let a = import_font("a.ttf", &data).unwrap();
        let b = import_font("b.otf", &data).unwrap();
        let c = import_font("A.TTF", &data).unwrap();
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(list_fonts(), vec![a.clone()]);
        assert_eq!(files_in_font_dir(&e).len(), 1);
        // 削除すると一覧からも消える
        remove_font(&a.id).unwrap();
        assert!(list_fonts().is_empty());
    }

    /// 取り込み・削除と読み込みが同時に走っても、最後に取り込んだフォントが使われる
    /// （ファイルが無かった間のゴシック体が、フォントセットの覚えに残らない）
    #[test]
    fn concurrent_import_never_leaves_stale_font_set() {
        let _e = env();
        let Some(data) = test_font() else { return };
        let info = import_font("a.ttf", &data).unwrap();
        let t = Typeface::Custom(info.id.clone());
        let stop = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = font_set_for(&t);
                }
            });
            for _ in 0..40 {
                remove_font(&info.id).unwrap();
                import_font("a.ttf", &data).unwrap();
            }
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let set = font_set_for(&t).unwrap();
        let custom = font_for(&t).unwrap();
        let gothic = font_for(&Typeface::Gothic).unwrap();
        assert!(!Arc::ptr_eq(&custom, &gothic));
        assert!(
            std::ptr::eq(set.primary(), &*custom),
            "古いフォントセットが残っている"
        );
    }

    #[test]
    fn missing_custom_falls_back_to_gothic() {
        let _e = env();
        let Some(_) = test_font() else { return };
        let missing = Typeface::Custom("zzzzzzzz-none".into());
        assert!(Arc::ptr_eq(
            &font_for(&missing).unwrap(),
            &font_for(&Typeface::Gothic).unwrap()
        ));
    }

    #[test]
    fn remove_clears_cached_font() {
        let _e = env();
        let Some(data) = test_font() else { return };
        let info = import_font("a.ttf", &data).unwrap();
        let t = Typeface::Custom(info.id.clone());
        let custom = font_for(&t).unwrap();
        let gothic = font_for(&Typeface::Gothic).unwrap();
        assert!(!Arc::ptr_eq(&custom, &gothic));
        remove_font(&info.id).unwrap();
        assert!(Arc::ptr_eq(&font_for(&t).unwrap(), &gothic));
        assert!(!has_custom(&info.id));
    }

    #[test]
    fn custom_font_set_uses_gothic_for_missing_glyphs() {
        let _e = env();
        let Some(data) = test_font() else { return };
        let info = import_font("a.ttf", &data).unwrap();
        // 主フォントに字形が無い文字は、先頭の補助フォント（ゴシック）から探す
        let set = font_set_for(&Typeface::Custom(info.id)).unwrap();
        assert!(set.find('あ').is_some());
        let gothic = font_for(&Typeface::Gothic).unwrap();
        let only_first = FontSet::with_fallbacks(gothic.clone(), vec![gothic]);
        assert!(only_first.find('あ').is_some());
    }
}
