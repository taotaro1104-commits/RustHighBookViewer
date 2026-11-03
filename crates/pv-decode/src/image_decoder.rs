use std::io::Cursor;

use image::ImageReader;

use crate::{Bitmap, DecodeError, Decoder, Format, Result, Size};

const FORMATS: [Format; 5] = [
    Format::Jpeg,
    Format::Png,
    Format::WebP,
    Format::Gif,
    Format::Bmp,
];

/// `image` クレートによる汎用デコーダ。
pub struct ImageDecoder {
    /// これを超える画素数はデコードせず拒否する（OOM / decompression bomb 対策）
    pub max_pixels: u64,
}

impl Default for ImageDecoder {
    fn default() -> Self {
        Self {
            max_pixels: 256_000_000,
        }
    }
}

/// 目標サイズを下回らない最大の縮小倍率（1/2, 1/4, 1/8）。
fn scale_factor(src: Size, target: Size) -> u32 {
    [8u32, 4, 2]
        .into_iter()
        .find(|f| src.w / f >= target.w && src.h / f >= target.h)
        .unwrap_or(1)
}

fn reader(data: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>> {
    let r = ImageReader::new(Cursor::new(data)).with_guessed_format()?;
    match r.format() {
        Some(image::ImageFormat::Jpeg)
        | Some(image::ImageFormat::Png)
        | Some(image::ImageFormat::WebP)
        | Some(image::ImageFormat::Gif)
        | Some(image::ImageFormat::Bmp) => Ok(r),
        _ => Err(DecodeError::Unsupported),
    }
}

impl Decoder for ImageDecoder {
    fn formats(&self) -> &[Format] {
        &FORMATS
    }

    fn probe_size(&self, data: &[u8]) -> Result<Size> {
        let (w, h) = reader(data)?.into_dimensions()?;
        Ok(Size::new(w, h))
    }

    fn decode(&self, data: &[u8], target: Option<Size>) -> Result<Bitmap> {
        let size = self.probe_size(data)?;
        if size.pixels() > self.max_pixels {
            return Err(DecodeError::TooLarge(size.pixels()));
        }
        // JPEG は DCT の段階で縮小しながら読む（原寸で展開して縮めるより数倍速い）
        if let Some(t) = target {
            let factor = scale_factor(size, t);
            if factor > 1 && is_jpeg(data) {
                if let Some(bitmap) = decode_jpeg_scaled(data, size, factor) {
                    return Ok(bitmap);
                }
            }
        }
        let rgba = reader(data)?.decode()?.into_rgba8();
        let bitmap = Bitmap::from_rgba(size, rgba.into_raw());
        Ok(match target {
            Some(t) => bitmap.downscale_box(scale_factor(size, t)),
            None => bitmap,
        })
    }
}

fn is_jpeg(data: &[u8]) -> bool {
    data.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// `1/factor` に縮めながら JPEG を読む。グレースケールと RGB だけ扱い、
/// CMYK などや読み込みに失敗したときは `None`（呼び出し側が通常の経路で読み直す）。
fn decode_jpeg_scaled(data: &[u8], size: Size, factor: u32) -> Option<Bitmap> {
    use jpeg_decoder::PixelFormat;

    let mut dec = jpeg_decoder::Decoder::new(Cursor::new(data));
    // `scale` は片方の軸が足りれば縮めるので、両軸が目標を下回らない倍率の出力寸法を渡す
    let want_w = u16::try_from(size.w.div_ceil(factor)).ok()?;
    let want_h = u16::try_from(size.h.div_ceil(factor)).ok()?;
    dec.scale(want_w, want_h).ok()?;
    let pixels = dec.decode().ok()?;
    let info = dec.info()?;
    let out = Size::new(u32::from(info.width), u32::from(info.height));
    let n = out.w as usize * out.h as usize;
    let rgba = match info.pixel_format {
        PixelFormat::RGB24 if pixels.len() == n * 3 => {
            let mut v = Vec::with_capacity(n * 4);
            for &[r, g, bl] in pixels.as_chunks::<3>().0 {
                v.extend_from_slice(&[r, g, bl, 255]);
            }
            v
        }
        PixelFormat::L8 if pixels.len() == n => {
            pixels.iter().flat_map(|&g| [g, g, g, 255]).collect()
        }
        _ => return None,
    };
    Some(Bitmap::from_rgba(out, rgba))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba, RgbaImage};

    fn encode(w: u32, h: u32, fmt: ImageFormat) -> Vec<u8> {
        let img = RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255]));
        let mut buf = Cursor::new(Vec::new());
        // JPEG はアルファ非対応のため RGB に落とす
        if fmt == ImageFormat::Jpeg {
            image::DynamicImage::ImageRgba8(img)
                .to_rgb8()
                .write_to(&mut buf, fmt)
                .unwrap();
        } else {
            img.write_to(&mut buf, fmt).unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn probes_and_decodes_each_format() {
        let d = ImageDecoder::default();
        for fmt in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::Bmp,
        ] {
            let data = encode(16, 8, fmt);
            assert_eq!(d.probe_size(&data).unwrap(), Size::new(16, 8), "{fmt:?}");
            let bmp = d.decode(&data, None).unwrap();
            assert_eq!(bmp.size, Size::new(16, 8), "{fmt:?}");
            assert_eq!(bmp.data.len(), 16 * 8 * 4);
        }
    }

    #[test]
    fn decode_with_target_downscales_but_not_below_target() {
        let d = ImageDecoder::default();
        let data = encode(64, 32, ImageFormat::Png);
        let b = d.decode(&data, Some(Size::new(16, 8))).unwrap();
        assert_eq!(b.size, Size::new(16, 8)); // 1/4
        let b = d.decode(&data, Some(Size::new(20, 10))).unwrap();
        assert_eq!(b.size, Size::new(32, 16)); // 1/8,1/4 は目標未満になるので 1/2
    }

    fn encode_jpeg_gradient(w: u32, h: u32, gray: bool) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 90])
        });
        let mut buf = Cursor::new(Vec::new());
        if gray {
            image::DynamicImage::ImageRgb8(img)
                .to_luma8()
                .write_to(&mut buf, ImageFormat::Jpeg)
                .unwrap();
        } else {
            img.write_to(&mut buf, ImageFormat::Jpeg).unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn jpeg_with_target_is_decoded_scaled_and_keeps_colors() {
        let d = ImageDecoder::default();
        let data = encode_jpeg_gradient(640, 480, false);
        let b = d.decode(&data, Some(Size::new(160, 120))).unwrap();
        assert_eq!(b.size, Size::new(160, 120)); // 1/4
        assert_eq!(b.data.len(), 160 * 120 * 4);
        // 左上は暗い赤・緑、右下は明るい。青は 90 前後のまま
        let tl = &b.row(0)[..4];
        let br = &b.row(119)[159 * 4..160 * 4];
        assert!(tl[0] < 20 && tl[1] < 20, "{tl:?}");
        assert!(br[0] > 230 && br[1] > 230, "{br:?}");
        assert!((i32::from(tl[2]) - 90).abs() < 12 && tl[3] == 255);
    }

    #[test]
    fn grayscale_jpeg_is_decoded_scaled() {
        let d = ImageDecoder::default();
        let data = encode_jpeg_gradient(320, 200, true);
        let b = d.decode(&data, Some(Size::new(80, 50))).unwrap();
        assert_eq!(b.size, Size::new(80, 50));
        let px = &b.row(10)[8..12];
        assert!(px[0] == px[1] && px[1] == px[2] && px[3] == 255);
    }

    #[test]
    fn jpeg_never_goes_below_target_on_either_axis() {
        let d = ImageDecoder::default();
        let data = encode_jpeg_gradient(1000, 300, false); // 横長
        let b = d.decode(&data, Some(Size::new(200, 150))).unwrap();
        assert!(b.size.w >= 200 && b.size.h >= 150, "{:?}", b.size);
    }

    #[test]
    fn corrupt_jpeg_body_is_error_not_panic() {
        let d = ImageDecoder::default();
        let mut data = encode_jpeg_gradient(256, 256, false);
        data.truncate(data.len() / 3);
        let _ = d.decode(&data, Some(Size::new(32, 32))); // どちらでもよい（落ちないこと）
    }

    /// 計測用（`cargo test -p pv-decode --release -- --ignored --nocapture`）
    #[test]
    #[ignore]
    fn bench_scaled_vs_full_decode() {
        use std::time::Instant;
        let (w, h) = (2400u32, 3400u32);
        // 実物の写真に近いよう、ノイズ混じりの画像にする
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let n = ((x * 7919 + y * 104_729) % 3) as u8;
            image::Rgb([
                (x * 255 / w) as u8 ^ n,
                (y * 255 / h) as u8 ^ n,
                n.wrapping_mul(3),
            ])
        });
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, ImageFormat::Jpeg).unwrap();
        let data = buf.into_inner();
        let dest = Size::new(240, 340);
        let target = Some(dest);
        let d = ImageDecoder::default();
        let t = Instant::now();
        for _ in 0..10 {
            d.decode(&data, target).unwrap();
        }
        let scaled = t.elapsed() / 10;
        let t = Instant::now();
        for _ in 0..10 {
            let rgba = reader(&data).unwrap().decode().unwrap().into_rgba8();
            Bitmap::from_rgba(Size::new(w, h), rgba.into_raw())
                .downscale_box(scale_factor(Size::new(w, h), dest));
        }
        let full = t.elapsed() / 10;
        println!(
            "scaled: {scaled:?}  full+box: {full:?}  ({} KB jpeg)",
            data.len() / 1024
        );
    }

    #[test]
    fn rejects_unknown_data() {
        let d = ImageDecoder::default();
        assert!(matches!(
            d.probe_size(b"definitely not an image"),
            Err(DecodeError::Unsupported)
        ));
        assert!(d.decode(&[], None).is_err());
    }

    #[test]
    fn rejects_oversized_images() {
        let d = ImageDecoder { max_pixels: 100 };
        let data = encode(20, 20, ImageFormat::Png);
        assert!(matches!(
            d.decode(&data, None),
            Err(DecodeError::TooLarge(400))
        ));
    }

    #[test]
    fn truncated_data_is_error_not_panic() {
        let d = ImageDecoder::default();
        let mut data = encode(32, 32, ImageFormat::Png);
        data.truncate(data.len() / 2);
        assert!(d.decode(&data, None).is_err());
    }
}
