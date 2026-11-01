//! 利用者に見せるメッセージの言語（日本語・英語・中国語（簡体）・韓国語・スペイン語）。
//! 画面の文言は Kotlin 側にあり、ここは Rust から返すエラーの説明だけを扱う。

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
    Zh,
    Ko,
    Es,
}

impl Lang {
    /// "ja" / "en" / "zh" / "ko" / "es"（地域付き "zh-CN" なども可）。分からなければ日本語。
    pub fn from_code(code: &str) -> Self {
        match code.split(['-', '_']).next().unwrap_or("") {
            "en" => Self::En,
            "zh" => Self::Zh,
            "ko" => Self::Ko,
            "es" => Self::Es,
            _ => Self::Ja,
        }
    }

    fn id(self) -> u8 {
        self as u8
    }

    fn from_id(id: u8) -> Self {
        [Self::Ja, Self::En, Self::Zh, Self::Ko, Self::Es]
            .get(id as usize)
            .copied()
            .unwrap_or(Self::Ja)
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    CURRENT.store(lang.id(), Ordering::Relaxed);
}

pub fn lang() -> Lang {
    Lang::from_id(CURRENT.load(Ordering::Relaxed))
}

/// 5 言語の文言から、`lang` のものを選ぶ。`{}` は `detail` に置き換える（無ければそのまま）。
pub fn pick(lang: Lang, texts: [&str; 5], detail: &str) -> String {
    texts[lang.id() as usize].replacen("{}", detail, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_map_to_languages_and_default_to_japanese() {
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("zh-CN"), Lang::Zh);
        assert_eq!(Lang::from_code("ko_KR"), Lang::Ko);
        assert_eq!(Lang::from_code("es"), Lang::Es);
        assert_eq!(Lang::from_code("fr"), Lang::Ja);
        assert_eq!(Lang::from_code(""), Lang::Ja);
    }

    #[test]
    fn pick_chooses_the_language_and_fills_the_detail() {
        let t = ["a{}", "b{}", "c{}", "d{}", "e{}"];
        assert_eq!(pick(Lang::En, t, "X"), "bX");
        assert_eq!(pick(Lang::Es, t, "Y"), "eY");
    }
}
