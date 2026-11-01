//! L3: ディスク上のサムネイルキャッシュ（WebP）。アプリ専用領域に置き `.nomedia` を配置する。

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
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
}

impl DiskCache {
    pub fn open(dir: &Path, max_bytes: u64) -> Result<Self> {
        fs::create_dir_all(dir)?;
        let nomedia = dir.join(".nomedia");
        if !nomedia.exists() {
            fs::write(nomedia, b"")?;
        }
        Ok(Self {
            dir: dir.to_owned(),
            max_bytes,
        })
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
        let tmp = self.dir.join(format!("{key:016x}.tmp"));
        fs::write(&tmp, buf.into_inner())?;
        fs::rename(&tmp, self.path(key))?;
        self.evict()
    }

    /// 合計が上限を超えていたら、最終アクセスが古いものから削除する。
    pub fn evict(&self) -> Result<()> {
        let mut files: Vec<(SystemTime, u64, PathBuf)> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "webp"))
            .filter_map(|e| {
                let m = e.metadata().ok()?;
                Some((m.modified().ok()?, m.len(), e.path()))
            })
            .collect();
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
