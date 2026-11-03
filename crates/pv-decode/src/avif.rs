//! AVIF のデコード（純 Rust: avif-parse + rav1d）。`image` クレートの対応外なので、ここで受け持つ。

#[cfg(not(target_arch = "arm"))]
use avif_decode::Image;

use crate::{Bitmap, DecodeError, Result, Size};

/// 先頭が ISO BMFF の `ftyp` で、`avif` / `avis` のブランドを含めば AVIF とみなす。
pub fn is_avif(data: &[u8]) -> bool {
    if data.len() < 16 || &data[4..8] != b"ftyp" {
        return false;
    }
    let ftyp_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let end = ftyp_len.clamp(16, data.len().min(256));
    // major brand（8..12）と、その後の互換ブランド（16.. から 4 バイトずつ）
    std::iter::once(&data[8..12])
        .chain(data[16..end].as_chunks::<4>().0.iter().map(|c| &c[..]))
        .any(|b| b == b"avif" || b == b"avis")
}

/// ヘッダの `ispe`（画像の寸法）から、サムネイルや補助画像を除いた最大の寸法を返す。デコードはしない。
pub fn probe_size(data: &[u8]) -> Option<Size> {
    let mut best: Option<Size> = None;
    let mut from = 0;
    // メタデータは先頭にあるので、探す範囲を限る（巨大なファイル全体を走査しない）
    let head = &data[..data.len().min(1 << 20)];
    while let Some(pos) = head[from..].windows(4).position(|w| w == b"ispe") {
        let at = from + pos + 4; // version/flags(4) の次に width(4), height(4)
        from += pos + 4;
        let Some(f) = head.get(at + 4..at + 12) else {
            break;
        };
        let (w, h) = (
            u32::from_be_bytes([f[0], f[1], f[2], f[3]]),
            u32::from_be_bytes([f[4], f[5], f[6], f[7]]),
        );
        if w > 0 && h > 0 && best.is_none_or(|b| Size::new(w, h).pixels() > b.pixels()) {
            best = Some(Size::new(w, h));
        }
    }
    best
}

#[cfg(not(target_arch = "arm"))]
fn avif_err(e: impl std::fmt::Display) -> DecodeError {
    DecodeError::Avif(e.to_string())
}

/// 32 ビット ARM では AVIF のデコーダ（rav1d）をビルドできないため、読めない旨を返す。
#[cfg(target_arch = "arm")]
pub fn decode(_data: &[u8]) -> Result<Bitmap> {
    Err(DecodeError::Avif(
        "AVIF is not supported on 32-bit ARM devices".into(),
    ))
}

/// 原寸の RGBA8 にデコードする（16 ビットは上位 8 ビットに落とす）。
#[cfg(not(target_arch = "arm"))]
pub fn decode(data: &[u8]) -> Result<Bitmap> {
    let img = avif_decode::Decoder::from_avif(data)
        .map_err(avif_err)?
        .to_image()
        .map_err(avif_err)?;
    let (size, rgba) = match img {
        Image::Rgb8(i) => convert(&i, |p| [p.r, p.g, p.b, 255]),
        Image::Rgba8(i) => convert(&i, |p| [p.r, p.g, p.b, p.a]),
        Image::Gray8(i) => convert(&i, |p| [p.value(), p.value(), p.value(), 255]),
        Image::Rgb16(i) => convert(&i, |p| [hi(p.r), hi(p.g), hi(p.b), 255]),
        Image::Rgba16(i) => convert(&i, |p| [hi(p.r), hi(p.g), hi(p.b), hi(p.a)]),
        Image::Gray16(i) => convert(&i, |p| {
            let v = hi(p.value());
            [v, v, v, 255]
        }),
    };
    Ok(Bitmap::from_rgba(size, rgba))
}

#[cfg(not(target_arch = "arm"))]
fn hi(v: u16) -> u8 {
    (v >> 8) as u8
}

#[cfg(not(target_arch = "arm"))]
fn convert<T: Copy>(img: &imgref::ImgVec<T>, px: impl Fn(T) -> [u8; 4]) -> (Size, Vec<u8>) {
    let size = Size::new(img.width() as u32, img.height() as u32);
    let mut out = Vec::with_capacity(img.width() * img.height() * 4);
    for row in img.rows() {
        for &p in row {
            out.extend_from_slice(&px(p));
        }
    }
    (size, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAD: &[u8] = include_bytes!("../testdata/grad64x48.avif");
    const GRAY: &[u8] = include_bytes!("../testdata/gray64x48.avif");
    const ALPHA: &[u8] = include_bytes!("../testdata/alpha64x48.avif");

    #[test]
    fn recognizes_avif_and_rejects_others() {
        assert!(is_avif(GRAD) && is_avif(GRAY) && is_avif(ALPHA));
        assert!(!is_avif(b"\x89PNG\r\n\x1a\n and more bytes here"));
        assert!(!is_avif(b"short"));
        // HEIC は ftyp だが AVIF ではない
        assert!(!is_avif(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic"));
    }

    #[test]
    fn probes_size_without_decoding() {
        for d in [GRAD, GRAY, ALPHA] {
            assert_eq!(probe_size(d), Some(Size::new(64, 48)));
        }
        assert_eq!(probe_size(b"no boxes in here"), None);
    }

    #[cfg(not(target_arch = "arm"))]
    #[test]
    fn decodes_color_image_close_to_the_source() {
        let b = decode(GRAD).unwrap();
        assert_eq!(b.size, Size::new(64, 48));
        assert_eq!(b.data.len(), 64 * 48 * 4);
        let tl = &b.row(0)[..4];
        let br = &b.row(47)[63 * 4..64 * 4];
        assert!(tl[0] < 40 && tl[1] < 40, "{tl:?}");
        assert!(br[0] > 215 && br[1] > 215, "{br:?}");
        assert!((i32::from(tl[2]) - 90).abs() < 30 && tl[3] == 255, "{tl:?}");
    }

    #[cfg(not(target_arch = "arm"))]
    #[test]
    fn decodes_grayscale_as_equal_channels() {
        let b = decode(GRAY).unwrap();
        assert_eq!(b.size, Size::new(64, 48));
        let px = &b.row(20)[40..44];
        assert!(px[0] == px[1] && px[1] == px[2] && px[3] == 255, "{px:?}");
    }

    #[cfg(not(target_arch = "arm"))]
    #[test]
    fn keeps_alpha_channel() {
        let b = decode(ALPHA).unwrap();
        let left = b.row(10)[4 * 4 + 3];
        let right = b.row(10)[60 * 4 + 3];
        assert!(left > 240, "{left}");
        assert!((100..160).contains(&right), "{right}");
    }

    #[test]
    fn truncated_avif_is_error_not_panic() {
        assert!(decode(&GRAD[..GRAD.len() / 2]).is_err());
        assert!(decode(&[]).is_err());
    }
}
