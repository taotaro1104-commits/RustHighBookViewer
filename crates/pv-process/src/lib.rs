//! ページ処理。画像処理をフィルタの列（Pipeline）として表現する。
//! Resize / Rotate / Color（明るさ・コントラスト・ガンマ・白黒・セピア・反転）。AutoCrop 等は P1。

mod color;

use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use pv_decode::{Bitmap, Size};

pub use color::ColorAdjust;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    None,
    Cw90,
    Cw180,
    Cw270,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resample {
    Bilinear,
    Lanczos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Rotate(Rotation),
    Resize { size: Size, method: Resample },
    Color(ColorAdjust),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pipeline(pub Vec<Filter>);

impl Pipeline {
    pub fn apply(&self, src: Bitmap) -> Bitmap {
        self.0.iter().fold(src, |bmp, f| match *f {
            Filter::Rotate(r) => rotate(&bmp, r),
            Filter::Resize { size, method } => resize(bmp, size, method),
            Filter::Color(adj) => color::apply(bmp, adj),
        })
    }

    /// キャッシュキー用の安定ハッシュ（FNV-1a。プロセスやバージョンをまたいで同じ値）。
    pub fn cache_key(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in format!("{:?}", self.0).bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        h
    }
}

fn resize(src: Bitmap, size: Size, method: Resample) -> Bitmap {
    if size == src.size || size.w == 0 || size.h == 0 || src.size.w == 0 || src.size.h == 0 {
        return src;
    }
    let alg = match method {
        Resample::Bilinear => FilterType::Bilinear,
        Resample::Lanczos => FilterType::Lanczos3,
    };
    let fallback = src.clone();
    let Ok(input) = Image::from_vec_u8(src.size.w, src.size.h, src.data, PixelType::U8x4) else {
        return fallback;
    };
    let mut out = Image::new(size.w, size.h, PixelType::U8x4);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(alg));
    if Resizer::new().resize(&input, &mut out, &opts).is_err() {
        return fallback;
    }
    Bitmap::from_rgba(size, out.into_vec())
}

fn rotate(src: &Bitmap, r: Rotation) -> Bitmap {
    let (w, h) = (src.size.w as usize, src.size.h as usize);
    let (nw, nh) = match r {
        Rotation::Cw90 | Rotation::Cw270 => (h, w),
        _ => (w, h),
    };
    if r == Rotation::None {
        return src.clone();
    }
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = match r {
                Rotation::Cw90 => (h - 1 - y, x),
                Rotation::Cw180 => (w - 1 - x, h - 1 - y),
                Rotation::Cw270 => (y, w - 1 - x),
                Rotation::None => (x, y),
            };
            let s = y * src.stride + x * 4;
            let d = (dy * nw + dx) * 4;
            out[d..d + 4].copy_from_slice(&src.data[s..s + 4]);
        }
    }
    Bitmap::from_rgba(Size::new(nw as u32, nh as u32), out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> Bitmap {
        Bitmap::from_rgba(Size::new(w, h), px.repeat((w * h) as usize))
    }

    #[test]
    fn resize_changes_dimensions_and_keeps_color() {
        let p = Pipeline(vec![Filter::Resize {
            size: Size::new(4, 2),
            method: Resample::Lanczos,
        }]);
        let out = p.apply(solid(16, 8, [10, 20, 30, 255]));
        assert_eq!(out.size, Size::new(4, 2));
        assert_eq!(&out.data[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn resize_to_same_size_is_noop() {
        let src = solid(4, 4, [1, 2, 3, 255]);
        let p = Pipeline(vec![Filter::Resize {
            size: Size::new(4, 4),
            method: Resample::Bilinear,
        }]);
        assert_eq!(p.apply(src.clone()), src);
    }

    #[test]
    fn rotate_90_moves_pixels() {
        // 2x1: [A][B] → 回転後 1x2: 上 A / 下 B
        let a = [1, 0, 0, 255];
        let b = [2, 0, 0, 255];
        let src = Bitmap::from_rgba(Size::new(2, 1), [a, b].concat());
        let out = Pipeline(vec![Filter::Rotate(Rotation::Cw90)]).apply(src);
        assert_eq!(out.size, Size::new(1, 2));
        assert_eq!(out.data, [a, b].concat());
    }

    #[test]
    fn rotate_180_twice_is_identity() {
        let src = Bitmap::from_rgba(Size::new(2, 2), (0..16).collect());
        let p = Pipeline(vec![
            Filter::Rotate(Rotation::Cw180),
            Filter::Rotate(Rotation::Cw180),
        ]);
        assert_eq!(p.apply(src.clone()), src);
    }

    #[test]
    fn cache_key_differs_by_filters() {
        let a = Pipeline(vec![Filter::Rotate(Rotation::Cw90)]);
        let b = Pipeline(vec![Filter::Rotate(Rotation::Cw180)]);
        assert_ne!(a.cache_key(), b.cache_key());
        assert_eq!(a.cache_key(), a.clone().cache_key());
        assert_ne!(Pipeline::default().cache_key(), a.cache_key());
    }
}
