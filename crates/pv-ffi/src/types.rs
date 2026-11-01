//! Kotlin 側へ公開する型。コアの型との変換もここに集める。

use pv_core::{Action, ColorAdjust, Direction, FitMode, ViewMode};
use pv_library::{Book, ReadStatus};

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PvError {
    #[error("{msg}")]
    Failed { msg: String },
}

impl PvError {
    pub fn new(e: impl std::fmt::Display) -> Self {
        PvError::Failed { msg: e.to_string() }
    }
}

pub type PvResult<T> = Result<T, PvError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvViewMode {
    Single,
    Double,
    Auto,
    Webtoon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvDirection {
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvFit {
    Width,
    Height,
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvStatus {
    Unread,
    Reading,
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvSort {
    Name,
    LastOpened,
    Added,
    Size,
}

/// UI が直接扱う操作。それ以外は Other にまとめる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvAction {
    NextSpread,
    PrevSpread,
    NextPage,
    PrevPage,
    FirstPage,
    LastPage,
    ShiftSpread,
    FitWidth,
    FitHeight,
    FitPage,
    ZoomIn,
    ZoomOut,
    AddBookmark,
    OpenThumbnails,
    SavePage,
    ToggleMagnifier,
    Other,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PvBook {
    pub id: i64,
    pub uri: String,
    pub title: String,
    pub page_count: u32,
    pub status: PvStatus,
    pub favorite: bool,
    pub last_opened_at: Option<i64>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PvThumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PvFrame {
    pub ticket: u64,
    pub spread: u32,
    pub width: u32,
    pub height: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub scale: f32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum PvEvent {
    Frame { frame: PvFrame },
    Cancelled { ticket: u64 },
    Failed { ticket: u64, message: String },
}

/// 色調整。各値は -100..=100（0 が無調整）。範囲外は取り込み時に収める。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, uniffi::Record)]
pub struct PvColor {
    pub brightness: i32,
    pub contrast: i32,
    pub gamma: i32,
    pub grayscale: bool,
    pub sepia: bool,
    pub invert: bool,
}

impl From<PvColor> for ColorAdjust {
    fn from(c: PvColor) -> Self {
        let n = |v: i32| v.clamp(-100, 100) as i8;
        ColorAdjust {
            brightness: n(c.brightness),
            contrast: n(c.contrast),
            gamma: n(c.gamma),
            grayscale: c.grayscale,
            sepia: c.sepia,
            invert: c.invert,
        }
    }
}

impl From<ColorAdjust> for PvColor {
    fn from(c: ColorAdjust) -> Self {
        Self {
            brightness: c.brightness as i32,
            contrast: c.contrast as i32,
            gamma: c.gamma as i32,
            grayscale: c.grayscale,
            sepia: c.sepia,
            invert: c.invert,
        }
    }
}

#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct PvLayout {
    pub mode: PvViewMode,
    pub direction: PvDirection,
    pub shift: bool,
    pub fit: PvFit,
}

impl From<ViewMode> for PvViewMode {
    fn from(m: ViewMode) -> Self {
        match m {
            ViewMode::Single => Self::Single,
            ViewMode::Double => Self::Double,
            ViewMode::Auto => Self::Auto,
            ViewMode::Webtoon => Self::Webtoon,
        }
    }
}

impl From<PvViewMode> for ViewMode {
    fn from(m: PvViewMode) -> Self {
        match m {
            PvViewMode::Single => Self::Single,
            PvViewMode::Double => Self::Double,
            PvViewMode::Auto => Self::Auto,
            PvViewMode::Webtoon => Self::Webtoon,
        }
    }
}

impl From<Direction> for PvDirection {
    fn from(d: Direction) -> Self {
        match d {
            Direction::LeftToRight => Self::LeftToRight,
            Direction::RightToLeft => Self::RightToLeft,
        }
    }
}

impl From<PvDirection> for Direction {
    fn from(d: PvDirection) -> Self {
        match d {
            PvDirection::LeftToRight => Self::LeftToRight,
            PvDirection::RightToLeft => Self::RightToLeft,
        }
    }
}

impl From<FitMode> for PvFit {
    fn from(f: FitMode) -> Self {
        match f {
            FitMode::Width => Self::Width,
            FitMode::Height => Self::Height,
            FitMode::Page => Self::Page,
        }
    }
}

impl From<PvFit> for FitMode {
    fn from(f: PvFit) -> Self {
        match f {
            PvFit::Width => Self::Width,
            PvFit::Height => Self::Height,
            PvFit::Page => Self::Page,
        }
    }
}

impl From<ReadStatus> for PvStatus {
    fn from(s: ReadStatus) -> Self {
        match s {
            ReadStatus::Unread => Self::Unread,
            ReadStatus::Reading => Self::Reading,
            ReadStatus::Finished => Self::Finished,
        }
    }
}

impl From<&Book> for PvBook {
    fn from(b: &Book) -> Self {
        Self {
            id: b.id,
            uri: b.uri.clone(),
            title: b.title.clone(),
            page_count: b.page_count as u32,
            status: b.status.into(),
            favorite: b.favorite,
            last_opened_at: b.last_opened_at,
        }
    }
}

impl From<&Action> for PvAction {
    fn from(a: &Action) -> Self {
        match a {
            Action::NextSpread => Self::NextSpread,
            Action::PrevSpread => Self::PrevSpread,
            Action::NextPage => Self::NextPage,
            Action::PrevPage => Self::PrevPage,
            Action::FirstPage => Self::FirstPage,
            Action::LastPage => Self::LastPage,
            Action::ShiftSpread => Self::ShiftSpread,
            Action::FitWidth => Self::FitWidth,
            Action::FitHeight => Self::FitHeight,
            Action::FitPage => Self::FitPage,
            Action::ZoomIn => Self::ZoomIn,
            Action::ZoomOut => Self::ZoomOut,
            Action::AddBookmark => Self::AddBookmark,
            Action::OpenThumbnails => Self::OpenThumbnails,
            Action::SavePage => Self::SavePage,
            Action::ToggleMagnifier => Self::ToggleMagnifier,
            _ => Self::Other,
        }
    }
}

impl PvAction {
    /// コアの Action に戻す。コアで処理しない操作は None。
    pub fn to_core(self) -> Option<Action> {
        Some(match self {
            Self::NextSpread => Action::NextSpread,
            Self::PrevSpread => Action::PrevSpread,
            Self::NextPage => Action::NextPage,
            Self::PrevPage => Action::PrevPage,
            Self::FirstPage => Action::FirstPage,
            Self::LastPage => Action::LastPage,
            Self::ShiftSpread => Action::ShiftSpread,
            Self::FitWidth => Action::FitWidth,
            Self::FitHeight => Action::FitHeight,
            Self::FitPage => Action::FitPage,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_actions_roundtrip() {
        for a in [
            PvAction::NextSpread,
            PvAction::PrevPage,
            PvAction::LastPage,
            PvAction::ShiftSpread,
            PvAction::FitWidth,
        ] {
            let core = a.to_core().expect("core action");
            assert_eq!(PvAction::from(&core), a);
        }
        assert_eq!(PvAction::ZoomIn.to_core(), None);
        assert_eq!(PvAction::from(&Action::RotateView), PvAction::Other);
    }

    #[test]
    fn color_roundtrip_and_clamp() {
        let c = PvColor {
            brightness: 30,
            contrast: -20,
            gamma: 5,
            grayscale: true,
            sepia: false,
            invert: true,
        };
        assert_eq!(PvColor::from(ColorAdjust::from(c)), c);
        let wild = PvColor {
            brightness: 9999,
            contrast: -9999,
            ..Default::default()
        };
        let a = ColorAdjust::from(wild);
        assert_eq!((a.brightness, a.contrast), (100, -100));
    }

    #[test]
    fn layout_enums_roundtrip() {
        for m in [
            ViewMode::Single,
            ViewMode::Double,
            ViewMode::Auto,
            ViewMode::Webtoon,
        ] {
            assert_eq!(ViewMode::from(PvViewMode::from(m)), m);
        }
        for f in [FitMode::Width, FitMode::Height, FitMode::Page] {
            assert_eq!(FitMode::from(PvFit::from(f)), f);
        }
    }
}
