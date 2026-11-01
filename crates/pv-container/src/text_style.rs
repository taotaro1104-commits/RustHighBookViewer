//! 文章の EPUB の見た目（文字の大きさ・書体）。
//! 変えるとページの割り付けが変わるので、本を開き直したときに反映される。
//! `open_source_styled` で開くときに渡す。

use serde::{Deserialize, Serialize};

pub const MIN_FONT_PX: u32 = 28;
pub const MAX_FONT_PX: u32 = 80;
pub const DEFAULT_FONT_PX: u32 = 44;
/// 行間（文字の大きさに対する割合、%）。100 で行どうしが接する
pub const MIN_LINE_SPACING: u32 = 120;
pub const MAX_LINE_SPACING: u32 = 240;
pub const DEFAULT_LINE_SPACING: u32 = 160;

/// 書体の種類
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Typeface {
    /// ゴシック体（既定）
    #[default]
    Gothic,
    /// 明朝体
    Mincho,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    /// 本文の文字の大きさ（ページ画像上の画素。ページの幅は 1080）
    pub font_px: u32,
    pub typeface: Typeface,
    /// 行の送り幅（文字の大きさに対する割合、%）
    pub line_spacing: u32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_px: DEFAULT_FONT_PX,
            typeface: Typeface::Gothic,
            line_spacing: DEFAULT_LINE_SPACING,
        }
    }
}

impl TextStyle {
    /// 範囲外の値を収める（UI や設定ファイルの不正値対策）。
    pub fn clamped(self) -> Self {
        Self {
            font_px: self.font_px.clamp(MIN_FONT_PX, MAX_FONT_PX),
            line_spacing: self.line_spacing.clamp(MIN_LINE_SPACING, MAX_LINE_SPACING),
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
            }
            .clamped()
            .font_px
        };
        assert_eq!((s(0), s(44), s(10_000)), (MIN_FONT_PX, 44, MAX_FONT_PX));
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
                line_spacing: DEFAULT_LINE_SPACING
            }
        );
        // 欠けている項目は既定値
        let d: TextStyle = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(d, TextStyle::default());
    }
}
