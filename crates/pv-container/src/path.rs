use encoding_rs::{Encoding, UTF_8};

/// パスを正規化する。`..`・`.`・空要素・ドライブ指定・絶対パスを除去し `/` 区切りにする。
/// 有効な要素が残らなければ None。
pub fn normalize_path(raw: &str) -> Option<String> {
    let unified = raw.replace('\\', "/");
    let parts: Vec<&str> = unified
        .split('/')
        .filter(|p| !p.is_empty() && *p != "." && *p != "..")
        .filter(|p| !is_drive_spec(p))
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn is_drive_spec(part: &str) -> bool {
    let b = part.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// 生のファイル名バイト列を文字列にする。UTF-8 として妥当ならそのまま、
/// そうでなければ chardetng で判定して変換する（CP932 など）。
pub fn decode_name(raw: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(raw) {
        return s.to_owned();
    }
    let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    det.feed(raw, true);
    let enc: &'static Encoding = det.guess(None, chardetng::Utf8Detection::Allow);
    // UTF-8 と判定されても実際は不正バイト列なので、日本語圏の既定として Shift_JIS を使う
    let enc = if enc == UTF_8 {
        encoding_rs::SHIFT_JIS
    } else {
        enc
    };
    enc.decode_without_bom_handling(raw).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_traversal_and_absolute() {
        assert_eq!(
            normalize_path("../../etc/passwd").as_deref(),
            Some("etc/passwd")
        );
        assert_eq!(normalize_path("/abs/x.png").as_deref(), Some("abs/x.png"));
        assert_eq!(
            normalize_path("C:\\dir\\x.png").as_deref(),
            Some("dir/x.png")
        );
        assert_eq!(
            normalize_path("a//./b\\c.png").as_deref(),
            Some("a/b/c.png")
        );
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(normalize_path(".."), None);
        assert_eq!(normalize_path("/"), None);
        assert_eq!(normalize_path(""), None);
    }

    #[test]
    fn utf8_passthrough() {
        assert_eq!(decode_name("第01巻/001.jpg".as_bytes()), "第01巻/001.jpg");
    }

    #[test]
    fn cp932_names_are_decoded() {
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("漫画の本/ページ001.png");
        assert_eq!(decode_name(&bytes), "漫画の本/ページ001.png");
    }
}
