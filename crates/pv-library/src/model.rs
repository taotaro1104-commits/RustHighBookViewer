#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("book {0} not found")]
    NotFound(i64),
    #[error("{0}")]
    Invalid(&'static str),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

pub type BookId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStatus {
    Unread,
    Reading,
    Finished,
}

impl ReadStatus {
    pub(crate) fn to_i64(self) -> i64 {
        match self {
            ReadStatus::Unread => 0,
            ReadStatus::Reading => 1,
            ReadStatus::Finished => 2,
        }
    }

    pub(crate) fn from_i64(v: i64) -> Self {
        match v {
            1 => ReadStatus::Reading,
            2 => ReadStatus::Finished,
            _ => ReadStatus::Unread,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBook {
    pub uri: String,
    pub size: u64,
    pub head_hash: String,
    pub title: String,
    pub page_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Book {
    pub id: BookId,
    pub uri: String,
    pub size: u64,
    pub head_hash: String,
    pub title: String,
    pub page_count: usize,
    pub added_at: i64,
    pub last_opened_at: Option<i64>,
    pub status: ReadStatus,
    pub favorite: bool,
    pub hidden_shelf: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub spread_idx: usize,
    pub page_idx: usize,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i64,
    pub book_id: BookId,
    pub page_idx: usize,
    pub note: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSort {
    /// 名前（自然順）
    Name,
    LastOpened,
    Added,
    Size,
}

#[derive(Debug, Clone, Default)]
pub struct BookFilter {
    pub status: Option<ReadStatus>,
    pub favorite_only: bool,
    /// 隠し本棚の本を含めるか
    pub include_hidden: bool,
    /// タイトルの部分一致（大文字小文字無視）
    pub query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    pub id: i64,
    pub name: String,
}
