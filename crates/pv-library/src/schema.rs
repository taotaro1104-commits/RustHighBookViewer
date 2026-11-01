use rusqlite_migration::{Migrations, M};

pub const V1: &str = "
CREATE TABLE books (
    id             INTEGER PRIMARY KEY,
    uri            TEXT    NOT NULL UNIQUE,
    size           INTEGER NOT NULL,
    head_hash      TEXT    NOT NULL,
    title          TEXT    NOT NULL,
    page_count     INTEGER NOT NULL DEFAULT 0,
    added_at       INTEGER NOT NULL,
    last_opened_at INTEGER,
    status         INTEGER NOT NULL DEFAULT 0,
    favorite       INTEGER NOT NULL DEFAULT 0,
    hidden_shelf   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_books_identity ON books(head_hash, size);

CREATE TABLE progress (
    book_id    INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    spread_idx INTEGER NOT NULL,
    page_idx   INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE bookmarks (
    id         INTEGER PRIMARY KEY,
    book_id    INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    page_idx   INTEGER NOT NULL,
    note       TEXT    NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    UNIQUE(book_id, page_idx)
);

CREATE TABLE shelves (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE shelf_items (
    shelf_id INTEGER NOT NULL REFERENCES shelves(id) ON DELETE CASCADE,
    book_id  INTEGER NOT NULL REFERENCES books(id)   ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (shelf_id, book_id)
);

CREATE TABLE book_settings (
    book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    json    TEXT NOT NULL
);
";

/// ページの寸法のキャッシュ。見開き判定のために全ページの先頭を読む処理（遅い入力では数秒）を、
/// 2 回目以降の読み込みで省く。`data` は (幅, 高さ) の u32 リトルエンディアンを並べたもの。
pub const V2: &str = "
CREATE TABLE page_sizes (
    book_id    INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    page_count INTEGER NOT NULL,
    data       BLOB    NOT NULL
);
";

pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1), M::up(V2)])
}
