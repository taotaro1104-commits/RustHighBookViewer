/// コンテナ内エントリの識別子（自然順ソート後の添字）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Image,
    /// 入れ子の書庫
    Archive,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryMeta {
    pub id: EntryId,
    /// 正規化済み（文字コード変換・パストラバーサル除去後）
    pub path: String,
    /// 展開後サイズ
    pub size: u64,
    pub kind: EntryKind,
}

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "webp", "gif", "bmp", "avif", "heic", "jxl",
];
const ARCHIVE_EXTS: &[&str] = &["zip", "cbz", "7z", "cb7", "rar", "cbr"];

pub fn kind_from_path(path: &str) -> EntryKind {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some(e) if IMAGE_EXTS.contains(&e) => EntryKind::Image,
        Some(e) if ARCHIVE_EXTS.contains(&e) => EntryKind::Archive,
        _ => EntryKind::Other,
    }
}

/// パスを自然順（大文字小文字無視）で比較する。
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    natord::compare_ignore_case(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_detection() {
        assert_eq!(kind_from_path("a/b.JPG"), EntryKind::Image);
        assert_eq!(kind_from_path("vol.cbz"), EntryKind::Archive);
        assert_eq!(kind_from_path("readme.txt"), EntryKind::Other);
        assert_eq!(kind_from_path("noext"), EntryKind::Other);
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["p10.png", "p2.png", "p1.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["p1.png", "p2.png", "p10.png"]);
    }
}
