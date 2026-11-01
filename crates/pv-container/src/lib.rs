//! 書庫・フォルダを「名前付きエントリの順序付き集合」として扱う抽象層。

mod container;
mod entry;
mod epub;
mod error;
mod extract;
mod folder;
mod path;
mod rar;
mod sevenz;
mod source;
mod zip_container;

pub use container::{
    open_path, open_source, open_source_in, open_source_with, Container, ContainerProvider, Limits,
    Probe,
};
pub use entry::{kind_from_path, natural_cmp, EntryId, EntryKind, EntryMeta};
pub use epub::EpubContainer;
pub use error::{ContainerError, Result};
pub use extract::{ExtractGuard, ExtractedContainer, Extractor};
pub use folder::FolderContainer;
pub use path::{decode_name, normalize_path};
pub use rar::RarProvider;
pub use sevenz::SevenZProvider;
pub use source::{FileSource, MemorySource, Source, SourceReader};
pub use zip_container::{open_nested, ZipContainer, ZipProvider};
