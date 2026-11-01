//! Kotlin(Android) から呼ぶための UniFFI 窓口。ロジックは pv-core / pv-library にあり、ここは薄い変換層。

mod core;
mod fd;
mod reader;
mod tree;
mod types;

pub use core::{is_image_file_name, PvCore};
pub use reader::PvReader;
pub use tree::{PvTree, PvTreeEntry};
pub use types::*;

uniffi::setup_scaffolding!();

#[cfg(test)]
mod tests;
