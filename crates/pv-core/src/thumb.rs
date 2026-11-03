//! L3: ディスク上のサムネイルキャッシュ（WebP）。アプリ専用領域に置き `.nomedia` を配置する。

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageFormat};
use pv_container::{open_path, Container, EntryKind};
use pv_decode::{Bitmap, Decoder, ImageDecoder, Size};
use pv_process::{Filter, Pipeline, Resample};
use pv_render::{fit_scale, FitMode, Vec2};

use crate::error::{CoreError, Result};

/// 書庫の先頭の画像から、長辺 `edge` のサムネイルを作る。
pub fn make_thumbnail(path: &Path, edge: u32) -> Result<Bitmap> {
    make_thumbnail_from(open_path(path)?.as_ref(), edge)
}

pub fn make_thumbnail_from(container: &dyn Container, edge: u32) -> Result<Bitmap> {
    let first = container
        .entries()
        .iter()
        .find(|e| e.kind == EntryKind::Image)
        .ok_or(CoreError::NoPages)?;
    let data = container.read(first.id)?;
    let decoder = ImageDecoder::default();
    let src = decoder.probe_size(&data)?;
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
    let bmp = decoder.decode(&data, Some(dest))?;
    Ok(Pipeline(vec![Filter::Resize {
        size: dest,
        method: Resample::Lanczos,
    }])
    .apply(bmp))
}

pub struct DiskCache {
    dir: PathBuf,
    max_bytes: u64,
    /// 保存済みサムネイルの合計サイズの見積もり。上限を超えたときだけディレクトリを走査して整理する
    /// （保存のたびに全ファイルを調べると、多数の本の登録で処理が 2 乗に増える）
    approx_bytes: AtomicU64,
}

impl DiskCache {
    pub fn open(dir: &Path, max_bytes: u64) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let nomedia = dir.join(".nomedia");
        if !nomedia.exists() {
            fs::write(nomedia, b"")?;
        }
        let cache = Self {
            dir: dir.to_owned(),
            max_bytes,
            approx_bytes: AtomicU64::new(0),
        };
        cache
            .approx_bytes
            .store(cache.list()?.iter().map(|f| f.1).sum(), Ordering::Relaxed);
        Ok(cache)
    }

    fn path(&self, key: u64) -> PathBuf {
        self.dir.join(format!("{key:016x}.webp"))
    }

    pub fn get(&self, key: u64) -> Option<Bitmap> {
        let p = self.path(key);
        let data = fs::read(&p).ok()?;
        let img = image::load_from_memory_with_format(&data, ImageFormat::WebP)
            .ok()?
            .into_rgba8();
        // 追い出し順（最終アクセス日時）のため更新時刻を進める
        if let Ok(f) = fs::OpenOptions::new().write(true).open(&p) {
            let _ = f.set_modified(SystemTime::now());
        }
        let size = Size::new(img.width(), img.height());
        Some(Bitmap::from_rgba(size, img.into_raw()))
    }

    pub fn put(&self, key: u64, bmp: &Bitmap) -> Result<()> {
        let mut buf = Cursor::new(Vec::new());
        WebPEncoder::new_lossless(&mut buf)
            .encode(&bmp.data, bmp.size.w, bmp.size.h, ExtendedColorType::Rgba8)
            .map_err(|e| CoreError::Settings(format!("thumbnail encode: {e}")))?;
        let bytes = buf.into_inner();
        let len = bytes.len() as u64;
        let tmp = self.dir.join(format!("{key:016x}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, self.path(key))?;
        // 同じキーの上書きでも加算するので少し多めに見積もるが、整理のときに正確な値に直る
        if self.approx_bytes.fetch_add(len, Ordering::Relaxed) + len > self.max_bytes {
            self.evict()?;
        }
        Ok(())
    }

    /// 保存済みの (更新時刻, サイズ, パス) の一覧。
    fn list(&self) -> Result<Vec<(SystemTime, u64, PathBuf)>> {
        Ok(fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "webp"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, m.len(), e.path()))
            })
            .collect())
    }

    /// 合計が上限を超えていたら、最終アクセスが古いものから削除する。
    pub fn evict(&self) -> Result<()> {
        let mut files = self.list()?;
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        files.sort_by_key(|f| f.0);
        for (_, len, path) in files {
            if total <= self.max_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
        self.approx_bytes.store(total, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bmp(v: u8) -> Bitmap {
        Bitmap::from_rgba(Size::new(8, 8), vec![v; 8 * 8 * 4])
    }

    #[test]
    fn roundtrip_lossless() {
        let dir = tempfile::tempdir().unwrap();
        let c = DiskCache::open(dir.path(), 1 << 20).unwrap();
        assert!(c.get(1).is_none());
        c.put(1, &bmp(77)).unwrap();
        assert_eq!(c.get(1).unwrap(), bmp(77));
        assert!(dir.path().join(".nomedia").exists());
    }

    #[test]
    fn evicts_oldest_when_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        let c = DiskCache::open(dir.path(), 0).unwrap();
        c.put(1, &bmp(1)).unwrap();
        c.put(2, &bmp(2)).unwrap();
        // 上限 0 なのでどちらも残らない
        assert!(c.get(1).is_none() && c.get(2).is_none());
    }

    fn total_bytes(dir: &Path) -> u64 {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "webp"))
            .map(|e| e.metadata().unwrap().len())
            .sum()
    }

    #[test]
    fn evicts_down_to_limit_across_many_puts() {
        let dir = tempfile::tempdir().unwrap();
        let one = {
            let c = DiskCache::open(dir.path(), 1 << 20).unwrap();
            c.put(1, &bmp(1)).unwrap();
            total_bytes(dir.path())
        };
        let c = DiskCache::open(dir.path(), one * 5).unwrap();
        for k in 2..40u64 {
            c.put(k, &bmp(k as u8)).unwrap();
        }
        // 上限を大きく超えて溜まらない（見積もりで整理が走っている）
        assert!(
            total_bytes(dir.path()) <= one * 6,
            "{}",
            total_bytes(dir.path())
        );
        // 直近に入れたものは残る
        assert!(c.get(39).is_some());
    }

    #[test]
    fn existing_files_count_toward_the_limit_after_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let big = DiskCache::open(dir.path(), 1 << 20).unwrap();
        for k in 1..=3u64 {
            big.put(k, &bmp(k as u8)).unwrap();
        }
        let one = total_bytes(dir.path()) / 3;
        let c = DiskCache::open(dir.path(), one * 2).unwrap();
        c.put(4, &bmp(4)).unwrap();
        assert!(
            total_bytes(dir.path()) <= one * 2,
            "{}",
            total_bytes(dir.path())
        );
    }

    #[test]
    fn thumbnail_of_folder_fits_edge_and_keeps_aspect() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(400, 200, image::Rgba([9, 9, 9, 255]))
            .save(dir.path().join("001.png"))
            .unwrap();
        let t = make_thumbnail(dir.path(), 100).unwrap();
        assert_eq!(t.size, Size::new(100, 50));
        // 小さい画像は拡大しない
        let t = make_thumbnail(dir.path(), 1000).unwrap();
        assert_eq!(t.size, Size::new(400, 200));
    }

    #[test]
    fn thumbnail_of_empty_folder_is_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(make_thumbnail(dir.path(), 64).is_err());
    }

    #[test]
    fn corrupt_file_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let c = DiskCache::open(dir.path(), 1 << 20).unwrap();
        fs::write(c.path(5), b"garbage").unwrap();
        assert!(c.get(5).is_none());
    }
}
