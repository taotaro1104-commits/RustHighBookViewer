//! 書庫・フォルダを「名前付きエントリの順序付き集合」として扱う抽象層。

mod container;
mod entry;
mod epub;
mod error;
mod extract;
mod folder;
mod font;
mod path;
mod rar;
mod scan;
mod sevenz;
mod source;
mod text_epub;
mod text_layout;
mod text_style;
mod text_vertical;
mod zip_container;

pub use container::{
    open_path, open_source, open_source_in, open_source_styled, open_source_with, Container,
    ContainerProvider, Limits, Probe, TocEntry,
};
pub use entry::{kind_from_path, natural_cmp, EntryId, EntryKind, EntryMeta};
pub use epub::EpubContainer;
pub use error::{ContainerError, Result};
pub use extract::{ExtractGuard, ExtractedContainer, Extractor};
pub use folder::FolderContainer;
pub use font::has_typeface;
pub use path::{decode_name, normalize_path};
pub use rar::RarProvider;
pub use scan::{scan_books, ScanItem, ScanKind};
pub use sevenz::SevenZProvider;
pub use source::{FileSource, MemorySource, Source, SourceReader};
pub use text_epub::TextEpubContainer;
pub use text_style::{
    TextStyle, Typeface, DEFAULT_FONT_PX, DEFAULT_INK, DEFAULT_LINE_SPACING, DEFAULT_PAPER,
    MAX_FONT_PX, MAX_LETTER_SPACING, MAX_LINE_SPACING, MIN_FONT_PX, MIN_LETTER_SPACING,
    MIN_LINE_SPACING,
};
pub use zip_container::{open_nested, ZipContainer, ZipProvider};
