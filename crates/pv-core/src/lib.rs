//! UI から見える唯一の窓口。BookSession・設定・操作マップ・キャッシュ・ジョブキューを提供する。

mod cache;
mod error;
mod frame;
mod input;
mod jobs;
mod render;
mod session;
mod settings;
mod thumb;

pub use cache::{ByteLru, PageKey};
pub use error::{CoreError, Result};
pub use frame::{CoreEvent, Frame, FrameTicket};
pub use input::{tap_zone, Action, Binding, ColorFilter, Dir, InputKey, InputMap};
pub use pv_container::{open_source, open_source_in, Container, FileSource, Source};
pub use session::{BookSession, PageThumbs};
pub use settings::{BookOverride, CacheSettings, Settings, SCHEMA_VERSION};
pub use thumb::{make_thumbnail, make_thumbnail_from, DiskCache};

pub use pv_decode::{Bitmap, Size};
pub use pv_layout::{Direction, LayoutOptions, Spread, ViewMode};
pub use pv_process::ColorAdjust;
pub use pv_render::FitMode;
