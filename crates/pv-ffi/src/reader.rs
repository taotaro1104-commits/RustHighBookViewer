//! 1 冊を開いている間の窓口（BookSession のラッパー）。Kotlin の閲覧画面が使う。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::Receiver;
use parking_lot::{Mutex, MutexGuard};
use pv_core::{BookOverride, BookSession, CoreEvent, InputKey, InputMap, PageThumbs, Size};
use pv_library::{now_ms, Library};

use crate::types::*;

/// `PvReader::new` に渡す、本の素性。
pub(crate) struct ReaderInfo {
    pub uri: String,
    pub is_text: bool,
}

#[derive(uniffi::Object)]
pub struct PvReader {
    session: Mutex<BookSession>,
    /// next_event がセッションのロックを持ったまま待たないよう、受信側は別に持つ
    rx: Receiver<CoreEvent>,
    input: Mutex<InputMap>,
    /// ユーザーが標準以外のプロファイルを使っている場合は、綴じ方向で作り直さない
    custom_input: bool,
    library: Arc<Library>,
    book_id: i64,
    title: String,
    /// 本棚から開き直すための識別子
    uri: String,
    /// 文章を描いたページでできた本か（文字の大きさ・書体を変えられる）
    is_text: bool,
    saved_page: Mutex<Option<usize>>,
    /// ページ寸法を DB に保存済みか（単ページ表示で開いた本を、あとで見開きにしたときに保存する）
    sizes_saved: AtomicBool,
    /// ページ一覧のサムネイル用。セッションのロックを取らずに並列で呼べる
    thumbs: PageThumbs,
}

impl PvReader {
    pub(crate) fn new(
        session: BookSession,
        library: Arc<Library>,
        book_id: i64,
        title: String,
        sizes_saved: bool,
        info: ReaderInfo,
    ) -> Self {
        let rx = session.events();
        let thumbs = session.thumbnailer();
        let settings = session.settings();
        let custom_input = settings.input.profile != "標準";
        let input = if custom_input {
            settings.input.clone()
        } else {
            InputMap::standard(settings.layout.direction)
        };
        Self {
            session: Mutex::new(session),
            rx,
            input: Mutex::new(input),
            custom_input,
            library,
            book_id,
            title,
            uri: info.uri,
            is_text: info.is_text,
            saved_page: Mutex::new(None),
            sizes_saved: AtomicBool::new(sizes_saved),
            thumbs,
        }
    }

    fn current_override(session: &BookSession) -> BookOverride {
        let s = session.settings();
        BookOverride {
            mode: Some(s.layout.mode),
            direction: Some(s.layout.direction),
            first_page_single: Some(s.layout.first_page_single),
            shift: Some(s.layout.shift),
            fit: Some(s.fit),
            color: Some(s.color),
        }
    }

    /// 書庫ごとの設定を保存し、綴じ方向が変わっていれば入力マップを作り直す。
    /// 必要な値を取り出したらセッションのロックを手放してから DB に書く（他の操作を待たせない）。
    fn after_settings_change(&self, session: MutexGuard<'_, BookSession>) {
        let json = serde_json::to_string(&Self::current_override(&session));
        let direction = session.settings().layout.direction;
        let sizes = (!self.sizes_saved.load(Ordering::Relaxed))
            .then(|| session.page_sizes())
            .flatten();
        drop(session);
        if let Some(sizes) = sizes {
            match self.library.save_page_sizes(self.book_id, &sizes) {
                Ok(()) => self.sizes_saved.store(true, Ordering::Relaxed),
                Err(e) => eprintln!("page sizes not saved: {e}"),
            }
        }
        if let Ok(json) = json {
            if let Err(e) = self.library.set_book_settings(self.book_id, &json) {
                eprintln!("book settings not saved: {e}");
            }
        }
        if !self.custom_input {
            *self.input.lock() = InputMap::standard(direction);
        }
    }
}

fn to_frame(ticket: u64, f: &pv_core::Frame) -> PvFrame {
    PvFrame {
        ticket,
        spread: f.spread as u32,
        width: f.bitmap.size.w,
        height: f.bitmap.size.h,
        viewport_width: f.viewport.w,
        viewport_height: f.viewport.h,
        scale: f.scale,
        rgba: f.bitmap.data.clone(),
    }
}

#[uniffi::export]
impl PvReader {
    pub fn book_id(&self) -> i64 {
        self.book_id
    }

    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// 本棚に登録した識別子（開き直すときに使う）。
    pub fn uri(&self) -> String {
        self.uri.clone()
    }

    /// 文章の本か。
    pub fn is_text(&self) -> bool {
        self.is_text
    }

    pub fn page_count(&self) -> u32 {
        self.session.lock().page_count() as u32
    }

    pub fn spread_count(&self) -> u32 {
        self.session.lock().spread_count() as u32
    }

    pub fn cursor(&self) -> u32 {
        self.session.lock().cursor() as u32
    }

    pub fn current_page(&self) -> u32 {
        self.session.lock().current_page() as u32
    }

    pub fn layout(&self) -> PvLayout {
        let s = self.session.lock();
        let st = s.settings();
        PvLayout {
            mode: st.layout.mode.into(),
            direction: st.layout.direction.into(),
            shift: st.layout.shift,
            fit: st.fit.into(),
        }
    }

    pub fn set_layout(&self, mode: PvViewMode, direction: PvDirection) {
        let mut s = self.session.lock();
        let mut l = s.settings().layout;
        l.mode = mode.into();
        l.direction = direction.into();
        s.set_layout(l);
        self.after_settings_change(s);
    }

    pub fn color(&self) -> PvColor {
        self.session.lock().settings().color.into()
    }

    /// 色調整を変更して書庫ごとの設定に保存する。表示中のフレームは呼び出し側が再要求する。
    pub fn set_color(&self, color: PvColor) {
        let mut s = self.session.lock();
        s.set_color(color.into());
        self.after_settings_change(s);
    }

    /// ページ一覧用の小さなサムネイル。デコードに失敗したページは None。
    pub fn page_thumbnail(&self, page: u32, edge: u32) -> Option<PvThumb> {
        let bmp = self.thumbs.get(page as usize, edge.clamp(16, 1024)).ok()?;
        Some(PvThumb {
            width: bmp.size.w,
            height: bmp.size.h,
            rgba: bmp.data,
        })
    }

    pub fn go_to(&self, spread: u32) {
        self.session.lock().go_to(spread as usize);
    }

    pub fn go_to_page(&self, page: u32) {
        self.session.lock().go_to_page(page as usize);
    }

    /// コアで処理できるナビゲーション・レイアウト操作を実行する。UI で扱うべき操作は false。
    pub fn apply(&self, action: PvAction) -> bool {
        let Some(core) = action.to_core() else {
            return false;
        };
        let mut s = self.session.lock();
        let handled = s.apply(&core);
        if handled
            && matches!(
                action,
                PvAction::ShiftSpread
                    | PvAction::FitWidth
                    | PvAction::FitHeight
                    | PvAction::FitPage
                    | PvAction::FitFill
            )
        {
            self.after_settings_change(s);
        }
        handled
    }

    pub fn request_frame(&self, spread: u32, width: u32, height: u32, scale: f32) -> u64 {
        let t = self
            .session
            .lock()
            .request_frame(spread as usize, Size::new(width, height), scale);
        t.id
    }

    /// イベントを最大 `timeout_ms` 待つ。Kotlin 側はワーカースレッドから呼ぶこと。
    pub fn next_event(&self, timeout_ms: u32) -> Option<PvEvent> {
        let ev = self
            .rx
            .recv_timeout(Duration::from_millis(timeout_ms as u64))
            .ok()?;
        Some(match ev {
            CoreEvent::FrameReady { ticket, frame } => PvEvent::Frame {
                frame: to_frame(ticket.id, &frame),
            },
            CoreEvent::Cancelled { ticket } => PvEvent::Cancelled { ticket: ticket.id },
            CoreEvent::Error { ticket, message } => PvEvent::Failed {
                ticket: ticket.id,
                message,
            },
        })
    }

    pub fn tap_action(&self, zone: u8) -> Option<PvAction> {
        self.lookup(InputKey::TapZone(zone))
    }

    pub fn key_action(&self, name: String) -> Option<PvAction> {
        self.lookup(InputKey::Key(name))
    }

    pub fn volume_action(&self, up: bool) -> Option<PvAction> {
        self.lookup(if up {
            InputKey::VolumeUp
        } else {
            InputKey::VolumeDown
        })
    }

    pub fn double_tap_action(&self) -> Option<PvAction> {
        self.lookup(InputKey::DoubleTap)
    }

    pub fn long_press_action(&self) -> Option<PvAction> {
        self.lookup(InputKey::LongPress)
    }

    /// onTrimMemory 相当。先読み窓の外の L1 を解放する。
    pub fn trim_memory(&self) {
        self.session.lock().trim_memory();
    }

    /// 読書位置を保存する。`force` が false なら、ページが変わったときだけ書く。
    pub fn save_progress(&self, force: bool) -> PvResult<()> {
        let (page, cursor) = {
            let s = self.session.lock();
            (s.current_page(), s.cursor())
        };
        let mut saved = self.saved_page.lock();
        if !force && *saved == Some(page) {
            return Ok(());
        }
        // DB への書き込みはセッションのロックを手放してから行う
        self.library
            .save_progress(self.book_id, cursor, page, now_ms())
            .map_err(PvError::new)?;
        *saved = Some(page);
        Ok(())
    }

    pub fn add_bookmark(&self) -> PvResult<u32> {
        let page = self.session.lock().current_page();
        self.library
            .add_bookmark(self.book_id, page, "", now_ms())
            .map_err(PvError::new)?;
        Ok(page as u32)
    }

    /// しおりのページ番号（昇順）。
    pub fn bookmarks(&self) -> PvResult<Vec<u32>> {
        Ok(self
            .library
            .bookmarks(self.book_id)
            .map_err(PvError::new)?
            .into_iter()
            .map(|b| b.page_idx as u32)
            .collect())
    }

    /// 指定ページのしおりを外す。しおりが無くてもエラーにしない。
    pub fn remove_bookmark(&self, page: u32) -> PvResult<()> {
        let found = self
            .library
            .bookmarks(self.book_id)
            .map_err(PvError::new)?
            .into_iter()
            .find(|b| b.page_idx == page as usize);
        if let Some(b) = found {
            self.library.remove_bookmark(b.id).map_err(PvError::new)?;
        }
        Ok(())
    }

    /// 処理前の元バイト列（画像の保存・共有用）。
    pub fn page_bytes(&self, page: u32) -> PvResult<Vec<u8>> {
        // セッションのロックを持たずに読む（SAF など遅い入力でも他の操作を止めない）
        let bytes = self.thumbs.bytes(page as usize).map_err(PvError::new)?;
        Ok(bytes.to_vec())
    }

    pub fn page_name(&self, page: u32) -> String {
        self.session
            .lock()
            .page_path(page as usize)
            .and_then(|p| p.rsplit('/').next().map(str::to_owned))
            .unwrap_or_default()
    }
}

impl PvReader {
    fn lookup(&self, key: InputKey) -> Option<PvAction> {
        self.input.lock().lookup(&key).map(PvAction::from)
    }
}
