use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::model::*;
use crate::schema::migrations;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// 本棚・履歴・しおり・読書位置の SQLite ストア。WAL で UI の読み取りとワーカーの書き込みを並行させる。
pub struct Library {
    conn: Mutex<Connection>,
}

const BOOK_COLS: &str =
    "id, uri, size, head_hash, title, page_count, added_at, last_opened_at, status, favorite, hidden_shelf";

fn book_from_row(r: &Row) -> rusqlite::Result<Book> {
    Ok(Book {
        id: r.get(0)?,
        uri: r.get(1)?,
        size: r.get::<_, i64>(2)? as u64,
        head_hash: r.get(3)?,
        title: r.get(4)?,
        page_count: r.get::<_, i64>(5)? as usize,
        added_at: r.get(6)?,
        last_opened_at: r.get(7)?,
        status: ReadStatus::from_i64(r.get(8)?),
        favorite: r.get::<_, i64>(9)? != 0,
        hidden_shelf: r.get::<_, i64>(10)? != 0,
    })
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        Self::init(Connection::open(path)?, true)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, false)
    }

    fn init(mut conn: Connection, wal: bool) -> Result<Self> {
        if wal {
            conn.pragma_update(None, "journal_mode", "WAL")?;
        }
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrations().to_latest(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 本を登録または更新する。同じ内容(head_hash+size)なら移動・リネーム後も同じ ID を返し、
    /// 読書位置としおりを引き継ぐ。
    pub fn upsert_book(&self, b: &NewBook, now: i64) -> Result<Book> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let by_content: Option<i64> = tx
            .query_row(
                "SELECT id FROM books WHERE head_hash = ?1 AND size = ?2 ORDER BY (uri = ?3) DESC LIMIT 1",
                params![b.head_hash, b.size as i64, b.uri],
                |r| r.get(0),
            )
            .optional()?;
        let id = match by_content {
            Some(id) => {
                // 他の行が既にこの uri を持っていると UNIQUE に反するため、先に退かす
                tx.execute(
                    "DELETE FROM books WHERE uri = ?1 AND id != ?2",
                    params![b.uri, id],
                )?;
                tx.execute(
                    "UPDATE books SET uri=?1, title=?2, page_count=?3 WHERE id=?4",
                    params![b.uri, b.title, b.page_count as i64, id],
                )?;
                id
            }
            None => {
                let by_uri: Option<i64> = tx
                    .query_row("SELECT id FROM books WHERE uri = ?1", [&b.uri], |r| {
                        r.get(0)
                    })
                    .optional()?;
                match by_uri {
                    Some(id) => {
                        // 同じパスで中身が変わった
                        tx.execute(
                            "UPDATE books SET size=?1, head_hash=?2, title=?3, page_count=?4 WHERE id=?5",
                            params![b.size as i64, b.head_hash, b.title, b.page_count as i64, id],
                        )?;
                        id
                    }
                    None => {
                        tx.execute(
                            "INSERT INTO books (uri,size,head_hash,title,page_count,added_at) VALUES (?1,?2,?3,?4,?5,?6)",
                            params![b.uri, b.size as i64, b.head_hash, b.title, b.page_count as i64, now],
                        )?;
                        tx.last_insert_rowid()
                    }
                }
            }
        };
        let book = tx.query_row(
            &format!("SELECT {BOOK_COLS} FROM books WHERE id=?1"),
            [id],
            book_from_row,
        )?;
        tx.commit()?;
        Ok(book)
    }

    pub fn get_book(&self, id: BookId) -> Result<Book> {
        self.conn
            .lock()
            .query_row(
                &format!("SELECT {BOOK_COLS} FROM books WHERE id=?1"),
                [id],
                book_from_row,
            )
            .optional()?
            .ok_or(LibraryError::NotFound(id))
    }

    pub fn delete_book(&self, id: BookId) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM books WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn list_books(&self, filter: &BookFilter, sort: BookSort) -> Result<Vec<Book>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!("SELECT {BOOK_COLS} FROM books"))?;
        let all = stmt
            .query_map([], book_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let q = filter.query.as_deref().map(str::to_lowercase);
        let mut out: Vec<Book> = all
            .into_iter()
            .filter(|b| filter.include_hidden || !b.hidden_shelf)
            .filter(|b| filter.status.is_none_or(|s| s == b.status))
            .filter(|b| !filter.favorite_only || b.favorite)
            .filter(|b| {
                q.as_ref()
                    .is_none_or(|q| b.title.to_lowercase().contains(q))
            })
            .collect();
        match sort {
            BookSort::Name => out.sort_by(|a, b| natord::compare_ignore_case(&a.title, &b.title)),
            BookSort::LastOpened => out.sort_by_key(|b| std::cmp::Reverse(b.last_opened_at)),
            BookSort::Added => out.sort_by_key(|b| std::cmp::Reverse(b.added_at)),
            BookSort::Size => out.sort_by_key(|b| std::cmp::Reverse(b.size)),
        }
        Ok(out)
    }

    /// 履歴: 最後に読んだ順。隠し本棚は含めない。
    pub fn recent(&self, limit: usize) -> Result<Vec<Book>> {
        let conn = self.conn.lock();
        let cols = BOOK_COLS
            .split(", ")
            .map(|c| format!("b.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT {cols} FROM books b JOIN progress p ON p.book_id=b.id
             WHERE b.hidden_shelf=0 ORDER BY p.updated_at DESC, b.id DESC LIMIT ?1"
        ))?;
        let rows = stmt
            .query_map([limit as i64], book_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_favorite(&self, id: BookId, fav: bool) -> Result<()> {
        self.update_flag(id, "favorite", fav as i64)
    }

    pub fn set_hidden_shelf(&self, id: BookId, hidden: bool) -> Result<()> {
        self.update_flag(id, "hidden_shelf", hidden as i64)
    }

    pub fn set_status(&self, id: BookId, s: ReadStatus) -> Result<()> {
        self.update_flag(id, "status", s.to_i64())
    }

    fn update_flag(&self, id: BookId, col: &'static str, v: i64) -> Result<()> {
        let n = self.conn.lock().execute(
            &format!("UPDATE books SET {col}=?1 WHERE id=?2"),
            params![v, id],
        )?;
        if n == 0 {
            return Err(LibraryError::NotFound(id));
        }
        Ok(())
    }

    pub fn touch_opened(&self, id: BookId, now: i64) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE books SET last_opened_at=?1 WHERE id=?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// 読書位置を保存し、最終ページに達したら読了、それ以外は読書中にする。
    pub fn save_progress(
        &self,
        id: BookId,
        spread_idx: usize,
        page_idx: usize,
        now: i64,
    ) -> Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let pages: i64 = tx
            .query_row("SELECT page_count FROM books WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or(LibraryError::NotFound(id))?;
        tx.execute(
            "INSERT INTO progress (book_id, spread_idx, page_idx, updated_at) VALUES (?1,?2,?3,?4)
             ON CONFLICT(book_id) DO UPDATE SET spread_idx=?2, page_idx=?3, updated_at=?4",
            params![id, spread_idx as i64, page_idx as i64, now],
        )?;
        let status = if pages > 0 && page_idx as i64 + 1 >= pages {
            ReadStatus::Finished
        } else {
            ReadStatus::Reading
        };
        tx.execute(
            "UPDATE books SET status=?1 WHERE id=?2",
            params![status.to_i64(), id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn get_progress(&self, id: BookId) -> Result<Option<Progress>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT spread_idx, page_idx, updated_at FROM progress WHERE book_id=?1",
                [id],
                |r| {
                    Ok(Progress {
                        spread_idx: r.get::<_, i64>(0)? as usize,
                        page_idx: r.get::<_, i64>(1)? as usize,
                        updated_at: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }

    /// しおりを追加する。同じページには 1 つだけで、再追加はメモを更新する。
    pub fn add_bookmark(&self, id: BookId, page_idx: usize, note: &str, now: i64) -> Result<()> {
        let n = self.conn.lock().execute(
            "INSERT INTO bookmarks (book_id,page_idx,note,created_at)
             SELECT ?1,?2,?3,?4 WHERE EXISTS (SELECT 1 FROM books WHERE id=?1)
             ON CONFLICT(book_id,page_idx) DO UPDATE SET note=?3",
            params![id, page_idx as i64, note, now],
        )?;
        if n == 0 {
            return Err(LibraryError::NotFound(id));
        }
        Ok(())
    }

    pub fn remove_bookmark(&self, bookmark_id: i64) -> Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM bookmarks WHERE id=?1", [bookmark_id])?;
        Ok(())
    }

    pub fn bookmarks(&self, id: BookId) -> Result<Vec<Bookmark>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, book_id, page_idx, note, created_at FROM bookmarks WHERE book_id=?1 ORDER BY page_idx",
        )?;
        let rows = stmt
            .query_map([id], |r| {
                Ok(Bookmark {
                    id: r.get(0)?,
                    book_id: r.get(1)?,
                    page_idx: r.get::<_, i64>(2)? as usize,
                    note: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn create_shelf(&self, name: &str) -> Result<Shelf> {
        if name.trim().is_empty() {
            return Err(LibraryError::Invalid("shelf name is empty"));
        }
        let conn = self.conn.lock();
        conn.execute("INSERT OR IGNORE INTO shelves (name) VALUES (?1)", [name])?;
        Ok(
            conn.query_row("SELECT id, name FROM shelves WHERE name=?1", [name], |r| {
                Ok(Shelf {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?,
        )
    }

    pub fn shelves(&self) -> Result<Vec<Shelf>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT id, name FROM shelves ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Shelf {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn add_to_shelf(&self, shelf: i64, book: BookId) -> Result<()> {
        self.conn.lock().execute(
            "INSERT OR IGNORE INTO shelf_items (shelf_id, book_id, position)
             VALUES (?1, ?2, COALESCE((SELECT MAX(position)+1 FROM shelf_items WHERE shelf_id=?1), 0))",
            params![shelf, book],
        )?;
        Ok(())
    }

    pub fn remove_from_shelf(&self, shelf: i64, book: BookId) -> Result<()> {
        self.conn.lock().execute(
            "DELETE FROM shelf_items WHERE shelf_id=?1 AND book_id=?2",
            params![shelf, book],
        )?;
        Ok(())
    }

    pub fn shelf_books(&self, shelf: i64) -> Result<Vec<Book>> {
        let conn = self.conn.lock();
        let cols = BOOK_COLS
            .split(", ")
            .map(|c| format!("b.{c}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT {cols} FROM books b JOIN shelf_items s ON s.book_id=b.id WHERE s.shelf_id=?1 ORDER BY s.position"
        ))?;
        let rows = stmt
            .query_map([shelf], book_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 書庫ごとの設定上書き（JSON 文字列。解釈は呼び出し側）。
    pub fn set_book_settings(&self, id: BookId, json: &str) -> Result<()> {
        self.conn.lock().execute(
            "INSERT INTO book_settings (book_id, json) VALUES (?1, ?2)
             ON CONFLICT(book_id) DO UPDATE SET json=?2",
            params![id, json],
        )?;
        Ok(())
    }

    /// すべての本の「本ごとの設定」を消す（全体の既定設定を、読んだことのある本にも効かせ直すため）。
    pub fn clear_book_settings(&self) -> Result<()> {
        self.conn.lock().execute("DELETE FROM book_settings", [])?;
        Ok(())
    }

    /// 全ページの寸法 (幅, 高さ) を保存する。同じ本の読み込みで寸法の取得を省くためのキャッシュ。
    pub fn save_page_sizes(&self, id: BookId, sizes: &[(u32, u32)]) -> Result<()> {
        let mut data = Vec::with_capacity(sizes.len() * 8);
        for &(w, h) in sizes {
            data.extend_from_slice(&w.to_le_bytes());
            data.extend_from_slice(&h.to_le_bytes());
        }
        self.conn.lock().execute(
            "INSERT INTO page_sizes (book_id, page_count, data) VALUES (?1, ?2, ?3)
             ON CONFLICT(book_id) DO UPDATE SET page_count=?2, data=?3",
            params![id, sizes.len() as i64, data],
        )?;
        Ok(())
    }

    /// 保存済みの寸法。ページ数が `expected_count` と違う（本が差し替わった）場合や、壊れている場合は None。
    pub fn page_sizes(&self, id: BookId, expected_count: usize) -> Result<Option<Vec<(u32, u32)>>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .lock()
            .query_row(
                "SELECT page_count, data FROM page_sizes WHERE book_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(count, data)| {
            if count as usize != expected_count || data.len() != expected_count * 8 {
                return None;
            }
            Some(
                data.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| {
                        (
                            u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                            u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                        )
                    })
                    .collect(),
            )
        }))
    }

    pub fn book_settings(&self, id: BookId) -> Result<Option<String>> {
        Ok(self
            .conn
            .lock()
            .query_row(
                "SELECT json FROM book_settings WHERE book_id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }
}
