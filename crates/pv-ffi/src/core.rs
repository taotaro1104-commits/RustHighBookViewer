//! アプリ全体の窓口: ライブラリ・設定・サムネイル・本を開く処理。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use pv_container::{
    kind_from_path, natural_cmp, open_source_styled, scan_books, EntryKind, ExtractGuard,
    FolderContainer, ScanKind,
};
use pv_core::{
    make_thumbnail_from, BookOverride, BookSession, Container, DiskCache, FileSource, Settings,
    Source,
};
use pv_library::{BookFilter, BookKey, BookSort, Library, NewBook, HEAD_LEN};

use crate::fd::source_from_fd;
use crate::reader::{PvReader, ReaderInfo};
use crate::tree::{PvTree, TreeContainer};
use crate::types::*;

const THUMB_EDGE: u32 = 256;
/// 一括読み込みで扱う本の数の上限
const MAX_SCAN_BOOKS: usize = 5000;

/// 本の題名: フォルダは名前、書庫ファイルは拡張子を除いた名前。
fn local_title(p: &Path, strip_ext: bool) -> String {
    let name = if strip_ext {
        p.file_stem()
    } else {
        p.file_name()
    };
    name.map_or_else(
        || p.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[derive(uniffi::Object)]
pub struct PvCore {
    library: Arc<Library>,
    settings: Mutex<Settings>,
    disk: DiskCache,
    /// 設定ファイル（settings.toml）の場所
    settings_path: PathBuf,
    /// 7z / RAR の展開先。起動ごとに空にする
    scratch: PathBuf,
    /// 7z / RAR の展開を途中で止める合図。新しく開くたびに下ろす
    cancel: Arc<AtomicBool>,
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn thumb_key(head_hash: &str, size: u64) -> u64 {
    fnv(&format!("{head_hash}:{size}"))
}

#[uniffi::export]
impl PvCore {
    /// `data_dir` はアプリ専用領域（Android の filesDir）。設定・DB・サムネイルを置く。
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> PvResult<Arc<Self>> {
        let dir = PathBuf::from(data_dir);
        std::fs::create_dir_all(&dir).map_err(PvError::new)?;
        let settings_path = dir.join("settings.toml");
        let settings = Settings::load(&settings_path).unwrap_or_else(|e| {
            eprintln!("settings ignored: {e}");
            Settings::default()
        });
        let library = Arc::new(Library::open(&dir.join("library.db")).map_err(PvError::new)?);
        let disk =
            DiskCache::open(&dir.join("thumbs"), settings.cache.l3_bytes).map_err(PvError::new)?;
        // 前回の異常終了で残った展開物を片付ける
        let scratch = dir.join("extract");
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(&scratch).map_err(PvError::new)?;
        Ok(Arc::new(Self {
            library,
            settings: Mutex::new(settings),
            disk,
            settings_path,
            scratch,
            cancel: Arc::default(),
        }))
    }

    /// 全体の既定設定。
    pub fn settings(&self) -> PvSettings {
        PvSettings::from(&*self.settings.lock())
    }

    /// 既定設定を保存し、これから開く本に使う。開いたことのある本は、本ごとの設定が優先される
    /// （`reset_book_settings` で消せる）。範囲外の値は収める。
    pub fn set_settings(&self, new: PvSettings) -> PvResult<()> {
        let mut guard = self.settings.lock();
        let updated = new.apply_to(&guard);
        updated.save(&self.settings_path).map_err(PvError::new)?;
        *guard = updated;
        Ok(())
    }

    /// エラーメッセージの言語を切り替える（"ja" / "en" / "zh" / "ko" / "es"）。
    pub fn set_language(&self, code: String) {
        crate::i18n::set_lang(crate::i18n::Lang::from_code(&code));
    }

    /// 文章の EPUB の文字の大きさ・書体。
    pub fn text_style(&self) -> PvTextStyle {
        let t = self.settings.lock().text;
        PvTextStyle {
            size: t.font_px,
            typeface: t.typeface.into(),
            line_spacing: t.line_spacing,
            ink: t.ink,
            paper: t.paper,
            letter_spacing: t.letter_spacing,
            bold: t.bold,
            italic: t.italic,
            vertical: t.vertical,
        }
    }

    /// 文章の EPUB の見た目を保存する。次に開く本から使う（開いている本は、開き直すと反映される）。
    /// 範囲外の大きさは収める。
    pub fn set_text_style(&self, style: PvTextStyle) -> PvResult<()> {
        let mut guard = self.settings.lock();
        let mut updated = guard.clone();
        updated.text = pv_container::TextStyle {
            font_px: style.size,
            typeface: style.typeface.into(),
            line_spacing: style.line_spacing,
            ink: style.ink,
            paper: style.paper,
            letter_spacing: style.letter_spacing,
            bold: style.bold,
            italic: style.italic,
            vertical: style.vertical,
        }
        .clamped();
        updated.save(&self.settings_path).map_err(PvError::new)?;
        *guard = updated;
        Ok(())
    }

    /// 端末で使える書体（明朝体は端末にフォントが無いことがある）。
    pub fn available_typefaces(&self) -> Vec<PvTypeface> {
        [PvTypeface::Gothic, PvTypeface::Mincho]
            .into_iter()
            .filter(|t| pv_container::has_typeface((*t).into()))
            .collect()
    }

    /// すべての本の「本ごとの設定」（表示モード・綴じ方向・フィット・色調整）を消す。
    pub fn reset_book_settings(&self) -> PvResult<()> {
        self.library.clear_book_settings().map_err(PvError::new)
    }

    /// 実行中の「開く」を中断する（7z / RAR の展開など時間のかかる処理が対象。別スレッドから呼ぶ）。
    /// 中断された `open_*` は `Cancelled` 相当のエラーを返す。
    pub fn cancel_open(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// content:// から得た fd（所有権ごと渡す）で書庫を開く。`uri` はライブラリでの識別・再オープン用。
    pub fn open_fd(
        self: Arc<Self>,
        fd: i32,
        uri: String,
        title: String,
    ) -> PvResult<Arc<PvReader>> {
        self.cancel.store(false, Ordering::Relaxed);
        let src = source_from_fd(fd).map_err(PvError::open)?;
        self.open_source_inner(src, uri, title)
    }

    /// SAF のフォルダ（ツリー URI）を開く。`uri` はライブラリでの識別・再オープン用。
    pub fn open_tree(
        self: Arc<Self>,
        tree: Arc<dyn PvTree>,
        uri: String,
        title: String,
    ) -> PvResult<Arc<PvReader>> {
        self.cancel.store(false, Ordering::Relaxed);
        let container = TreeContainer::from_tree(tree).map_err(PvError::open)?;
        let key = BookKey::from_listing(container.listing());
        self.open_container_inner(Arc::new(container), key, uri, title)
    }

    /// 端末内のパスで開く。フォルダなら直下の画像を 1 冊に、書庫ファイルならその 1 冊を開く
    /// （「すべてのファイルへのアクセス」権限があるとき用。SAF より速い）。
    pub fn open_local(self: Arc<Self>, path: String) -> PvResult<Arc<PvReader>> {
        self.cancel.store(false, Ordering::Relaxed);
        let p = self.prepare_local(&path)?;
        self.open_container_inner(p.container, p.key, p.uri, p.title)
    }

    /// フォルダ以下を探して、本の単位に分けて返す。
    /// 各フォルダの直下の画像をまとめて 1 冊、EPUB などの書庫ファイルは 1 つにつき 1 冊。サブフォルダも同様。
    pub fn scan_folder(&self, path: String) -> PvResult<Vec<PvScanItem>> {
        // 以降の一括登録のために、前回のキャンセルを解除しておく
        self.cancel.store(false, Ordering::Relaxed);
        let items = scan_books(Path::new(&path), MAX_SCAN_BOOKS).map_err(PvError::open)?;
        Ok(items
            .into_iter()
            .map(|i| PvScanItem {
                title: local_title(&i.path, i.kind == ScanKind::Archive),
                is_folder: i.kind == ScanKind::Images,
                path: i.path.to_string_lossy().into_owned(),
            })
            .collect())
    }

    /// 開かずに本棚へ登録する（ページ数とサムネイルまで）。フォルダ一括読み込み用。
    /// 複数のスレッドから同時に呼べる。キャンセルの解除はしない（`scan_folder` が行う）。
    pub fn register_local(&self, path: String) -> PvResult<PvBook> {
        let p = self.prepare_local(&path)?;
        let (book, _) = self.register_container(p.container.as_ref(), &p.key, p.uri, p.title)?;
        Ok(PvBook::from(&book))
    }

    /// ファイルパスで開く（テスト・デスクトップ向け）。
    pub fn open_path(self: Arc<Self>, path: String) -> PvResult<Arc<PvReader>> {
        self.cancel.store(false, Ordering::Relaxed);
        let p = Path::new(&path);
        let src: Arc<dyn Source> = Arc::new(FileSource::open(p).map_err(PvError::open)?);
        let title = p
            .file_stem()
            .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
        self.open_source_inner(src, path, title)
    }

    pub fn list_books(
        &self,
        query: String,
        favorites_only: bool,
        sort: PvSort,
    ) -> PvResult<Vec<PvBook>> {
        let filter = BookFilter {
            favorite_only: favorites_only,
            query: (!query.is_empty()).then_some(query),
            ..Default::default()
        };
        let sort = match sort {
            PvSort::Name => BookSort::Name,
            PvSort::LastOpened => BookSort::LastOpened,
            PvSort::Added => BookSort::Added,
            PvSort::Size => BookSort::Size,
        };
        let books = self
            .library
            .list_books(&filter, sort)
            .map_err(PvError::new)?;
        Ok(books.iter().map(PvBook::from).collect())
    }

    pub fn recent_books(&self, limit: u32) -> PvResult<Vec<PvBook>> {
        let books = self.library.recent(limit as usize).map_err(PvError::new)?;
        Ok(books.iter().map(PvBook::from).collect())
    }

    pub fn thumbnail(&self, book_id: i64) -> Option<PvThumb> {
        let book = self.library.get_book(book_id).ok()?;
        let bmp = self.disk.get(thumb_key(&book.head_hash, book.size))?;
        Some(PvThumb {
            width: bmp.size.w,
            height: bmp.size.h,
            rgba: bmp.data,
        })
    }

    pub fn set_favorite(&self, book_id: i64, favorite: bool) -> PvResult<()> {
        self.library
            .set_favorite(book_id, favorite)
            .map_err(PvError::new)
    }

    /// 指定したフォルダの中身（登録済みの本の場所から作る）。`prefix` が空ならいちばん上。
    /// `recursive` なら、配下のすべての本を `books` に入れる（そうでなければ直下の本だけ）。
    pub fn browse_library(
        &self,
        prefix: String,
        recursive: bool,
        sort: PvSort,
    ) -> PvResult<PvFolderView> {
        let books = self.list_books(String::new(), false, sort)?;
        let uris: Vec<String> = books.iter().map(|b| b.uri.clone()).collect();
        let v = crate::folders::view(&uris, &prefix, recursive);
        Ok(PvFolderView {
            folders: v
                .folders
                .into_iter()
                .map(|e| PvFolderEntry {
                    name: e.name,
                    path: e.path,
                    count: e.count as u32,
                })
                .collect(),
            books: v.shown.into_iter().map(|i| books[i].clone()).collect(),
        })
    }

    /// グループの一覧（名前順）。
    pub fn groups(&self) -> PvResult<Vec<PvGroup>> {
        let rows = self.library.shelves_with_counts().map_err(PvError::new)?;
        Ok(rows
            .into_iter()
            .map(|(s, n)| PvGroup {
                id: s.id,
                name: s.name,
                count: n as u32,
            })
            .collect())
    }

    /// グループを作る。同じ名前のグループがあれば、それを返す。
    pub fn create_group(&self, name: String) -> PvResult<PvGroup> {
        let s = self
            .library
            .create_shelf(name.trim())
            .map_err(PvError::new)?;
        let count = self.library.shelf_books(s.id).map_err(PvError::new)?.len() as u32;
        Ok(PvGroup {
            id: s.id,
            name: s.name,
            count,
        })
    }

    pub fn rename_group(&self, group_id: i64, name: String) -> PvResult<()> {
        self.library
            .rename_shelf(group_id, name.trim())
            .map_err(PvError::new)
    }

    /// グループを消す（中の本はライブラリに残る）。
    pub fn delete_group(&self, group_id: i64) -> PvResult<()> {
        self.library.delete_shelf(group_id).map_err(PvError::new)
    }

    /// 本をグループに入れる（渡した順に末尾へ足す。すでに入っている本は無視）。
    pub fn add_to_group(&self, group_id: i64, book_ids: Vec<i64>) -> PvResult<()> {
        for id in book_ids {
            self.library
                .add_to_shelf(group_id, id)
                .map_err(PvError::new)?;
        }
        Ok(())
    }

    pub fn remove_from_group(&self, group_id: i64, book_ids: Vec<i64>) -> PvResult<()> {
        for id in book_ids {
            self.library
                .remove_from_shelf(group_id, id)
                .map_err(PvError::new)?;
        }
        Ok(())
    }

    /// グループの中の本（入れた順）。
    pub fn group_books(&self, group_id: i64) -> PvResult<Vec<PvBook>> {
        let books = self.library.shelf_books(group_id).map_err(PvError::new)?;
        Ok(books.iter().map(PvBook::from).collect())
    }

    pub fn delete_book(&self, book_id: i64) -> PvResult<()> {
        self.library.delete_book(book_id).map_err(PvError::new)
    }
}

/// パスから開く準備ができたもの
struct Prepared {
    container: Arc<dyn Container>,
    key: BookKey,
    uri: String,
    title: String,
}

impl PvCore {
    fn prepare_local(&self, path: &str) -> PvResult<Prepared> {
        let p = Path::new(path);
        let uri = format!("file://{path}");
        if p.is_dir() {
            let container = FolderContainer::open_flat(p).map_err(PvError::open)?;
            // 本の識別子は画像だけの「相対パス:サイズ」一覧から作る
            let listing = container
                .entries()
                .iter()
                .filter(|e| e.kind == EntryKind::Image)
                .map(|e| (e.path.clone(), e.size))
                .collect();
            return Ok(Prepared {
                container: Arc::new(container),
                key: BookKey::from_listing(listing),
                uri,
                title: local_title(p, false),
            });
        }
        let src: Arc<dyn Source> = Arc::new(FileSource::open(p).map_err(PvError::open)?);
        let mut head = vec![0u8; HEAD_LEN.min(src.len() as usize)];
        let n = read_fully(src.as_ref(), &mut head).map_err(PvError::new)?;
        let key = BookKey::from_head(src.len(), &head[..n]);
        let guard = ExtractGuard {
            cancel: Arc::clone(&self.cancel),
            ..ExtractGuard::default()
        };
        let container = open_source_styled(src, &self.scratch, &guard, self.settings.lock().text)
            .map_err(PvError::open)?;
        Ok(Prepared {
            container,
            key,
            uri,
            title: local_title(p, true),
        })
    }

    /// 本棚への登録（ページ数の確認・ライブラリへの保存・サムネイル）。開く処理と共通。
    fn register_container(
        &self,
        container: &dyn Container,
        key: &BookKey,
        uri: String,
        title: String,
    ) -> PvResult<(pv_library::Book, usize)> {
        // ページ（画像）の数。0 冊分のものを本棚に登録しないよう、先に確かめる
        let page_count = container
            .entries()
            .iter()
            .filter(|e| e.kind == EntryKind::Image)
            .count();
        if page_count == 0 {
            return Err(PvError::open(pv_core::CoreError::NoPages));
        }
        let book = self
            .library
            .upsert_book(
                &NewBook {
                    uri,
                    size: key.size,
                    head_hash: key.head_hash.clone(),
                    title,
                    page_count,
                },
                pv_library::now_ms(),
            )
            .map_err(PvError::new)?;
        // 本棚用サムネイル（SAF では後から開き直せない場合があるので、登録時に作る）
        let tkey = thumb_key(&key.head_hash, key.size);
        if self.disk.get(tkey).is_none() {
            match make_thumbnail_from(container, THUMB_EDGE) {
                Ok(bmp) => {
                    if let Err(e) = self.disk.put(tkey, &bmp) {
                        eprintln!("thumbnail not cached: {e}");
                    }
                }
                Err(e) => eprintln!("thumbnail not created: {e}"),
            }
        }
        Ok((book, page_count))
    }

    fn open_source_inner(
        &self,
        src: Arc<dyn Source>,
        uri: String,
        title: String,
    ) -> PvResult<Arc<PvReader>> {
        let size = src.len();
        let mut head = vec![0u8; HEAD_LEN.min(size as usize)];
        let n = read_fully(src.as_ref(), &mut head).map_err(PvError::new)?;
        let key = BookKey::from_head(size, &head[..n]);

        let guard = ExtractGuard {
            cancel: Arc::clone(&self.cancel),
            ..ExtractGuard::default()
        };
        let container: Arc<dyn Container> =
            open_source_styled(src, &self.scratch, &guard, self.settings.lock().text)
                .map_err(PvError::open)?;
        self.open_container_inner(container, key, uri, title)
    }

    fn open_container_inner(
        &self,
        container: Arc<dyn Container>,
        key: BookKey,
        uri: String,
        title: String,
    ) -> PvResult<Arc<PvReader>> {
        let settings = self.settings.lock().clone();
        let uri_for_reader = uri.clone();
        let (book, page_count) =
            self.register_container(container.as_ref(), &key, uri, title.clone())?;
        let now = pv_library::now_ms();
        self.library
            .touch_opened(book.id, now)
            .map_err(PvError::new)?;

        // 前回調べたページの寸法があれば使う（全ページの先頭を読む処理は、SAF や 7z では数秒かかる）
        let cached = self
            .library
            .page_sizes(book.id, page_count)
            .unwrap_or_else(|e| {
                eprintln!("page sizes ignored: {e}");
                None
            });
        let mut session = BookSession::open_container_with_sizes(
            Arc::clone(&container),
            fnv(&key.head_hash),
            &settings,
            cached.as_deref(),
        )
        .map_err(PvError::open)?;

        // 書庫ごとの設定上書きを復元してから、読書位置へ移動する
        match self.library.book_settings(book.id) {
            Ok(Some(json)) => match serde_json::from_str::<BookOverride>(&json) {
                Ok(o) => {
                    let merged = settings.merged(&o);
                    session.set_layout(merged.layout);
                    session.set_fit(merged.fit);
                    session.set_color(merged.color);
                }
                Err(e) => eprintln!("book settings ignored: {e}"),
            },
            // 文章の本（縦書き）は、何も設定していなければ単ページ・右綴じで開く
            _ if container.is_text() => {
                let mut layout = settings.layout;
                layout.mode = pv_core::ViewMode::Single;
                // 縦書きは右綴じ（左へめくる）、横書きは左綴じ（右へめくる）
                layout.direction = if settings.text.vertical {
                    pv_core::Direction::RightToLeft
                } else {
                    pv_core::Direction::LeftToRight
                };
                session.set_layout(layout);
            }
            _ => {}
        }
        if let Ok(Some(p)) = self.library.get_progress(book.id) {
            session.go_to_page(p.page_idx.min(session.page_count().saturating_sub(1)));
        }
        // 今回調べた寸法は、次回のために保存する
        let mut sizes_saved = cached.is_some();
        if !sizes_saved {
            if let Some(sizes) = session.page_sizes() {
                match self.library.save_page_sizes(book.id, &sizes) {
                    Ok(()) => sizes_saved = true,
                    Err(e) => eprintln!("page sizes not saved: {e}"),
                }
            }
        }

        Ok(Arc::new(PvReader::new(
            session,
            Arc::clone(&self.library),
            book.id,
            title,
            sizes_saved,
            ReaderInfo {
                uri: uri_for_reader,
                is_text: container.is_text(),
                toc: container
                    .toc()
                    .into_iter()
                    .map(|e| PvTocEntry {
                        title: e.title,
                        page: e.page as u32,
                        level: u32::from(e.level),
                    })
                    .collect(),
            },
        )))
    }
}

/// 本の一覧を並べ替える（どのタブの一覧にも使う）。
/// `sort` のキーで並べ、`descending` なら逆順。同じ値の本は、名前（自然順）、登録順の順に並べる。
/// 一度も開いていない本は、「最近開いた順」では最後（降順のとき）または最初（昇順のとき）に来る。
#[uniffi::export]
pub fn sort_books(mut books: Vec<PvBook>, sort: PvSort, descending: bool) -> Vec<PvBook> {
    books.sort_by(|a, b| {
        let primary = match sort {
            PvSort::Name => natural_cmp(&a.title, &b.title),
            PvSort::LastOpened => a.last_opened_at.cmp(&b.last_opened_at),
            PvSort::Added => a.added_at.cmp(&b.added_at),
            PvSort::Size => a.size.cmp(&b.size),
        };
        let primary = if descending {
            primary.reverse()
        } else {
            primary
        };
        primary
            .then_with(|| natural_cmp(&a.title, &b.title))
            .then_with(|| a.id.cmp(&b.id))
    });
    books
}

/// 拡張子から画像ファイルか判定する（フォルダの閲覧画面で、画像のあるフォルダを見分けるため）。
#[uniffi::export]
pub fn is_image_file_name(name: String) -> bool {
    kind_from_path(&name) == EntryKind::Image
}

/// read_at は短く返すことがあるので、埋まるまで読む。
fn read_fully(src: &dyn Source, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        let n = src.read_at(done as u64, &mut buf[done..])?;
        if n == 0 {
            break;
        }
        done += n;
    }
    Ok(done)
}
