#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

impl Size {
    pub const fn new(w: u32, h: u32) -> Self {
        Self { w, h }
    }

    pub fn pixels(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
}

/// RGBA8 の所有バッファ。将来の GPU アップロードを考え行ストライド（バイト）を持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub size: Size,
    pub stride: usize,
    pub data: Vec<u8>,
}

impl Bitmap {
    pub fn from_rgba(size: Size, data: Vec<u8>) -> Self {
        debug_assert_eq!(data.len(), size.w as usize * size.h as usize * 4);
        Self {
            size,
            stride: size.w as usize * 4,
            data,
        }
    }

    pub fn row(&self, y: u32) -> &[u8] {
        let start = y as usize * self.stride;
        &self.data[start..start + self.size.w as usize * 4]
    }

    /// 整数倍率 `factor` の箱型フィルタで縮小する。`factor <= 1` はそのまま返す。
    /// SIMD（fast_image_resize）で縮め、使えないときは `downscale_box_scalar` に戻る。
    pub fn downscale_box(&self, factor: u32) -> Bitmap {
        if factor <= 1 {
            return self.clone();
        }
        let (nw, nh) = ((self.size.w / factor).max(1), (self.size.h / factor).max(1));
        self.downscale_box_simd(nw, nh, factor)
            .unwrap_or_else(|| self.downscale_box_scalar(factor))
    }

    fn downscale_box_simd(&self, nw: u32, nh: u32, factor: u32) -> Option<Bitmap> {
        use fast_image_resize::images::{Image, ImageRef};
        use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

        let packed = self.stride == self.size.w as usize * 4;
        let src = if packed {
            ImageRef::new(self.size.w, self.size.h, &self.data, PixelType::U8x4).ok()?
        } else {
            return None;
        };
        let mut out = Image::new(nw, nh, PixelType::U8x4);
        // 従来の実装と同じく、倍率で割り切れる範囲（端数の行・列は除く）だけを、アルファの前乗算なしで平均する
        let opts = ResizeOptions::new()
            .resize_alg(ResizeAlg::Convolution(FilterType::Box))
            .use_alpha(false)
            .crop(0.0, 0.0, f64::from(nw * factor), f64::from(nh * factor));
        Resizer::new().resize(&src, &mut out, &opts).ok()?;
        Some(Bitmap::from_rgba(Size::new(nw, nh), out.into_vec()))
    }

    /// 素朴な実装（基準・フォールバック用）。
    fn downscale_box_scalar(&self, factor: u32) -> Bitmap {
        let (nw, nh) = ((self.size.w / factor).max(1), (self.size.h / factor).max(1));
        let mut out = Vec::with_capacity(nw as usize * nh as usize * 4);
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for dy in 0..factor {
                    let sy = y * factor + dy;
                    if sy >= self.size.h {
                        break;
                    }
                    let row = self.row(sy);
                    for dx in 0..factor {
                        let sx = x * factor + dx;
                        if sx >= self.size.w {
                            break;
                        }
                        let p = &row[sx as usize * 4..sx as usize * 4 + 4];
                        for c in 0..4 {
                            acc[c] += p[c] as u32;
                        }
                        n += 1;
                    }
                }
                out.extend(acc.iter().map(|v| (v / n.max(1)) as u8));
            }
        }
        Bitmap::from_rgba(Size::new(nw, nh), out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_averages_blocks() {
        // 2x2 → 1x1: 平均 (0,100,200,255 の R)
        let data = vec![
            0, 0, 0, 255, 100, 0, 0, 255, //
            200, 0, 0, 255, 100, 0, 0, 255,
        ];
        let b = Bitmap::from_rgba(Size::new(2, 2), data).downscale_box(2);
        assert_eq!(b.size, Size::new(1, 1));
        assert_eq!(b.data, vec![100, 0, 0, 255]);
    }

    #[test]
    fn downscale_factor_one_is_identity() {
        let b = Bitmap::from_rgba(Size::new(1, 1), vec![1, 2, 3, 4]);
        assert_eq!(b.downscale_box(1), b);
    }

    #[test]
    fn downscale_handles_odd_dimensions() {
        let b = Bitmap::from_rgba(Size::new(3, 3), vec![9; 3 * 3 * 4]).downscale_box(2);
        assert_eq!(b.size, Size::new(1, 1));
        assert_eq!(b.data, vec![9, 9, 9, 9]);
    }

    #[test]
    fn simd_downscale_matches_scalar_within_rounding() {
        let (w, h) = (66u32, 50u32);
        let data: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                [(x * 3) as u8, (y * 5) as u8, ((x * y) % 251) as u8, 255]
            })
            .collect();
        let b = Bitmap::from_rgba(Size::new(w, h), data);
        for factor in [2u32, 4, 8] {
            let fast = b.downscale_box(factor);
            let slow = b.downscale_box_scalar(factor);
            assert_eq!(fast.size, slow.size, "factor {factor}");
            // 端数の行・列の扱いの差があるので、内側の画素だけ比べる
            let (iw, ih) = (fast.size.w - 1, fast.size.h - 1);
            for y in 0..ih {
                for x in 0..iw {
                    for c in 0..4usize {
                        let (p, q) = (
                            i32::from(fast.row(y)[x as usize * 4 + c]),
                            i32::from(slow.row(y)[x as usize * 4 + c]),
                        );
                        assert!(
                            (p - q).abs() <= 2,
                            "factor {factor} ({x},{y}) c{c}: {p} vs {q}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn bench_downscale_box() {
        let (w, h) = (2400u32, 3400u32);
        let b = Bitmap::from_rgba(Size::new(w, h), vec![77; w as usize * h as usize * 4]);
        let t = std::time::Instant::now();
        for _ in 0..5 {
            std::hint::black_box(b.downscale_box(8));
        }
        println!("downscale_box(8) 2400x3400: {:?}", t.elapsed() / 5);
    }
}
