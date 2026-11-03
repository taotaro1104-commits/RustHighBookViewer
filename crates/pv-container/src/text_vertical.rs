//! 縦書きの字形の描き分け。
//! 日本語の縦書きでは、括弧・句読点・長音などの形や位置が横書きと違う。
//! フォントに縦書き用の字形（CJK 互換形 U+FE10〜FE4F）があればそれを使い、無ければ回転や位置の補正で近づける。

use ab_glyph::{point, Font, PxScale, ScaleFont};

use crate::font::FontSet;
use crate::text_glyph::{draw_shape, has_shape};

/// 描画先（グレースケール）
pub struct Canvas<'a> {
    pub buf: &'a mut [u8],
    pub w: usize,
    pub h: usize,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i64, y: i64, cov: f32, ink: u8) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let a = (cov.clamp(0.0, 1.0) * 255.0) as u32;
        let dst = &mut self.buf[y as usize * self.w + x as usize];
        *dst = ((u32::from(*dst) * (255 - a) + u32::from(ink) * a) / 255) as u8;
    }
}

/// 縦書き用の字形（CJK 互換形）。フォントに無ければ使わない。
fn vertical_form(c: char) -> Option<char> {
    Some(match c {
        '、' => '\u{fe11}',
        '。' => '\u{fe12}',
        '…' => '\u{fe19}',
        '‥' => '\u{fe30}',
        '―' | '—' => '\u{fe31}',
        '（' | '(' => '\u{fe35}',
        '）' | ')' => '\u{fe36}',
        '｛' | '{' => '\u{fe37}',
        '｝' | '}' => '\u{fe38}',
        '〔' => '\u{fe39}',
        '〕' => '\u{fe3a}',
        '【' => '\u{fe3b}',
        '】' => '\u{fe3c}',
        '《' => '\u{fe3d}',
        '》' => '\u{fe3e}',
        '〈' => '\u{fe3f}',
        '〉' => '\u{fe40}',
        '「' => '\u{fe41}',
        '」' => '\u{fe42}',
        '『' => '\u{fe43}',
        '』' => '\u{fe44}',
        _ => return None,
    })
}

/// 縦書きでは 90 度回して描く文字（長音・ダッシュ・波線・括弧）。
fn rotates(c: char) -> bool {
    "ー－−〜～―—─━…‥=＝（）〔〕【】《》〈〉「」『』()[]{}｛｝［］".contains(c)
}

/// 小書きの仮名は、縦書きでは枠の右上に寄せる。
fn is_small_kana(c: char) -> bool {
    "ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ".contains(c)
}

enum Place {
    /// 立てたまま描く。(dx, dy) は枠の大きさに対する補正
    Upright(f32, f32),
    Rotated,
}

/// どの字形を、どう置くか。
fn plan(font: &FontSet, c: char) -> (char, Place) {
    if let Some(v) = vertical_form(c) {
        if font.find(v).is_some() {
            return (v, Place::Upright(0.0, 0.0));
        }
    }
    if rotates(c) {
        return (c, Place::Rotated);
    }
    match c {
        // 横書き用の句読点は枠の左下にあるので、右上へ寄せる
        '、' | '。' | '，' | '．' => (c, Place::Upright(0.5, -0.5)),
        c if is_small_kana(c) => (c, Place::Upright(0.1, -0.1)),
        c => (c, Place::Upright(0.0, 0.0)),
    }
}

/// 列（中心 `cx`）の上端 `top` から下へ、1 文字 `size` ずつ並べて描く。
pub fn draw_vertical(
    font: &FontSet,
    canvas: &mut Canvas,
    col: Column,
    text: &str,
    ink: u8,
    look: Look,
) {
    let Column {
        cx,
        top,
        size,
        pitch,
    } = col;
    let scale = PxScale::from(size);
    let scaled = font.primary().as_scaled(scale);
    // 枠（全角）の中心に字形の中心を合わせるための、ベースラインの位置
    let centre_offset = (scaled.ascent() + scaled.descent()) / 2.0;
    for (i, c) in text.chars().enumerate() {
        let cy = top + (i as f32 + 0.5) * pitch;
        let (shown, place) = plan(font, c);
        let found = font.find(shown);
        if found.is_none() && has_shape(shown) {
            // どのフォントにも無い丸などの記号は、枠の中心に円を描く
            draw_shape(shown, cx, cy, size, &mut |x, y, cov| {
                canvas.blend(x, y, cov, ink)
            });
            continue;
        }
        let glyphs = found.unwrap_or(font.primary());
        let id = glyphs.glyph_id(shown);
        let adv = glyphs.as_scaled(scale).h_advance(id);
        let (dx, dy) = match place {
            Place::Upright(dx, dy) => (dx * size, dy * size),
            Place::Rotated => (0.0, 0.0),
        };
        let base = point(cx - adv / 2.0 + dx, cy + centre_offset + dy);
        let rotated = matches!(place, Place::Rotated);
        // 太字は、わずかにずらして重ね描きして太らせる（フォントに太字が無くても使える）
        let thick = if look.bold { size * 0.035 } else { 0.0 };
        let offsets: &[(f32, f32)] = if look.bold {
            &[(0.0, 0.0), (thick, 0.0), (0.0, thick), (thick, thick)]
        } else {
            &[(0.0, 0.0)]
        };
        for &(ox, oy) in offsets {
            let pen = point(base.x + ox, base.y + oy);
            let Some(outlined) = glyphs.outline_glyph(id.with_scale_and_position(scale, pen))
            else {
                continue;
            };
            let b = outlined.px_bounds();
            outlined.draw(|gx, gy, cov| {
                let mut x = b.min.x as i64 + i64::from(gx);
                let mut y = b.min.y as i64 + i64::from(gy);
                if rotated {
                    // 枠の中心まわりに時計回りへ 90 度: 右 → 下、下 → 左
                    let du = x as f32 + 0.5 - cx;
                    let dv = y as f32 + 0.5 - cy;
                    x = (cx - dv - 0.5).round() as i64;
                    y = (cy + du - 0.5).round() as i64;
                }
                if look.italic {
                    // 枠の中心を軸に、上ほど右へ傾ける（行ごとにずらすだけなので、欠けは出ない）
                    x += ((cy - y as f32) * ITALIC_SLANT).round() as i64;
                }
                canvas.blend(x, y, cov, ink);
            });
        }
    }
}

/// 斜体の傾き（高さ 1 に対する横ずれ）
const ITALIC_SLANT: f32 = 0.2;

/// 縦書きの 1 列（中心の x、最初の文字の上端、文字の大きさ、文字を並べる間隔）
#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub cx: f32,
    pub top: f32,
    pub size: f32,
    pub pitch: f32,
}

/// 字形の見た目
#[derive(Debug, Clone, Copy, Default)]
pub struct Look {
    pub bold: bool,
    pub italic: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_and_brackets_have_vertical_treatment() {
        assert_eq!(vertical_form('。'), Some('\u{fe12}'));
        assert_eq!(vertical_form('「'), Some('\u{fe41}'));
        assert_eq!(vertical_form('あ'), None);
        assert!(rotates('ー') && rotates('「') && !rotates('あ'));
        assert!(is_small_kana('っ') && !is_small_kana('つ'));
    }
}
