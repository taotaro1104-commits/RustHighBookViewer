//! Kotlin 側へ公開する型。コアの型との変換もここに集める。

use pv_core::{Action, ColorAdjust, Direction, FitMode, Settings, ViewMode};
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

    /// 「本を開く」で起きたエラーを、利用者に原因が伝わる日本語にして包む。
    pub fn open(e: impl std::error::Error + 'static) -> Self {
        PvError::Failed {
            msg: describe_error(&e),
        }
    }
}

/// エラーの原因を日本語で説明する。分からないものは元のメッセージを添える。
pub fn describe_error(e: &(dyn std::error::Error + 'static)) -> String {
    use pv_container::ContainerError;
    use pv_core::CoreError;
    if let Some(c) = e.downcast_ref::<CoreError>() {
        return match c {
            CoreError::Container(inner) => describe_error(inner),
            CoreError::Io(inner) => describe_error(inner),
            CoreError::NoPages => "画像が見つかりません".into(),
            CoreError::Cancelled => "キャンセルしました".into(),
            other => format!("開けませんでした（{other}）"),
        };
    }
    if let Some(c) = e.downcast_ref::<ContainerError>() {
        return match c {
            ContainerError::Io(inner) => describe_error(inner),
            ContainerError::Unsupported => {
                "対応していない形式です（ZIP/CBZ・7z・RAR・EPUB・フォルダに対応）".into()
            }
            ContainerError::Cancelled => "キャンセルしました".into(),
            ContainerError::LimitExceeded(what) => match *what {
                "entry count" => "ファイルが多すぎて開けません（安全のための上限）",
                "entry size" => "大きすぎるファイルが含まれています（安全のための上限）",
                "total size" => "展開後の合計サイズが大きすぎます（安全のための上限）",
                "archive size" => "書庫が大きすぎます（2GB まで）",
                "disk space" => "端末の空き容量が足りません",
                "compression ratio" => "圧縮率が異常なため開きません（安全のため）",
                _ => "安全のための上限を超えています",
            }
            .into(),
            ContainerError::Zip(inner) => format!("書庫が壊れているようです（{inner}）"),
            ContainerError::Archive(inner) => {
                format!(
                    "書庫を読めません。壊れているか、パスワード付きの可能性があります（{inner}）"
                )
            }
            ContainerError::NotFound(_) => "書庫の中身が見つかりません".into(),
        };
    }
    if let Some(io) = e.downcast_ref::<std::io::Error>() {
        return match io.kind() {
            std::io::ErrorKind::PermissionDenied => {
                "アクセス権限がありません。設定でファイルへのアクセスを許可してください".into()
            }
            std::io::ErrorKind::NotFound => "ファイルまたはフォルダが見つかりません".into(),
            _ => format!("読み込みに失敗しました（{io}）"),
        };
    }
    e.to_string()
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

/// フォルダ一括読み込みで見つかった 1 冊
#[derive(Debug, Clone, uniffi::Record)]
pub struct PvScanItem {
    pub path: String,
    pub title: String,
    /// true = フォルダ直下の画像をまとめた本、false = 書庫ファイル
    pub is_folder: bool,
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

/// 全体の既定設定。まだ開いていない本に使われる（開いたことのある本は、本ごとの設定が優先される）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct PvSettings {
    pub mode: PvViewMode,
    pub direction: PvDirection,
    /// 先頭ページ（表紙）を単独で表示する
    pub first_page_single: bool,
    /// 横長のページは単独で表示する
    pub wide_page_alone: bool,
    pub fit: PvFit,
    /// 画像キャッシュの上限（MB）。0 なら端末メモリの 1/8
    pub cache_mb: u32,
    /// 先読みするスプレッド数（前・後ろ）
    pub prefetch_back: u32,
    pub prefetch_forward: u32,
}

pub const MAX_PREFETCH: u32 = 12;
pub const MAX_CACHE_MB: u32 = 4096;

impl From<&Settings> for PvSettings {
    fn from(s: &Settings) -> Self {
        Self {
            mode: s.layout.mode.into(),
            direction: s.layout.direction.into(),
            first_page_single: s.layout.first_page_single,
            wide_page_alone: s.layout.wide_page_alone,
            fit: s.fit.into(),
            cache_mb: s.cache.l1_bytes.map_or(0, |b| (b >> 20) as u32),
            prefetch_back: s.cache.prefetch_back as u32,
            prefetch_forward: s.cache.prefetch_forward as u32,
        }
    }
}

impl PvSettings {
    /// 設定に反映する。範囲外の値は収める（UI や設定ファイルの不正値対策）。
    pub fn apply_to(self, s: &Settings) -> Settings {
        let mut out = s.clone();
        out.layout.mode = self.mode.into();
        out.layout.direction = self.direction.into();
        out.layout.first_page_single = self.first_page_single;
        out.layout.wide_page_alone = self.wide_page_alone;
        out.fit = self.fit.into();
        out.cache.l1_bytes = match self.cache_mb {
            0 => None,
            mb => Some(u64::from(mb.clamp(64, MAX_CACHE_MB)) << 20),
        };
        out.cache.prefetch_back = self.prefetch_back.min(MAX_PREFETCH) as usize;
        out.cache.prefetch_forward = self.prefetch_forward.min(MAX_PREFETCH) as usize;
        out
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
    fn errors_are_described_in_japanese() {
        use pv_container::ContainerError;
        use pv_core::CoreError;
        let d = |e: &(dyn std::error::Error + 'static)| describe_error(e);
        assert!(d(&CoreError::NoPages).contains("画像が見つかりません"));
        assert!(d(&ContainerError::Unsupported).contains("対応していない形式"));
        assert!(d(&ContainerError::LimitExceeded("entry count")).contains("多すぎ"));
        assert!(d(&ContainerError::LimitExceeded("disk space")).contains("空き容量"));
        // 入れ子（CoreError → ContainerError → io::Error）の権限エラーまで届く
        let perm = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let nested = CoreError::Container(ContainerError::Io(perm));
        assert!(d(&nested).contains("アクセス権限"));
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(d(&missing).contains("見つかりません"));
    }

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
