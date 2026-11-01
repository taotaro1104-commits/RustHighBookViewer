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
    pub fn downscale_box(&self, factor: u32) -> Bitmap {
        if factor <= 1 {
            return self.clone();
        }
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
}
