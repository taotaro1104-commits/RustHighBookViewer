use crate::*;

fn nb(uri: &str, hash: &str, title: &str, pages: usize) -> NewBook {
    NewBook {
        uri: uri.into(),
        size: 100,
        head_hash: hash.into(),
        title: title.into(),
        page_count: pages,
    }
}

fn lib() -> Library {
    Library::open_in_memory().unwrap()
}

#[test]
fn upsert_is_idempotent_for_same_path() {
    let l = lib();
    let a = l.upsert_book(&nb("/a.cbz", "h1", "A", 10), 1).unwrap();
    let b = l.upsert_book(&nb("/a.cbz", "h1", "A", 10), 2).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(b.added_at, 1);
    assert_eq!(
        l.list_books(&BookFilter::default(), BookSort::Name)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn moved_book_keeps_id_progress_and_bookmarks() {
    let l = lib();
    let a = l.upsert_book(&nb("/old/a.cbz", "h1", "A", 10), 1).unwrap();
    l.save_progress(a.id, 3, 5, 10).unwrap();
    l.add_bookmark(a.id, 2, "memo", 11).unwrap();

    let moved = l
        .upsert_book(&nb("/new/renamed.cbz", "h1", "renamed", 10), 20)
        .unwrap();
    assert_eq!(moved.id, a.id);
    assert_eq!(moved.uri, "/new/renamed.cbz");
    assert_eq!(l.get_progress(a.id).unwrap().unwrap().page_idx, 5);
    assert_eq!(l.bookmarks(a.id).unwrap().len(), 1);
}

#[test]
fn same_path_new_content_updates_in_place() {
    let l = lib();
    let a = l.upsert_book(&nb("/a.cbz", "h1", "A", 10), 1).unwrap();
    let b = l.upsert_book(&nb("/a.cbz", "h2", "A", 12), 2).unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!((b.head_hash.as_str(), b.page_count), ("h2", 12));
}

#[test]
fn moving_onto_a_stale_row_does_not_violate_unique_uri() {
    let l = lib();
    let a = l.upsert_book(&nb("/a.cbz", "h1", "A", 5), 1).unwrap();
    let stale = l.upsert_book(&nb("/b.cbz", "h2", "B", 5), 2).unwrap();
    // A が /b.cbz へ移動（B のあった場所に別の中身）
    let moved = l.upsert_book(&nb("/b.cbz", "h1", "A", 5), 3).unwrap();
    assert_eq!(moved.id, a.id);
    assert!(l.get_book(stale.id).is_err());
}

#[test]
fn progress_drives_status() {
    let l = lib();
    let b = l.upsert_book(&nb("/a", "h", "A", 10), 1).unwrap();
    assert_eq!(l.get_book(b.id).unwrap().status, ReadStatus::Unread);
    assert!(l.get_progress(b.id).unwrap().is_none());
    l.save_progress(b.id, 0, 0, 5).unwrap();
    assert_eq!(l.get_book(b.id).unwrap().status, ReadStatus::Reading);
    l.save_progress(b.id, 4, 9, 6).unwrap();
    assert_eq!(l.get_book(b.id).unwrap().status, ReadStatus::Finished);
    let p = l.get_progress(b.id).unwrap().unwrap();
    assert_eq!((p.spread_idx, p.page_idx, p.updated_at), (4, 9, 6));
    l.save_progress(b.id, 1, 2, 7).unwrap();
    assert_eq!(l.get_book(b.id).unwrap().status, ReadStatus::Reading);
}

#[test]
fn unknown_book_is_an_error() {
    let l = lib();
    assert!(matches!(
        l.save_progress(99, 0, 0, 1),
        Err(LibraryError::NotFound(99))
    ));
    assert!(l.add_bookmark(99, 0, "", 1).is_err());
    assert!(l.set_favorite(99, true).is_err());
    assert!(l.get_book(99).is_err());
}

#[test]
fn bookmarks_unique_per_page_and_removable() {
    let l = lib();
    let b = l.upsert_book(&nb("/a", "h", "A", 10), 1).unwrap();
    l.add_bookmark(b.id, 4, "first", 1).unwrap();
    l.add_bookmark(b.id, 4, "edited", 2).unwrap();
    l.add_bookmark(b.id, 1, "", 3).unwrap();
    let bm = l.bookmarks(b.id).unwrap();
    assert_eq!(bm.iter().map(|x| x.page_idx).collect::<Vec<_>>(), [1, 4]);
    assert_eq!(bm[1].note, "edited");
    l.remove_bookmark(bm[0].id).unwrap();
    assert_eq!(l.bookmarks(b.id).unwrap().len(), 1);
}

#[test]
fn filters_sort_and_hidden_shelf() {
    let l = lib();
    let a = l.upsert_book(&nb("/1", "h1", "Vol 10", 3), 10).unwrap();
    let b = l.upsert_book(&nb("/2", "h2", "Vol 2", 3), 20).unwrap();
    let c = l.upsert_book(&nb("/3", "h3", "Secret", 3), 30).unwrap();
    l.set_favorite(b.id, true).unwrap();
    l.set_hidden_shelf(c.id, true).unwrap();
    l.touch_opened(a.id, 99).unwrap();

    let titles = |f: &BookFilter, s| {
        l.list_books(f, s)
            .unwrap()
            .into_iter()
            .map(|b| b.title)
            .collect::<Vec<_>>()
    };
    let all = BookFilter::default();
    assert_eq!(titles(&all, BookSort::Name), ["Vol 2", "Vol 10"]); // 自然順・隠しは除外
    assert_eq!(
        titles(
            &BookFilter {
                include_hidden: true,
                ..all.clone()
            },
            BookSort::Added
        ),
        ["Secret", "Vol 2", "Vol 10"]
    );
    assert_eq!(
        titles(
            &BookFilter {
                favorite_only: true,
                ..all.clone()
            },
            BookSort::Name
        ),
        ["Vol 2"]
    );
    assert_eq!(
        titles(
            &BookFilter {
                query: Some("VOL 1".into()),
                ..all.clone()
            },
            BookSort::Name
        ),
        ["Vol 10"]
    );
    assert_eq!(
        titles(
            &BookFilter {
                status: Some(ReadStatus::Reading),
                ..all.clone()
            },
            BookSort::Name
        )
        .len(),
        0
    );
    assert_eq!(
        l.list_books(&all, BookSort::LastOpened).unwrap()[0].id,
        a.id
    );
}

#[test]
fn recent_orders_by_last_read_and_excludes_hidden() {
    let l = lib();
    let a = l.upsert_book(&nb("/1", "h1", "A", 9), 1).unwrap();
    let b = l.upsert_book(&nb("/2", "h2", "B", 9), 1).unwrap();
    let c = l.upsert_book(&nb("/3", "h3", "C", 9), 1).unwrap();
    l.save_progress(a.id, 0, 0, 10).unwrap();
    l.save_progress(b.id, 0, 0, 30).unwrap();
    l.save_progress(c.id, 0, 0, 20).unwrap();
    l.set_hidden_shelf(c.id, true).unwrap();
    let ids: Vec<_> = l.recent(10).unwrap().into_iter().map(|b| b.id).collect();
    assert_eq!(ids, [b.id, a.id]);
    assert_eq!(l.recent(1).unwrap().len(), 1);
}

#[test]
fn shelves_keep_insertion_order_and_cascade() {
    let l = lib();
    let a = l.upsert_book(&nb("/1", "h1", "A", 1), 1).unwrap();
    let b = l.upsert_book(&nb("/2", "h2", "B", 1), 1).unwrap();
    let s = l.create_shelf("Favorites").unwrap();
    assert_eq!(l.create_shelf("Favorites").unwrap(), s); // 同名は同じ棚
    assert!(l.create_shelf("  ").is_err());
    l.add_to_shelf(s.id, b.id).unwrap();
    l.add_to_shelf(s.id, a.id).unwrap();
    l.add_to_shelf(s.id, a.id).unwrap(); // 重複は無視
    let ids: Vec<_> = l
        .shelf_books(s.id)
        .unwrap()
        .into_iter()
        .map(|x| x.id)
        .collect();
    assert_eq!(ids, [b.id, a.id]);
    l.remove_from_shelf(s.id, b.id).unwrap();
    l.delete_book(a.id).unwrap();
    assert!(l.shelf_books(s.id).unwrap().is_empty());
    assert_eq!(l.shelves().unwrap(), [s]);
}

#[test]
fn book_settings_roundtrip_and_overwrite() {
    let l = lib();
    let b = l.upsert_book(&nb("/1", "h1", "A", 1), 1).unwrap();
    assert_eq!(l.book_settings(b.id).unwrap(), None);
    l.set_book_settings(b.id, "{\"a\":1}").unwrap();
    l.set_book_settings(b.id, "{\"a\":2}").unwrap();
    assert_eq!(l.book_settings(b.id).unwrap().as_deref(), Some("{\"a\":2}"));
}

#[test]
fn persists_across_reopen_with_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lib/library.db");
    let id = {
        let l = Library::open(&path).unwrap();
        let b = l.upsert_book(&nb("/a", "h", "A", 4), 1).unwrap();
        l.save_progress(b.id, 1, 2, 5).unwrap();
        b.id
    };
    let l = Library::open(&path).unwrap();
    assert_eq!(l.get_progress(id).unwrap().unwrap().page_idx, 2);
}

#[test]
fn page_sizes_roundtrip_and_reject_stale_cache() {
    let l = lib();
    let b = l.upsert_book(&nb("/a.cbz", "h1", "A", 3), 1).unwrap();
    assert_eq!(l.page_sizes(b.id, 3).unwrap(), None);

    let sizes = vec![(600, 900), (900, 600), (u32::MAX, 1)];
    l.save_page_sizes(b.id, &sizes).unwrap();
    assert_eq!(l.page_sizes(b.id, 3).unwrap(), Some(sizes.clone()));
    // ページ数が変わっていたら、古いキャッシュとして使わない
    assert_eq!(l.page_sizes(b.id, 4).unwrap(), None);

    // 上書き
    l.save_page_sizes(b.id, &[(1, 2)]).unwrap();
    assert_eq!(l.page_sizes(b.id, 1).unwrap(), Some(vec![(1, 2)]));
}

#[test]
fn page_sizes_are_removed_with_the_book() {
    let l = lib();
    let b = l.upsert_book(&nb("/a.cbz", "h1", "A", 1), 1).unwrap();
    l.save_page_sizes(b.id, &[(10, 20)]).unwrap();
    l.delete_book(b.id).unwrap();
    assert_eq!(l.page_sizes(b.id, 1).unwrap(), None);
}
