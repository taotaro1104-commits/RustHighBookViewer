//! 縦書きの字形の描き分け。
//! 日本語の縦書きでは、括弧・句読点・長音などの形や位置が横書きと違う。
//! フォントに縦書き用の字形（CJK 互換形 U+FE10〜FE4F）があればそれを使い、無ければ回転や位置の補正で近づける。

use ab_glyph::{point, Font, FontVec, PxScale, ScaleFont};

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
fn plan(font: &FontVec, c: char) -> (char, Place) {
    if let Some(v) = vertical_form(c) {
        if font.glyph_id(v).0 != 0 {
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
    font: &FontVec,
    canvas: &mut Canvas,
    cx: f32,
    top: f32,
    size: f32,
    text: &str,
    ink: u8,
) {
    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    // 枠（全角）の中心に字形の中心を合わせるための、ベースラインの位置
    let centre_offset = (scaled.ascent() + scaled.descent()) / 2.0;
    for (i, c) in text.chars().enumerate() {
        let cy = top + (i as f32 + 0.5) * size;
        let (shown, place) = plan(font, c);
        let id = font.glyph_id(shown);
        let adv = scaled.h_advance(id);
        let (dx, dy) = match place {
            Place::Upright(dx, dy) => (dx * size, dy * size),
            Place::Rotated => (0.0, 0.0),
        };
        let pen = point(cx - adv / 2.0 + dx, cy + centre_offset + dy);
        let Some(outlined) = font.outline_glyph(id.with_scale_and_position(scale, pen)) else {
            continue;
        };
        let b = outlined.px_bounds();
        let rotated = matches!(place, Place::Rotated);
        outlined.draw(|gx, gy, cov| {
            let x = b.min.x as i64 + i64::from(gx);
            let y = b.min.y as i64 + i64::from(gy);
            if rotated {
                // 枠の中心まわりに時計回りへ 90 度: 右 → 下、下 → 左
                let du = x as f32 + 0.5 - cx;
                let dv = y as f32 + 0.5 - cy;
                canvas.blend(
                    (cx - dv - 0.5).round() as i64,
                    (cy + du - 0.5).round() as i64,
                    cov,
                    ink,
                );
            } else {
                canvas.blend(x, y, cov, ink);
            }
        });
    }
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
