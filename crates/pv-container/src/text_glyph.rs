//! 文章の EPUB で、字形を持たない文字の扱い。
//! - 異体字セレクタ・ゼロ幅の制御文字は、字として描かず場所も取らない（そのままだと 1 文字分の空白や豆腐になる）
//! - 白丸・黒丸などの記号は、端末のどのフォントにも無いとき、円を自前で描く

/// 描かず、場所も取らない文字か。異体字セレクタ（U+FE0E 文字表示 / U+FE0F 絵文字表示 / 漢字の異体字 U+E0100〜）、
/// ゼロ幅スペース・接合子、方向指定、BOM、ソフトハイフン、タグ文字、囲みキーなど。
pub fn is_ignorable(c: char) -> bool {
    matches!(c,
        '\u{00ad}'                   // ソフトハイフン
        | '\u{034f}'                 // 結合書記素接合子
        | '\u{180b}'..='\u{180f}'    // モンゴル語の異体字セレクタ
        | '\u{200b}'..='\u{200f}'    // ゼロ幅スペース・ZWNJ・ZWJ・方向指定
        | '\u{202a}'..='\u{202e}'    // 方向の埋め込み
        | '\u{2060}'..='\u{2064}'    // 単語結合子ほか
        | '\u{2066}'..='\u{206f}'    // 方向の分離・古い書式文字
        | '\u{20e3}'                 // 囲みキー（絵文字のキーキャップ）
        | '\u{fe00}'..='\u{fe0f}'    // 異体字セレクタ 1〜16
        | '\u{feff}'                 // BOM / ゼロ幅の非改行スペース
        | '\u{e0001}' | '\u{e0020}'..='\u{e007f}' // タグ文字
        | '\u{e0100}'..='\u{e01ef}'  // 異体字セレクタ 17〜256
    )
}

/// 描かない文字を取り除く。無ければ元の文字列のまま。
pub fn strip_ignorable(s: &str) -> std::borrow::Cow<'_, str> {
    if s.chars().any(is_ignorable) {
        s.chars()
            .filter(|&c| !is_ignorable(c))
            .collect::<String>()
            .into()
    } else {
        s.into()
    }
}

/// 自前で描ける記号。大きさは文字の枠（`size`）に対する割合。
#[derive(Clone, Copy)]
enum Shape {
    /// 輪（半径、線の太さ）
    Ring(f32, f32),
    /// 塗りつぶした円（半径）
    Disk(f32),
}

fn shape_of(c: char) -> Option<Shape> {
    Some(match c {
        '\u{26aa}' => Shape::Ring(0.30, 0.07), // ⚪ 白丸（中）
        '\u{26ab}' => Shape::Disk(0.30),       // ⚫ 黒丸（中）
        '\u{26ac}' => Shape::Ring(0.18, 0.06), // ⚬ 白丸（小）
        '\u{2b55}' => Shape::Ring(0.38, 0.09), // ⭕ 太い大きな輪
        '\u{1f534}' | '\u{1f535}' | '\u{1f7e0}'..='\u{1f7e4}' => Shape::Disk(0.36), // 色付きの丸（単色で）
        _ => return None,
    })
}

/// 自前で描ける記号か。
pub fn has_shape(c: char) -> bool {
    shape_of(c).is_some()
}

/// 記号 `c` を、中心 (`cx`, `cy`)・枠の大きさ `size` で描く。`put(x, y, 濃さ)` を画素ごとに呼ぶ。
/// 描ける記号でなければ `false`。
pub fn draw_shape(
    c: char,
    cx: f32,
    cy: f32,
    size: f32,
    put: &mut dyn FnMut(i64, i64, f32),
) -> bool {
    let Some(shape) = shape_of(c) else {
        return false;
    };
    let (radius, half_w) = match shape {
        Shape::Ring(r, w) => (r * size, (w * size).max(1.0) / 2.0),
        Shape::Disk(r) => (r * size, 0.0),
    };
    let reach = radius + half_w + 1.5;
    let (x0, x1) = ((cx - reach).floor() as i64, (cx + reach).ceil() as i64);
    let (y0, y1) = ((cy - reach).floor() as i64, (cy + reach).ceil() as i64);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            // 境界は 1 画素ぶんぼかす
            let cov = match shape {
                Shape::Ring(..) => half_w - (d - radius).abs() + 0.5,
                Shape::Disk(_) => radius - d + 0.5,
            };
            if cov > 0.0 {
                put(x, y, cov.min(1.0));
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variation_selectors_and_zero_width_controls_are_ignorable() {
        for c in [
            '\u{fe0e}',
            '\u{fe0f}',
            '\u{fe00}',
            '\u{200d}',
            '\u{200b}',
            '\u{feff}',
            '\u{e0100}',
            '\u{20e3}',
        ] {
            assert!(is_ignorable(c), "{c:?}");
        }
        for c in ['あ', '⚪', 'A', ' ', '\u{3000}', '\n', '゙'] {
            assert!(!is_ignorable(c), "{c:?}");
        }
    }

    #[test]
    fn strip_removes_only_ignorable_chars() {
        assert_eq!(strip_ignorable("⚪\u{fe0e}あ"), "⚪あ");
        assert_eq!(strip_ignorable("葛\u{e0100}城"), "葛城");
        assert_eq!(strip_ignorable("❤\u{fe0f}\u{200d}🔥"), "❤🔥");
        assert!(matches!(
            strip_ignorable("そのまま"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    fn render(c: char, size: f32) -> (usize, Vec<f32>) {
        let n = size as usize;
        let mut cov = vec![0.0f32; n * n];
        let drawn = draw_shape(c, size / 2.0, size / 2.0, size, &mut |x, y, a| {
            if (0..n as i64).contains(&x) && (0..n as i64).contains(&y) {
                cov[y as usize * n + x as usize] = a;
            }
        });
        assert!(drawn, "{c:?}");
        (n, cov)
    }

    #[test]
    fn white_circle_is_a_ring_and_black_circle_is_filled() {
        let (n, ring) = render('\u{26aa}', 100.0);
        assert!(ring[n / 2 * n + n / 2] < 0.01, "中心は空");
        assert!(ring[n / 2 * n + n / 2 + 30] > 0.9, "輪の上は濃い");
        let (n, disk) = render('\u{26ab}', 100.0);
        assert!(disk[n / 2 * n + n / 2] > 0.99, "中心は塗られる");
        assert!(disk[n / 2 * n + 2] < 0.01, "枠の端は空");
    }

    #[test]
    fn shapes_stay_inside_their_cell() {
        for c in ['\u{26aa}', '\u{26ab}', '\u{26ac}', '\u{2b55}', '\u{1f534}'] {
            let size = 60.0;
            draw_shape(c, 30.0, 30.0, size, &mut |x, y, _| {
                assert!(
                    (0..60).contains(&x) && (0..60).contains(&y),
                    "{c:?} ({x},{y})"
                );
            });
        }
        assert!(!has_shape('あ') && has_shape('\u{26aa}'));
        assert!(!draw_shape('あ', 0.0, 0.0, 10.0, &mut |_, _, _| {}));
    }
}
