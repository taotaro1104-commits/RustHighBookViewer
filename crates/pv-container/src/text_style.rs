//! 文章の EPUB の見た目（文字の大きさ・書体）。
//! 変えるとページの割り付けが変わるので、本を開き直したときに反映される。
//! `open_source_styled` で開くときに渡す。

use serde::{Deserialize, Serialize};

pub const MIN_FONT_PX: u32 = 28;
pub const MAX_FONT_PX: u32 = 160;
pub const DEFAULT_FONT_PX: u32 = 44;
/// 行間（文字の大きさに対する割合、%）。100 で行どうしが接する
pub const MIN_LINE_SPACING: u32 = 100;
pub const MAX_LINE_SPACING: u32 = 240;
pub const DEFAULT_LINE_SPACING: u32 = 160;
/// 文字間（文字の大きさに対する追加の割合、%）。0 で詰めなし、負で詰める
pub const MIN_LETTER_SPACING: i32 = -10;
pub const MAX_LETTER_SPACING: i32 = 60;
/// 文字色・背景色の既定（`0xRRGGBB`）
pub const DEFAULT_INK: u32 = 0x1C1C1C;
pub const DEFAULT_PAPER: u32 = 0xF5F5F5;

/// 書体の種類。設定ファイルでは文字列（`"gothic"` / `"mincho"` / `"custom:<フォント ID>"`）で持つ。
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum Typeface {
    /// ゴシック体（既定）
    #[default]
    Gothic,
    /// 明朝体
    Mincho,
    /// 取り込んだフォント（ID は `import_font` が返したもの）
    Custom(String),
}

impl Serialize for Typeface {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Gothic => s.serialize_str("gothic"),
            Self::Mincho => s.serialize_str("mincho"),
            Self::Custom(id) => s.serialize_str(&format!("custom:{id}")),
        }
    }
}

impl<'de> Deserialize<'de> for Typeface {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "gothic" => Ok(Self::Gothic),
            "mincho" => Ok(Self::Mincho),
            _ => match s.strip_prefix("custom:") {
                Some(id) if !id.is_empty() => Ok(Self::Custom(id.to_string())),
                _ => Err(serde::de::Error::invalid_value(
                    serde::de::Unexpected::Str(&s),
                    &"\"gothic\", \"mincho\" or \"custom:<id>\"",
                )),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    /// 本文の文字の大きさ（ページ画像上の画素。ページの幅は 1080）
    pub font_px: u32,
    pub typeface: Typeface,
    /// 行の送り幅（文字の大きさに対する割合、%）
    pub line_spacing: u32,
    /// 文字の色（`0xRRGGBB`）
    pub ink: u32,
    /// 背景（紙）の色（`0xRRGGBB`）
    pub paper: u32,
    /// 文字間（文字の大きさに対する追加の割合、%）
    pub letter_spacing: i32,
    /// 太字（フォントに太字が無いため、輪郭を太らせて描く）
    pub bold: bool,
    /// 斜体（フォントに斜体が無いため、傾けて描く）
    pub italic: bool,
    /// 縦書き（false なら横書き）
    pub vertical: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_px: DEFAULT_FONT_PX,
            typeface: Typeface::Gothic,
            line_spacing: DEFAULT_LINE_SPACING,
            ink: DEFAULT_INK,
            paper: DEFAULT_PAPER,
            letter_spacing: 0,
            bold: false,
            italic: false,
            vertical: true,
        }
    }
}

impl TextStyle {
    /// 範囲外の値を収める（UI や設定ファイルの不正値対策）。
    pub fn clamped(self) -> Self {
        Self {
            font_px: self.font_px.clamp(MIN_FONT_PX, MAX_FONT_PX),
            line_spacing: self.line_spacing.clamp(MIN_LINE_SPACING, MAX_LINE_SPACING),
            ink: self.ink & 0xFF_FFFF,
            paper: self.paper & 0xFF_FFFF,
            letter_spacing: self
                .letter_spacing
                .clamp(MIN_LETTER_SPACING, MAX_LETTER_SPACING),
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_range_sizes_are_clamped() {
        let s = |px| {
            TextStyle {
                font_px: px,
                typeface: Typeface::Mincho,
                line_spacing: 160,
                ink: 0,
                paper: 0,
                ..TextStyle::default()
            }
            .clamped()
            .font_px
        };
        assert_eq!((s(0), s(44), s(10_000)), (MIN_FONT_PX, 44, MAX_FONT_PX));
        assert_eq!(MAX_FONT_PX, 160);
    }

    #[test]
    fn line_spacing_is_clamped() {
        let l = |v| {
            TextStyle {
                line_spacing: v,
                ..TextStyle::default()
            }
            .clamped()
            .line_spacing
        };
        assert_eq!(
            (l(0), l(180), l(9999)),
            (MIN_LINE_SPACING, 180, MAX_LINE_SPACING)
        );
    }

    #[test]
    fn style_roundtrips_through_toml_like_text() {
        let json = serde_json::json!({"font_px": 52, "typeface": "mincho"});
        let s: TextStyle = serde_json::from_value(json).unwrap();
        assert_eq!(
            s,
            TextStyle {
                font_px: 52,
                typeface: Typeface::Mincho,
                line_spacing: DEFAULT_LINE_SPACING,
                ink: DEFAULT_INK,
                paper: DEFAULT_PAPER,
                letter_spacing: 0,
                bold: false,
                italic: false,
                vertical: true
            }
        );
        // 欠けている項目は既定値
        let d: TextStyle = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(d, TextStyle::default());
    }

    #[test]
    fn typeface_round_trips_as_string() {
        let custom = Typeface::Custom("0a1b2c3d-Foo".into());
        assert_eq!(
            serde_json::to_value(&custom).unwrap(),
            serde_json::json!("custom:0a1b2c3d-Foo")
        );
        assert_eq!(
            serde_json::to_value(Typeface::Gothic).unwrap(),
            serde_json::json!("gothic")
        );
        assert_eq!(
            serde_json::to_value(Typeface::Mincho).unwrap(),
            serde_json::json!("mincho")
        );
        for t in [Typeface::Gothic, Typeface::Mincho, custom] {
            let v = serde_json::to_value(&t).unwrap();
            assert_eq!(serde_json::from_value::<Typeface>(v).unwrap(), t);
        }
    }

    #[test]
    fn unknown_or_empty_custom_is_rejected() {
        for bad in ["custom:", "cursive", ""] {
            assert!(serde_json::from_value::<Typeface>(serde_json::json!(bad)).is_err());
        }
    }

    #[test]
    fn colors_are_masked_to_24_bits() {
        let s = TextStyle {
            ink: 0xFFAB_CDEF,
            paper: 0x0100_0000,
            ..TextStyle::default()
        }
        .clamped();
        assert_eq!((s.ink, s.paper), (0xAB_CDEF, 0));
    }

    #[test]
    fn letter_spacing_is_clamped_and_may_be_negative() {
        let l = |v| {
            TextStyle {
                letter_spacing: v,
                ..TextStyle::default()
            }
            .clamped()
            .letter_spacing
        };
        assert_eq!(
            (l(-500), l(-5), l(0), l(25), l(500)),
            (MIN_LETTER_SPACING, -5, 0, 25, MAX_LETTER_SPACING)
        );
    }
}
