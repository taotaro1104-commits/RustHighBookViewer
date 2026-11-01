//! 色調整フィルタ（明るさ・コントラスト・ガンマ・白黒・セピア・反転）。
//! すべて整数のパラメータにして、設定の比較・ハッシュ・保存を単純にする。

use pv_decode::Bitmap;
use serde::{Deserialize, Serialize};

/// 各値の範囲は -100..=100。0 が無調整。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorAdjust {
    pub brightness: i8,
    pub contrast: i8,
    /// 0 で 1.0、+100 で 4.0（明るく）、-100 で 0.25（暗く）
    pub gamma: i8,
    pub grayscale: bool,
    pub sepia: bool,
    pub invert: bool,
}

impl ColorAdjust {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// 範囲外の値を収めた値を返す（UI や設定ファイルの不正値対策）。
    pub fn clamped(self) -> Self {
        let c = |v: i8| v.clamp(-100, 100);
        Self {
            brightness: c(self.brightness),
            contrast: c(self.contrast),
            gamma: c(self.gamma),
            ..self
        }
    }

    /// 輝度・コントラスト・ガンマを合成した 256 エントリの変換表。
    fn lut(&self) -> [u8; 256] {
        let a = self.clamped();
        let exp = 2f32.powf(-(a.gamma as f32) / 50.0); // 出力 = 入力^exp。gamma>0 で明るい側へ
        let contrast = a.contrast as f32 / 100.0;
        let factor = if contrast >= 0.0 {
            1.0 / (1.0 - contrast * 0.9)
        } else {
            1.0 + contrast
        };
        let shift = a.brightness as f32 / 100.0;
        let mut lut = [0u8; 256];
        for (i, out) in lut.iter_mut().enumerate() {
            let v = (i as f32 / 255.0).powf(exp);
            let v = (v - 0.5) * factor + 0.5 + shift;
            *out = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        lut
    }
}

pub(crate) fn apply(mut bmp: Bitmap, adj: ColorAdjust) -> Bitmap {
    if adj.is_identity() {
        return bmp;
    }
    let lut = adj.lut();
    let (w, h, stride) = (bmp.size.w as usize, bmp.size.h as usize, bmp.stride);
    for y in 0..h {
        for px in bmp.data[y * stride..y * stride + w * 4]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
        {
            let (mut r, mut g, mut b) = (
                lut[px[0] as usize],
                lut[px[1] as usize],
                lut[px[2] as usize],
            );
            if adj.grayscale || adj.sepia {
                let y = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
                if adj.sepia {
                    r = (y * 107 / 100).min(255) as u8;
                    g = (y * 74 / 100) as u8;
                    b = (y * 43 / 100) as u8;
                } else {
                    (r, g, b) = (y as u8, y as u8, y as u8);
                }
            }
            if adj.invert {
                (r, g, b) = (255 - r, 255 - g, 255 - b);
            }
            px[0] = r;
            px[1] = g;
            px[2] = b;
        }
    }
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use pv_decode::Size;

    fn px(rgba: [u8; 4]) -> Bitmap {
        Bitmap::from_rgba(Size::new(1, 1), rgba.to_vec())
    }

    fn run(adj: ColorAdjust, rgba: [u8; 4]) -> [u8; 4] {
        let out = apply(px(rgba), adj);
        [out.data[0], out.data[1], out.data[2], out.data[3]]
    }

    #[test]
    fn identity_changes_nothing() {
        let c = ColorAdjust::default();
        assert!(c.is_identity());
        assert_eq!(run(c, [10, 120, 250, 77]), [10, 120, 250, 77]);
    }

    #[test]
    fn brightness_moves_values_and_clamps() {
        let up = ColorAdjust {
            brightness: 50,
            ..Default::default()
        };
        let o = run(up, [100, 100, 100, 255]);
        assert!(o[0] > 200 && o[0] < 255, "{o:?}");
        assert_eq!(run(up, [250, 250, 250, 255])[0], 255);
        let down = ColorAdjust {
            brightness: -100,
            ..Default::default()
        };
        assert_eq!(run(down, [200, 200, 200, 255])[0], 0);
    }

    #[test]
    fn contrast_spreads_around_midpoint() {
        let c = ColorAdjust {
            contrast: 60,
            ..Default::default()
        };
        assert!(run(c, [200, 200, 200, 255])[0] > 200);
        assert!(run(c, [50, 50, 50, 255])[0] < 50);
        let flat = ColorAdjust {
            contrast: -100,
            ..Default::default()
        };
        let (a, b) = (
            run(flat, [0, 0, 0, 255])[0],
            run(flat, [255, 255, 255, 255])[0],
        );
        assert!(
            a.abs_diff(b) <= 1,
            "fully flat contrast collapses to mid gray"
        );
    }

    #[test]
    fn gamma_positive_lifts_midtones_but_keeps_ends() {
        let g = ColorAdjust {
            gamma: 50,
            ..Default::default()
        };
        assert!(run(g, [128, 128, 128, 255])[0] > 150);
        assert_eq!(run(g, [0, 0, 0, 255])[0], 0);
        assert_eq!(run(g, [255, 255, 255, 255])[0], 255);
    }

    #[test]
    fn invert_and_alpha() {
        let c = ColorAdjust {
            invert: true,
            ..Default::default()
        };
        assert_eq!(run(c, [0, 100, 255, 33]), [255, 155, 0, 33]);
    }

    #[test]
    fn grayscale_equalizes_channels() {
        let c = ColorAdjust {
            grayscale: true,
            ..Default::default()
        };
        let o = run(c, [255, 0, 0, 255]);
        assert!(o[0] == o[1] && o[1] == o[2]);
        assert!((70..=80).contains(&o[0]), "luma of pure red ≈ 76: {o:?}");
    }

    #[test]
    fn sepia_is_warm_toned() {
        let c = ColorAdjust {
            sepia: true,
            ..Default::default()
        };
        let o = run(c, [128, 128, 128, 255]);
        assert!(o[0] > o[1] && o[1] > o[2], "{o:?}");
    }

    #[test]
    fn out_of_range_values_are_clamped_not_panicking() {
        let c = ColorAdjust {
            brightness: i8::MAX,
            contrast: i8::MIN,
            gamma: i8::MIN,
            ..Default::default()
        };
        let _ = run(c, [1, 2, 3, 4]);
        assert_eq!(c.clamped().contrast, -100);
    }
}
