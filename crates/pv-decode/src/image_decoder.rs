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
        let rgba = reader(data)?.decode()?.into_rgba8();
        let bitmap = Bitmap::from_rgba(size, rgba.into_raw());
        Ok(match target {
            Some(t) => bitmap.downscale_box(scale_factor(size, t)),
            None => bitmap,
        })
    }
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
