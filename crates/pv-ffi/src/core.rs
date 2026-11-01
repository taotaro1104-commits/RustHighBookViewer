//! アプリ全体の窓口: ライブラリ・設定・サムネイル・本を開く処理。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use pv_core::{
    make_thumbnail_from, open_source_in, BookOverride, BookSession, Container, DiskCache,
    FileSource, Settings, Source,
};
use pv_library::{BookFilter, BookKey, BookSort, Library, NewBook, HEAD_LEN};

use crate::fd::source_from_fd;
use crate::reader::PvReader;
use crate::tree::{PvTree, TreeContainer};
use crate::types::*;

const THUMB_EDGE: u32 = 256;

#[derive(uniffi::Object)]
pub struct PvCore {
    library: Arc<Library>,
    settings: Mutex<Settings>,
    disk: DiskCache,
    /// 7z / RAR の展開先。起動ごとに空にする
    scratch: PathBuf,
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
        let settings = Settings::load(&dir.join("settings.toml")).unwrap_or_else(|e| {
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
            scratch,
        }))
    }

    /// content:// から得た fd（所有権ごと渡す）で書庫を開く。`uri` はライブラリでの識別・再オープン用。
    pub fn open_fd(
        self: Arc<Self>,
        fd: i32,
        uri: String,
        title: String,
    ) -> PvResult<Arc<PvReader>> {
        let src = source_from_fd(fd).map_err(PvError::new)?;
        self.open_source_inner(src, uri, title)
    }

    /// SAF のフォルダ（ツリー URI）を開く。`uri` はライブラリでの識別・再オープン用。
    pub fn open_tree(
        self: Arc<Self>,
        tree: Arc<dyn PvTree>,
        uri: String,
        title: String,
    ) -> PvResult<Arc<PvReader>> {
        let container = TreeContainer::from_tree(tree).map_err(PvError::new)?;
        let key = BookKey::from_listing(container.listing());
        self.open_container_inner(Arc::new(container), key, uri, title)
    }

    /// ファイルパスで開く（テスト・デスクトップ向け）。
    pub fn open_path(self: Arc<Self>, path: String) -> PvResult<Arc<PvReader>> {
        let p = Path::new(&path);
        let src: Arc<dyn Source> = Arc::new(FileSource::open(p).map_err(PvError::new)?);
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

    pub fn delete_book(&self, book_id: i64) -> PvResult<()> {
        self.library.delete_book(book_id).map_err(PvError::new)
    }
}

impl PvCore {
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

        let container: Arc<dyn Container> =
            open_source_in(src, &self.scratch).map_err(PvError::new)?;
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
        let mut session =
            BookSession::open_container(Arc::clone(&container), fnv(&key.head_hash), &settings)
                .map_err(PvError::new)?;

        let now = pv_library::now_ms();
        let book = self
            .library
            .upsert_book(
                &NewBook {
                    uri,
                    size: key.size,
                    head_hash: key.head_hash.clone(),
                    title: title.clone(),
                    page_count: session.page_count(),
                },
                now,
            )
            .map_err(PvError::new)?;
        self.library
            .touch_opened(book.id, now)
            .map_err(PvError::new)?;

        // 書庫ごとの設定上書きを復元してから、読書位置へ移動する
        if let Ok(Some(json)) = self.library.book_settings(book.id) {
            match serde_json::from_str::<BookOverride>(&json) {
                Ok(o) => {
                    let merged = settings.merged(&o);
                    session.set_layout(merged.layout);
                    session.set_fit(merged.fit);
                    session.set_color(merged.color);
                }
                Err(e) => eprintln!("book settings ignored: {e}"),
            }
        }
        if let Ok(Some(p)) = self.library.get_progress(book.id) {
            session.go_to_page(p.page_idx.min(session.page_count().saturating_sub(1)));
        }

        // 本棚用サムネイル（SAF では後から開き直せない場合があるので、開いた時点で作る）
        let tkey = thumb_key(&key.head_hash, key.size);
        if self.disk.get(tkey).is_none() {
            match make_thumbnail_from(container.as_ref(), THUMB_EDGE) {
                Ok(bmp) => {
                    if let Err(e) = self.disk.put(tkey, &bmp) {
                        eprintln!("thumbnail not cached: {e}");
                    }
                }
                Err(e) => eprintln!("thumbnail not created: {e}"),
            }
        }
        Ok(Arc::new(PvReader::new(
            session,
            Arc::clone(&self.library),
            book.id,
            title,
        )))
    }
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
