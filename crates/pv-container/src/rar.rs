use std::path::PathBuf;
use std::sync::Arc;

use unrar::Archive;

use crate::container::{Container, ContainerProvider, Limits, Probe};
use crate::error::{ContainerError, Result};
use crate::extract::{spill_to_file, ExtractedContainer};
use crate::source::Source;

/// RAR 4.x / 5.x 共通の先頭 6 バイト
const MAGIC: &[u8; 6] = b"Rar!\x1a\x07";

pub struct RarProvider {
    pub scratch: PathBuf,
    pub limits: Limits,
}

fn archive_err(e: unrar::error::UnrarError) -> ContainerError {
    ContainerError::Archive(e.to_string())
}

impl ContainerProvider for RarProvider {
    fn probe(&self, src: &dyn Source) -> Probe {
        let mut head = [0u8; 6];
        match src.read_at(0, &mut head) {
            Ok(6) if &head == MAGIC => Probe::Match,
            _ => Probe::NoMatch,
        }
    }

    fn open(&self, src: Arc<dyn Source>) -> Result<Arc<dyn Container>> {
        // unrar はファイルパスしか受け取れないため、いったん作業領域へ書き出す。
        // 書き出したファイルは展開が終わった時点で TempDir ごと消える。
        std::fs::create_dir_all(&self.scratch)?;
        let spill = tempfile::Builder::new()
            .prefix("pv-r-")
            .tempdir_in(&self.scratch)?;
        let path = spill_to_file(src.as_ref(), spill.path(), "src.rar")?;
        let c = ExtractedContainer::build(&self.scratch, self.limits, |ex| {
            let mut cursor = Archive::new(&path)
                .open_for_processing()
                .map_err(archive_err)?;
            while let Some(header) = cursor.read_header().map_err(archive_err)? {
                let (is_file, name, size) = {
                    let e = header.entry();
                    (
                        e.is_file(),
                        e.filename.to_string_lossy().into_owned(),
                        e.unpacked_size,
                    )
                };
                cursor = if is_file && size <= self.limits.max_entry_size {
                    let (data, next) = header.read().map_err(archive_err)?;
                    ex.add(&name, &mut data.as_slice())?;
                    next
                } else {
                    header.skip().map_err(archive_err)?
                };
            }
            Ok(())
        })?;
        Ok(Arc::new(c))
    }
}
