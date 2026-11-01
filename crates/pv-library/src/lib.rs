//! 本棚・履歴・しおり・読書位置の永続化（SQLite）。

mod fingerprint;
mod library;
mod model;
mod schema;

pub use fingerprint::{fingerprint, BookKey, HEAD_LEN};
pub use library::{now_ms, Library};
pub use model::*;

#[cfg(test)]
mod tests;
