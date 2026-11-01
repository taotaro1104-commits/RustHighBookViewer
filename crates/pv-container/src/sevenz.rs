use std::path::PathBuf;
use std::sync::Arc;

use sevenz_rust2::{ArchiveReader, Password};

use crate::container::{Container, ContainerProvider, Limits, Probe};
use crate::error::{ContainerError, Result};
use crate::extract::{ExtractGuard, ExtractedContainer};
use crate::source::{Source, SourceReader};

const MAGIC: &[u8; 6] = b"7z\xBC\xAF\x27\x1C";

pub struct SevenZProvider {
    pub scratch: PathBuf,
    pub limits: Limits,
    pub guard: ExtractGuard,
}

impl ContainerProvider for SevenZProvider {
    fn probe(&self, src: &dyn Source) -> Probe {
        let mut head = [0u8; 6];
        match src.read_at(0, &mut head) {
            Ok(6) if &head == MAGIC => Probe::Match,
            _ => Probe::NoMatch,
        }
    }

    fn open(&self, src: Arc<dyn Source>) -> Result<Arc<dyn Container>> {
        let mut reader = ArchiveReader::new(SourceReader::new(src), Password::empty())
            .map_err(|e| ContainerError::Archive(e.to_string()))?;
        let c = ExtractedContainer::build(&self.scratch, self.limits, &self.guard, |ex| {
            // 上限超過などの自前エラーは、ライブラリ越しに失われないよう別枠で保持する
            let mut failure: Option<ContainerError> = None;
            let res = reader.for_each_entries(|entry, data| {
                if entry.is_directory() {
                    return Ok(true);
                }
                match ex.add(entry.name(), data) {
                    Ok(()) => Ok(true),
                    Err(e) => {
                        failure = Some(e);
                        Ok(false)
                    }
                }
            });
            match failure {
                Some(e) => Err(e),
                None => res.map_err(|e| ContainerError::Archive(e.to_string())),
            }
        })?;
        Ok(Arc::new(c))
    }
}
