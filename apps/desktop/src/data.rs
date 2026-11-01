//! アプリ全体で共有するデータ（設定・ライブラリ・サムネイル）と、本を開く処理。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use pv_core::{make_thumbnail, Bitmap, BookOverride, BookSession, DiskCache, Settings};
use pv_library::{fingerprint, now_ms, Book, BookId, Library, NewBook};

pub const THUMB_EDGE: u32 = 256;

pub struct Data {
    pub dir: PathBuf,
    pub library: Arc<Library>,
    pub settings: Settings,
    thumbs: Sender<ThumbRequest>,
    pub thumb_results: Receiver<(BookId, Bitmap)>,
}

struct ThumbRequest {
    id: BookId,
    uri: String,
    key: u64,
}

pub struct OpenedBook {
    pub session: BookSession,
    pub book: Book,
}

fn thumb_key(book: &Book) -> u64 {
    format!("{}:{}", book.head_hash, book.size)
        .bytes()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100000001b3)
        })
}

impl Data {
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let dir = std::env::var_os("PV_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|d| d.join("PerfectViewerRs")))
            .ok_or("no data directory")?;
        std::fs::create_dir_all(&dir)?;
        let settings = Settings::load(&dir.join("settings.toml")).unwrap_or_else(|e| {
            eprintln!("settings ignored: {e}");
            Settings::default()
        });
        let library = Arc::new(Library::open(&dir.join("library.db"))?);
        let disk = DiskCache::open(&dir.join("thumbs"), settings.cache.l3_bytes)?;

        // サムネイル生成は UI を止めないよう専用スレッドで行う（L3 ディスクキャッシュ経由）
        let (req_tx, req_rx) = channel::<ThumbRequest>();
        let (res_tx, res_rx) = channel();
        std::thread::spawn(move || {
            for r in req_rx {
                let bmp = disk.get(r.key).or_else(|| {
                    let b = make_thumbnail(Path::new(&r.uri), THUMB_EDGE).ok()?;
                    let _ = disk.put(r.key, &b);
                    Some(b)
                });
                if let Some(b) = bmp {
                    if res_tx.send((r.id, b)).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Self {
            dir,
            library,
            settings,
            thumbs: req_tx,
            thumb_results: res_rx,
        })
    }

    pub fn request_thumbnail(&self, book: &Book) {
        let _ = self.thumbs.send(ThumbRequest {
            id: book.id,
            uri: book.uri.clone(),
            key: thumb_key(book),
        });
    }

    pub fn save_settings(&self) {
        if let Err(e) = self.settings.save(&self.dir.join("settings.toml")) {
            eprintln!("settings not saved: {e}");
        }
    }

    /// 書庫を開き、ライブラリに登録し、書庫ごとの設定と読書位置を復元する。
    pub fn open_book(&self, path: &Path) -> Result<OpenedBook, Box<dyn std::error::Error>> {
        let uri = path.to_string_lossy().into_owned();
        let mut session = BookSession::open(&uri, &self.settings)?;
        let key = fingerprint(path)?;
        let title = path
            .file_stem()
            .or_else(|| path.file_name())
            .map_or_else(|| uri.clone(), |s| s.to_string_lossy().into_owned());
        let now = now_ms();
        let book = self.library.upsert_book(
            &NewBook {
                uri,
                size: key.size,
                head_hash: key.head_hash,
                title,
                page_count: session.page_count(),
            },
            now,
        )?;
        self.library.touch_opened(book.id, now)?;

        if let Some(json) = self.library.book_settings(book.id)? {
            match serde_json::from_str::<BookOverride>(&json) {
                Ok(o) => apply_override(&mut session, &self.settings.merged(&o)),
                Err(e) => eprintln!("book settings ignored: {e}"),
            }
        }
        let progress = self.library.get_progress(book.id)?;
        if let Some(p) = progress {
            session.go_to_page(p.page_idx.min(session.page_count() - 1));
        }
        Ok(OpenedBook { session, book })
    }

    pub fn save_override(&self, id: BookId, o: &BookOverride) {
        match serde_json::to_string(o) {
            Ok(json) => {
                if let Err(e) = self.library.set_book_settings(id, &json) {
                    eprintln!("book settings not saved: {e}");
                }
            }
            Err(e) => eprintln!("book settings not encoded: {e}"),
        }
    }
}

fn apply_override(session: &mut BookSession, merged: &Settings) {
    session.set_layout(merged.layout);
    session.set_fit(merged.fit);
}
