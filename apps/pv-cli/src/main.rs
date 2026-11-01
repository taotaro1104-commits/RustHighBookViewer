//! pv-cli: フォルダ / ZIP の画像を PNG として書き出す（Phase 0 の動作確認用）。
//!
//! 使い方: pv-cli <書庫またはフォルダ> <出力ディレクトリ>

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pv_container::{open_path, EntryKind};
use pv_decode::{Decoder, ImageDecoder};

fn run(src: &Path, out_dir: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let container = open_path(src)?;
    let decoder = ImageDecoder::default();
    std::fs::create_dir_all(out_dir)?;

    let mut written = 0;
    for entry in container
        .entries()
        .iter()
        .filter(|e| e.kind == EntryKind::Image)
    {
        let data = container.read(entry.id)?;
        let bitmap = match decoder.decode(&data, None) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {}: {e}", entry.path);
                continue;
            }
        };
        let out = out_dir.join(format!("{:04}.png", written + 1));
        image::save_buffer(
            &out,
            &bitmap.data,
            bitmap.size.w,
            bitmap.size.h,
            image::ColorType::Rgba8,
        )?;
        println!("{} -> {}", entry.path, out.display());
        written += 1;
    }
    Ok(written)
}

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [src, out] = args.as_slice() else {
        eprintln!("usage: pv-cli <archive-or-folder> <output-dir>");
        return ExitCode::from(2);
    };
    match run(src, out) {
        Ok(n) => {
            println!("{n} page(s) written");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
