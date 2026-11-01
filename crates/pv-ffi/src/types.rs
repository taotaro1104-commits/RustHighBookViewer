//! Kotlin 側へ公開する型。コアの型との変換もここに集める。

use crate::i18n::{pick, Lang};
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

/// エラーの原因を、今選んでいる言語で説明する。分からないものは元のメッセージを添える。
pub fn describe_error(e: &(dyn std::error::Error + 'static)) -> String {
    describe_error_in(crate::i18n::lang(), e)
}

/// `describe_error` の言語指定版。
pub fn describe_error_in(lang: Lang, e: &(dyn std::error::Error + 'static)) -> String {
    use pv_container::ContainerError;
    use pv_core::CoreError;
    let t = |texts: [&str; 5]| pick(lang, texts, "");
    let td = |texts: [&str; 5], detail: &str| pick(lang, texts, detail);
    let cancelled = || {
        t([
            "キャンセルしました",
            "Cancelled",
            "已取消",
            "취소했습니다",
            "Cancelado",
        ])
    };
    if let Some(c) = e.downcast_ref::<CoreError>() {
        return match c {
            CoreError::Container(inner) => describe_error_in(lang, inner),
            CoreError::Io(inner) => describe_error_in(lang, inner),
            CoreError::NoPages => t([
                "画像が見つかりません",
                "No images found",
                "未找到图片",
                "이미지를 찾을 수 없습니다",
                "No se encontraron imágenes",
            ]),
            CoreError::Cancelled => cancelled(),
            other => td(
                [
                    "開けませんでした（{}）",
                    "Could not open ({})",
                    "无法打开（{}）",
                    "열 수 없었습니다 ({})",
                    "No se pudo abrir ({})",
                ],
                &other.to_string(),
            ),
        };
    }
    if let Some(c) = e.downcast_ref::<ContainerError>() {
        return match c {
            ContainerError::Io(inner) => describe_error_in(lang, inner),
            ContainerError::NoFont => t([
                "文章の EPUB を表示するための日本語フォントが端末に見つかりません",
                "No Japanese font was found on this device to display a text EPUB",
                "设备上找不到用于显示文字 EPUB 的日文字体",
                "문장 EPUB을 표시할 일본어 글꼴이 기기에 없습니다",
                "No se encontró en el dispositivo una fuente japonesa para mostrar EPUB de texto",
            ]),
            ContainerError::Unsupported => t([
                "対応していない形式です（ZIP/CBZ・7z・RAR・EPUB・フォルダに対応）",
                "Unsupported format (ZIP/CBZ, 7z, RAR, EPUB and folders are supported)",
                "不支持的格式（支持 ZIP/CBZ、7z、RAR、EPUB 和文件夹）",
                "지원하지 않는 형식입니다 (ZIP/CBZ·7z·RAR·EPUB·폴더 지원)",
                "Formato no compatible (se admiten ZIP/CBZ, 7z, RAR, EPUB y carpetas)",
            ]),
            ContainerError::Cancelled => cancelled(),
            ContainerError::LimitExceeded(what) => match *what {
                "entry count" => t([
                    "ファイルが多すぎて開けません（安全のための上限）",
                    "Too many files to open (safety limit)",
                    "文件太多，无法打开（安全上限）",
                    "파일이 너무 많아 열 수 없습니다 (안전을 위한 상한)",
                    "Demasiados archivos para abrir (límite de seguridad)",
                ]),
                "entry size" => t([
                    "大きすぎるファイルが含まれています（安全のための上限）",
                    "Contains a file that is too large (safety limit)",
                    "包含过大的文件（安全上限）",
                    "너무 큰 파일이 포함되어 있습니다 (안전을 위한 상한)",
                    "Contiene un archivo demasiado grande (límite de seguridad)",
                ]),
                "total size" => t([
                    "展開後の合計サイズが大きすぎます（安全のための上限）",
                    "The total size after extraction is too large (safety limit)",
                    "解压后的总大小过大（安全上限）",
                    "압축을 푼 뒤의 총 크기가 너무 큽니다 (안전을 위한 상한)",
                    "El tamaño total tras extraer es demasiado grande (límite de seguridad)",
                ]),
                "archive size" => t([
                    "書庫が大きすぎます（2GB まで）",
                    "The archive is too large (up to 2 GB)",
                    "压缩包过大（最大 2GB）",
                    "압축 파일이 너무 큽니다 (최대 2GB)",
                    "El archivo es demasiado grande (hasta 2 GB)",
                ]),
                "disk space" => t([
                    "端末の空き容量が足りません",
                    "Not enough free space on the device",
                    "设备可用空间不足",
                    "기기의 여유 공간이 부족합니다",
                    "No hay espacio libre suficiente en el dispositivo",
                ]),
                "compression ratio" => t([
                    "圧縮率が異常なため開きません（安全のため）",
                    "Not opened because the compression ratio is abnormal (for safety)",
                    "压缩率异常，出于安全考虑未打开",
                    "압축률이 비정상적이어서 열지 않습니다 (안전을 위해)",
                    "No se abre porque la tasa de compresión es anómala (por seguridad)",
                ]),
                _ => t([
                    "安全のための上限を超えています",
                    "A safety limit was exceeded",
                    "超出了安全上限",
                    "안전을 위한 상한을 넘었습니다",
                    "Se superó un límite de seguridad",
                ]),
            },
            ContainerError::Zip(inner) => td(
                [
                    "書庫が壊れているようです（{}）",
                    "The archive seems to be corrupted ({})",
                    "压缩包似乎已损坏（{}）",
                    "압축 파일이 손상된 것 같습니다 ({})",
                    "El archivo parece estar dañado ({})",
                ],
                &inner.to_string(),
            ),
            ContainerError::Archive(inner) => td(
                [
                    "書庫を読めません。壊れているか、パスワード付きの可能性があります（{}）",
                    "Cannot read the archive. It may be corrupted or password-protected ({})",
                    "无法读取压缩包。可能已损坏或设有密码（{}）",
                    "압축 파일을 읽을 수 없습니다. 손상되었거나 암호가 걸려 있을 수 있습니다 ({})",
                    "No se puede leer el archivo. Puede estar dañado o protegido con contraseña ({})",
                ],
                inner,
            ),
            ContainerError::NotFound(_) => t([
                "書庫の中身が見つかりません",
                "Cannot find the contents of the archive",
                "找不到压缩包中的内容",
                "압축 파일의 내용을 찾을 수 없습니다",
                "No se encuentra el contenido del archivo",
            ]),
        };
    }
    if let Some(io) = e.downcast_ref::<std::io::Error>() {
        return match io.kind() {
            std::io::ErrorKind::PermissionDenied => t([
                "アクセス権限がありません。設定でファイルへのアクセスを許可してください",
                "No permission to access it. Please allow file access in the settings",
                "没有访问权限。请在设置中允许访问文件",
                "접근 권한이 없습니다. 설정에서 파일 접근을 허용해 주세요",
                "Sin permiso de acceso. Permite el acceso a archivos en los ajustes",
            ]),
            std::io::ErrorKind::NotFound => t([
                "ファイルまたはフォルダが見つかりません",
                "File or folder not found",
                "找不到文件或文件夹",
                "파일 또는 폴더를 찾을 수 없습니다",
                "No se encuentra el archivo o la carpeta",
            ]),
            _ => td(
                [
                    "読み込みに失敗しました（{}）",
                    "Failed to load ({})",
                    "读取失败（{}）",
                    "불러오지 못했습니다 ({})",
                    "Error al cargar ({})",
                ],
                &io.to_string(),
            ),
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
    Fill,
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
    FitFill,
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

/// 文章の EPUB の書体
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PvTypeface {
    Gothic,
    Mincho,
}

impl From<pv_container::Typeface> for PvTypeface {
    fn from(t: pv_container::Typeface) -> Self {
        match t {
            pv_container::Typeface::Gothic => Self::Gothic,
            pv_container::Typeface::Mincho => Self::Mincho,
        }
    }
}

impl From<PvTypeface> for pv_container::Typeface {
    fn from(t: PvTypeface) -> Self {
        match t {
            PvTypeface::Gothic => Self::Gothic,
            PvTypeface::Mincho => Self::Mincho,
        }
    }
}

/// 文章の EPUB の見た目。変えたあとは、本を開き直すと反映される。
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct PvTextStyle {
    /// 文字の大きさ（`TEXT_SIZE_MIN`〜`TEXT_SIZE_MAX`）
    pub size: u32,
    pub typeface: PvTypeface,
}

/// 本をまとめたグループ（シリーズ・フォルダ単位など）
#[derive(Debug, Clone, uniffi::Record)]
pub struct PvGroup {
    pub id: i64,
    pub name: String,
    pub count: u32,
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
            FitMode::Fill => Self::Fill,
        }
    }
}

impl From<PvFit> for FitMode {
    fn from(f: PvFit) -> Self {
        match f {
            PvFit::Width => Self::Width,
            PvFit::Height => Self::Height,
            PvFit::Page => Self::Page,
            PvFit::Fill => Self::Fill,
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
            Action::FitFill => Self::FitFill,
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
            Self::FitFill => Action::FitFill,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_described_in_every_language() {
        use pv_container::ContainerError;
        use pv_core::CoreError;
        let ja = |e: &(dyn std::error::Error + 'static)| describe_error_in(Lang::Ja, e);
        assert!(ja(&CoreError::NoPages).contains("画像が見つかりません"));
        assert!(ja(&ContainerError::Unsupported).contains("対応していない形式"));
        assert!(ja(&ContainerError::LimitExceeded("entry count")).contains("多すぎ"));
        assert!(ja(&ContainerError::LimitExceeded("disk space")).contains("空き容量"));
        // 入れ子（CoreError → ContainerError → io::Error）の権限エラーまで届く
        let perm = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let nested = CoreError::Container(ContainerError::Io(perm));
        assert!(ja(&nested).contains("アクセス権限"));
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(ja(&missing).contains("見つかりません"));

        // 言語ごとに別の文言になり、詳細は埋め込まれる
        let corrupt = ContainerError::Archive("bad header".into());
        let texts: Vec<String> = [Lang::Ja, Lang::En, Lang::Zh, Lang::Ko, Lang::Es]
            .into_iter()
            .map(|l| describe_error_in(l, &corrupt))
            .collect();
        assert!(texts
            .iter()
            .all(|t| t.contains("bad header") && !t.contains("{}")));
        for i in 0..texts.len() {
            for j in i + 1..texts.len() {
                assert_ne!(texts[i], texts[j]);
            }
        }
        assert!(describe_error_in(Lang::En, &CoreError::NoPages).contains("No images"));
        assert!(describe_error_in(Lang::Es, &ContainerError::NoFont).contains("fuente"));
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
