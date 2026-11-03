//! 1 スプレッドの描画: 読み出し(L2) → デコード → リサイズ(L1) → 合成。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::Mutex;
use pv_container::{Container, EntryId};
use pv_decode::{Bitmap, Decoder, ImageDecoder, Size};
use pv_process::{ColorAdjust, Filter, Pipeline, Resample};
use pv_render::{fit_scale, FitMode, Vec2};

use crate::cache::{ByteLru, PageKey, RawKey};
use crate::error::{CoreError, Result};

/// 1 ページの長辺上限。これを超える巨大な出力はこの範囲に収める（タイル描画は未実装）。
pub const MAX_DIM: u32 = 8192;

pub(crate) struct Inner {
    pub container: Arc<dyn Container>,
    pub book_id: u64,
    pub pages: Vec<EntryId>,
    pub decoder: ImageDecoder,
    pub l1: Mutex<ByteLru<PageKey, Arc<Bitmap>>>,
    pub l2: Mutex<ByteLru<RawKey, Bytes>>,
    pub generation: AtomicU64,
}

impl Inner {
    fn check(&self, gen: u64) -> Result<()> {
        if self.generation.load(Ordering::Acquire) == gen {
            Ok(())
        } else {
            Err(CoreError::Cancelled)
        }
    }

    pub fn raw(&self, page: usize) -> Result<Bytes> {
        let id = *self.pages.get(page).ok_or(CoreError::NoPages)?;
        let key = RawKey {
            book: self.book_id,
            entry: id.0,
        };
        if let Some(b) = self.l2.lock().get(&key) {
            return Ok(b);
        }
        let data = self.container.read(id)?;
        self.l2.lock().put(key, data.clone());
        Ok(data)
    }

    /// ページを `bx × by` の枠にフィット方式で収めた大きさ。
    fn dest_size(src: Size, bx: f32, by: f32, fit: FitMode) -> Size {
        let (mut w, mut h) = if fit == FitMode::Fill {
            // 全画面: 縦横比を無視して、枠ちょうどに引き伸ばす（全体が画面に収まる）
            (bx.round().max(1.0), by.round().max(1.0))
        } else {
            let s = fit_scale(
                Vec2::new(src.w as f32, src.h as f32),
                Vec2::new(bx, by),
                fit,
            );
            (
                (src.w as f32 * s).round().max(1.0),
                (src.h as f32 * s).round().max(1.0),
            )
        };
        let over = (w.max(h) / MAX_DIM as f32).max(1.0);
        w = (w / over).round().max(1.0);
        h = (h / over).round().max(1.0);
        Size::new(w as u32, h as u32)
    }

    fn page_bitmap(
        &self,
        page: usize,
        bx: f32,
        by: f32,
        fit: FitMode,
        color: ColorAdjust,
        gen: u64,
    ) -> Result<Arc<Bitmap>> {
        let id = *self.pages.get(page).ok_or(CoreError::NoPages)?;
        let raw = self.raw(page)?;
        let src_size = self.decoder.probe_size(&raw)?;
        let dest = Self::dest_size(src_size, bx, by, fit);

        let mut filters = vec![Filter::Resize {
            size: dest,
            method: Resample::Lanczos,
        }];
        // 縮小後に掛けると画素数が少ない分だけ速い
        if !color.is_identity() {
            filters.push(Filter::Color(color));
        }
        let pipeline = Pipeline(filters);
        let key = PageKey {
            book: self.book_id,
            entry: id.0,
            pipeline: pipeline.cache_key(),
            w: dest.w,
            h: dest.h,
        };
        if let Some(hit) = self.l1.lock().get(&key) {
            return Ok(hit);
        }
        self.check(gen)?;
        let decoded = self.decoder.decode(&raw, Some(dest))?;
        self.check(gen)?;
        let out = Arc::new(pipeline.apply(decoded));
        self.l1.lock().put(key, Arc::clone(&out));
        Ok(out)
    }

    /// ページ一覧用の小さなサムネイル（長辺 `edge` 以内、拡大はしない）。L1 は汚さない。
    pub fn page_thumbnail(&self, page: usize, edge: u32) -> Result<Bitmap> {
        let raw = self.raw(page)?;
        let src = self.decoder.probe_size(&raw)?;
        let s = fit_scale(
            Vec2::new(src.w as f32, src.h as f32),
            Vec2::new(edge as f32, edge as f32),
            FitMode::Page,
        )
        .min(1.0);
        let dest = Size::new(
            ((src.w as f32 * s).round() as u32).max(1),
            ((src.h as f32 * s).round() as u32).max(1),
        );
        let bmp = self.decoder.decode(&raw, Some(dest))?;
        Ok(Pipeline(vec![Filter::Resize {
            size: dest,
            method: Resample::Lanczos,
        }])
        .apply(bmp))
    }

    /// `pages`（画面左→右）を枠に収めて横に並べた 1 枚のビットマップを作る。
    pub fn render_spread(
        &self,
        pages: &[usize],
        vp: Size,
        scale: f32,
        fit: FitMode,
        color: ColorAdjust,
        gen: u64,
    ) -> Result<Bitmap> {
        let n = pages.len().max(1) as f32;
        let (bx, by) = (vp.w as f32 * scale / n, vp.h as f32 * scale);
        let bitmaps = pages
            .iter()
            .map(|&p| self.page_bitmap(p, bx, by, fit, color, gen))
            .collect::<Result<Vec<_>>>()?;
        self.check(gen)?;
        Ok(compose(&bitmaps))
    }
}

/// ビットマップを横に並べ、縦は中央揃え（余白は黒）。
fn compose(parts: &[Arc<Bitmap>]) -> Bitmap {
    if let [only] = parts {
        return (**only).clone();
    }
    let w: u32 = parts.iter().map(|b| b.size.w).sum();
    let h: u32 = parts.iter().map(|b| b.size.h).max().unwrap_or(0);
    let mut data = [0u8, 0, 0, 255].repeat(w as usize * h as usize);
    let mut x0 = 0usize;
    for b in parts {
        let y0 = (h - b.size.h) as usize / 2;
        for y in 0..b.size.h {
            let dst = ((y0 + y as usize) * w as usize + x0) * 4;
            let row = b.row(y);
            data[dst..dst + row.len()].copy_from_slice(row);
        }
        x0 += b.size.w as usize;
    }
    Bitmap::from_rgba(Size::new(w, h), data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> Arc<Bitmap> {
        Arc::new(Bitmap::from_rgba(
            Size::new(w, h),
            vec![v; (w * h * 4) as usize],
        ))
    }

    #[test]
    fn compose_places_side_by_side_and_centers() {
        let out = compose(&[solid(2, 4, 10), solid(2, 2, 20)]);
        assert_eq!(out.size, Size::new(4, 4));
        assert_eq!(out.row(0)[..4], [10; 4]);
        assert_eq!(out.row(0)[8..12], [0, 0, 0, 255]); // 低い方の上余白
        assert_eq!(out.row(1)[8..12], [20; 4]);
    }

    #[test]
    fn dest_size_modes() {
        let src = Size::new(100, 200);
        assert_eq!(
            Inner::dest_size(src, 400.0, 400.0, FitMode::Page),
            Size::new(200, 400)
        );
        assert_eq!(
            Inner::dest_size(src, 400.0, 400.0, FitMode::Width),
            Size::new(400, 800)
        );
        assert_eq!(
            Inner::dest_size(src, 400.0, 100.0, FitMode::Height),
            Size::new(50, 100)
        );
    }

    /// 全画面は、縦横比を無視して枠ちょうどに引き伸ばす
    #[test]
    fn dest_size_fill_ignores_aspect_ratio() {
        for src in [Size::new(100, 200), Size::new(300, 100), Size::new(50, 50)] {
            assert_eq!(
                Inner::dest_size(src, 400.0, 300.0, FitMode::Fill),
                Size::new(400, 300),
                "{src:?}"
            );
        }
    }

    #[test]
    fn dest_size_is_capped() {
        let d = Inner::dest_size(Size::new(100, 100_000), 100.0, 100.0, FitMode::Width);
        assert!(d.w.max(d.h) <= MAX_DIM && d.w >= 1);
    }
}
