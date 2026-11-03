use std::io::{Cursor, Write};
use std::path::Path;

use crate::*;
use std::sync::Arc;

fn png(w: u32, h: u32, v: u8) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba([v, v, v, 255]))
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

fn make_cbz(path: &Path, pages: usize) {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..pages {
        z.start_file(format!("{:03}.png", i + 1), opts).unwrap();
        z.write_all(&png(120, 180, (i as u8 + 1) * 20)).unwrap();
    }
    z.finish().unwrap();
}

fn setup(pages: usize) -> (tempfile::TempDir, std::sync::Arc<PvCore>, String) {
    let dir = tempfile::tempdir().unwrap();
    let cbz = dir.path().join("Vol 1.cbz");
    make_cbz(&cbz, pages);
    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    (dir, core, cbz.to_string_lossy().into_owned())
}

fn first_frame(r: &PvReader) -> PvFrame {
    for _ in 0..50 {
        match r.next_event(100) {
            Some(PvEvent::Frame { frame }) => return frame,
            Some(PvEvent::Failed { message, .. }) => panic!("{message}"),
            _ => {}
        }
    }
    panic!("no frame within 5s");
}

#[test]
fn open_register_render_and_list() {
    let (_d, core, cbz) = setup(5);
    let r = core.clone().open_path(cbz.clone()).unwrap();
    assert_eq!(r.page_count(), 5);
    assert_eq!(r.title(), "Vol 1");

    let t = r.request_frame(0, 200, 200, 1.0);
    let f = first_frame(&r);
    assert_eq!(f.ticket, t);
    assert_eq!(f.rgba.len(), (f.width * f.height * 4) as usize);
    assert_eq!((f.width, f.height), (133, 200)); // 120x180 を 200x200 に全体フィット

    let books = core.list_books(String::new(), false, PvSort::Name).unwrap();
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].page_count, 5);
    assert_eq!(books[0].uri, cbz);
    let thumb = core
        .thumbnail(books[0].id)
        .expect("thumbnail created at open");
    assert_eq!(thumb.rgba.len(), (thumb.width * thumb.height * 4) as usize);
}

#[test]
fn progress_bookmarks_and_layout_survive_reopen() {
    let (_d, core, cbz) = setup(8);
    let saved_page = {
        let r = core.clone().open_path(cbz.clone()).unwrap();
        r.go_to_page(6);
        r.save_progress(false).unwrap();
        let saved_page = r.current_page();
        assert_eq!(r.add_bookmark().unwrap(), r.current_page());
        r.set_layout(PvViewMode::Single, PvDirection::LeftToRight);
        assert!(r.apply(PvAction::FitWidth));
        saved_page
    };
    let r = core.clone().open_path(cbz).unwrap();
    // 保存されるのは見開きの先頭ページ。同じ見開きに戻ること
    assert_eq!(r.current_page(), saved_page);
    assert_eq!(r.bookmarks().unwrap().len(), 1);
    let l = r.layout();
    assert_eq!(
        (l.mode, l.direction, l.fit),
        (PvViewMode::Single, PvDirection::LeftToRight, PvFit::Width)
    );
}

#[test]
fn input_follows_binding_direction() {
    let (_d, core, cbz) = setup(3);
    let r = core.open_path(cbz).unwrap();
    // 既定は右綴じ: 左側タップで進む
    assert_eq!(r.tap_action(3), Some(PvAction::NextSpread));
    r.set_layout(PvViewMode::Double, PvDirection::LeftToRight);
    assert_eq!(r.tap_action(3), Some(PvAction::PrevSpread));
    assert_eq!(r.volume_action(false), Some(PvAction::NextSpread));
    // 中央の列は上・中・下のどこでもメニューの開閉
    assert_eq!(r.tap_action(1), Some(PvAction::OpenThumbnails));
    assert_eq!(r.tap_action(7), Some(PvAction::OpenThumbnails));
}

#[test]
fn navigation_and_ui_only_actions() {
    let (_d, core, cbz) = setup(6);
    let r = core.open_path(cbz).unwrap();
    assert!(r.apply(PvAction::LastPage));
    assert_eq!(r.cursor(), r.spread_count() - 1);
    assert!(!r.apply(PvAction::ZoomIn), "UI 側で扱う操作は false");
    assert!(r.apply(PvAction::FirstPage));
    assert_eq!(r.cursor(), 0);
}

#[test]
fn page_bytes_are_the_original_encoding() {
    let (_d, core, cbz) = setup(2);
    let r = core.open_path(cbz).unwrap();
    assert_eq!(r.page_bytes(0).unwrap(), png(120, 180, 20));
    assert_eq!(r.page_name(1), "002.png");
    assert!(r.page_bytes(99).is_err());
}

#[test]
fn favorites_filter_and_delete() {
    let (_d, core, cbz) = setup(2);
    core.clone().open_path(cbz).unwrap();
    let id = core.list_books(String::new(), false, PvSort::Name).unwrap()[0].id;
    assert!(core
        .list_books(String::new(), true, PvSort::Name)
        .unwrap()
        .is_empty());
    core.set_favorite(id, true).unwrap();
    assert_eq!(
        core.list_books(String::new(), true, PvSort::Name)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        core.recent_books(10).unwrap().len(),
        0,
        "未読了は履歴に載らない"
    );
    core.delete_book(id).unwrap();
    assert!(core
        .list_books(String::new(), false, PvSort::Name)
        .unwrap()
        .is_empty());
    assert!(core.thumbnail(id).is_none());
}

#[test]
fn not_an_archive_is_a_clean_error() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.cbz");
    std::fs::write(&bad, b"this is not a zip").unwrap();
    let core = PvCore::new(dir.path().join("d").to_string_lossy().into_owned()).unwrap();
    assert!(core
        .clone()
        .open_path(bad.to_string_lossy().into_owned())
        .is_err());
    assert!(core.open_path("/no/such/file.cbz".into()).is_err());
}

#[test]
fn opens_7z_through_the_same_path_and_cleans_scratch() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    for i in 0..3u8 {
        std::fs::write(
            src.join(format!("{:02}.png", i + 1)),
            png(80, 120, 30 * (i + 1)),
        )
        .unwrap();
    }
    let archive = dir.path().join("Vol 7.7z");
    sevenz_rust2::compress_to_path(&src, &archive).unwrap();
    let data = dir.path().join("data");
    let core = PvCore::new(data.to_string_lossy().into_owned()).unwrap();
    let r = core
        .open_path(archive.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(r.page_count(), 3);
    r.request_frame(0, 100, 100, 1.0);
    assert!(first_frame(&r).width > 0);
    let extract = data.join("extract");
    assert!(std::fs::read_dir(&extract).unwrap().count() > 0);
    drop(r);
    assert_eq!(std::fs::read_dir(&extract).unwrap().count(), 0);
}

#[test]
fn color_and_page_thumbnails_via_reader() {
    let (_d, core, cbz) = setup(4);
    let r = core.open_path(cbz).unwrap();
    assert_eq!(r.color(), PvColor::default());
    r.request_frame(0, 200, 200, 1.0);
    let before = first_frame(&r).rgba[0];
    r.set_color(PvColor {
        invert: true,
        ..Default::default()
    });
    assert!(r.color().invert);
    r.request_frame(0, 200, 200, 1.0);
    assert_eq!(first_frame(&r).rgba[0], 255 - before);

    let t = r.page_thumbnail(2, 60).unwrap();
    assert_eq!(t.height, 60);
    assert_eq!(t.rgba.len(), (t.width * t.height * 4) as usize);
    assert!(r.page_thumbnail(99, 60).is_none());
}

#[test]
fn color_setting_is_restored_on_reopen() {
    let (_d, core, cbz) = setup(3);
    let r = core.clone().open_path(cbz.clone()).unwrap();
    r.set_color(PvColor {
        sepia: true,
        brightness: 10,
        ..Default::default()
    });
    drop(r);
    let r = core.open_path(cbz).unwrap();
    assert!(r.color().sepia);
    assert_eq!(r.color().brightness, 10);
}

/// Kotlin 側の実装を模した PvTree（ファイル名と中身を持つ）。fd 版は unix のみなので列挙だけ確認する。
struct FakeTree(Vec<(String, u64)>);

impl PvTree for FakeTree {
    fn entries(&self) -> Result<Vec<PvTreeEntry>, PvError> {
        Ok(self
            .0
            .iter()
            .map(|(p, s)| PvTreeEntry {
                path: p.clone(),
                size: *s,
            })
            .collect())
    }
    fn open_fd(&self, _index: u32) -> Result<i32, PvError> {
        Err(PvError::new("no fd on this platform"))
    }
}

#[test]
fn tree_without_images_is_an_error_and_unreadable_pages_fail_cleanly() {
    let (_d, core, _) = setup(1);
    let none = Arc::new(FakeTree(vec![("memo.txt".into(), 3)]));
    assert!(core
        .clone()
        .open_tree(none, "content://t".into(), "t".into())
        .is_err());
    // 画像が列挙されても fd が開けなければ、開くことはできるがページ表示は Failed イベントになる
    let broken = Arc::new(FakeTree(vec![("a.png".into(), 3)]));
    let r = core
        .open_tree(broken, "content://t2".into(), "t2".into())
        .unwrap();
    r.request_frame(0, 100, 100, 1.0);
    let failed = (0..50).any(|_| matches!(r.next_event(100), Some(PvEvent::Failed { .. })));
    assert!(failed);
}

#[test]
fn epub_pages_follow_the_spine_not_the_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Comic.epub");
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts = zip::write::SimpleFileOptions::default();
    let mut add = |name: &str, data: &[u8]| {
        z.start_file(name, opts).unwrap();
        z.write_all(data).unwrap();
    };
    add("mimetype", b"application/epub+zip");
    add(
        "META-INF/container.xml",
        br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
    );
    // ファイル名の順は 001, 002, 003。読む順（spine）は 003, 001, 002
    add(
        "OEBPS/content.opf",
        br#"<package xmlns="http://www.idpf.org/2007/opf"><manifest>
            <item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/>
            <item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/>
            <item id="p3" href="p3.xhtml" media-type="application/xhtml+xml"/>
        </manifest><spine><itemref idref="p3"/><itemref idref="p1"/><itemref idref="p2"/></spine></package>"#,
    );
    for (i, v) in [(1u8, 30u8), (2, 90), (3, 150)] {
        add(
            &format!("OEBPS/p{i}.xhtml"),
            format!(r#"<html><body><img src="img/{i:03}.png"/></body></html>"#).as_bytes(),
        );
        add(&format!("OEBPS/img/{i:03}.png"), &png(100, 150, v));
    }
    z.finish().unwrap();

    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    let r = core.open_path(path.to_string_lossy().into_owned()).unwrap();
    assert_eq!(r.page_count(), 3);
    assert_eq!(r.page_name(0), "003.png");
    assert_eq!(r.page_name(1), "001.png");
    r.request_frame(0, 100, 150, 1.0);
    // 1 ページ目は spine の先頭（003.png = 明度 150）
    assert_eq!(first_frame(&r).rgba[0], 150);
}

#[test]
fn reopening_a_book_keeps_the_same_layout_with_cached_page_sizes() {
    let (_d, core, cbz) = setup(6);
    let first = core.clone().open_path(cbz.clone()).unwrap();
    let spreads = first.spread_count();
    drop(first);
    // 2 回目はページ寸法をキャッシュから読む。結果は同じでなければならない
    let second = core.open_path(cbz).unwrap();
    assert_eq!(second.spread_count(), spreads);
}

#[test]
fn settings_are_saved_and_applied_to_newly_opened_books() {
    let (d, core, cbz) = setup(4);
    let mut s = core.settings();
    assert_eq!(s.direction, PvDirection::RightToLeft);
    s.direction = PvDirection::LeftToRight;
    s.mode = PvViewMode::Single;
    s.prefetch_forward = 99; // 範囲外は収める
    core.set_settings(s).unwrap();
    assert!(core.settings().prefetch_forward <= 12);

    let r = core.clone().open_path(cbz).unwrap();
    let l = r.layout();
    assert_eq!(
        (l.mode, l.direction),
        (PvViewMode::Single, PvDirection::LeftToRight)
    );
    drop(r);

    // 再起動（新しい PvCore）後も残っている
    let core2 = PvCore::new(d.path().join("data").to_string_lossy().into_owned()).unwrap();
    assert_eq!(core2.settings().direction, PvDirection::LeftToRight);
}

#[test]
fn resetting_book_settings_lets_new_defaults_apply_again() {
    let (_d, core, cbz) = setup(4);
    let r = core.clone().open_path(cbz.clone()).unwrap();
    r.set_layout(PvViewMode::Single, PvDirection::RightToLeft); // 本ごとの設定として保存される
    drop(r);

    let mut s = core.settings();
    s.mode = PvViewMode::Double;
    s.direction = PvDirection::LeftToRight;
    core.set_settings(s).unwrap();
    let r = core.clone().open_path(cbz.clone()).unwrap();
    assert_eq!(
        r.layout().mode,
        PvViewMode::Single,
        "本ごとの設定が優先される"
    );
    drop(r);

    core.reset_book_settings().unwrap();
    let r = core.open_path(cbz).unwrap();
    assert_eq!(r.layout().mode, PvViewMode::Double);
    assert_eq!(r.layout().direction, PvDirection::LeftToRight);
}

#[test]
fn bookmarks_can_be_added_and_removed() {
    let (_d, core, cbz) = setup(5);
    let r = core.open_path(cbz).unwrap();
    r.go_to_page(3);
    let p = r.add_bookmark().unwrap();
    assert_eq!(r.bookmarks().unwrap(), [p]);
    r.remove_bookmark(p).unwrap();
    assert!(r.bookmarks().unwrap().is_empty());
    // 無いしおりの削除はエラーにしない
    r.remove_bookmark(p).unwrap();
}

#[test]
fn folder_by_path_opens_only_its_own_images_and_keeps_identity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("漫画 テスト");
    for (sub, name, v) in [
        (".", "000.png", 5u8),
        ("第1巻", "001.png", 10),
        ("第1巻", "002.png", 20),
        ("第2巻", "001.png", 30),
    ] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
        std::fs::write(root.join(sub).join(name), png(60, 90, v)).unwrap();
    }
    std::fs::write(root.join("メモ.txt"), b"memo").unwrap();
    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    let path = root.to_string_lossy().into_owned();

    // サブフォルダの画像は含めない（別の本として扱う）
    let r = core.clone().open_local(path.clone()).unwrap();
    assert_eq!(r.page_count(), 1);
    assert_eq!(r.title(), "漫画 テスト");
    let id = r.book_id();
    drop(r);

    // 同じフォルダは同じ本（読書位置・しおりを引き継ぐ）
    let again = core.clone().open_local(path).unwrap();
    assert_eq!(again.book_id(), id);
    let books = core.list_books(String::new(), false, PvSort::Name).unwrap();
    assert_eq!(books.len(), 1);
}

#[test]
fn folder_errors_are_readable_japanese() {
    let dir = tempfile::tempdir().unwrap();
    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    let msg = |r: PvResult<Arc<PvReader>>| match r {
        Err(PvError::Failed { msg }) => msg,
        Ok(_) => panic!("expected error"),
    };

    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let m = msg(core
        .clone()
        .open_local(empty.to_string_lossy().into_owned()));
    assert!(m.contains("画像が見つかりません"), "{m}");

    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("a.txt"), b"x").unwrap();
    let m = msg(core
        .clone()
        .open_local(notes.to_string_lossy().into_owned()));
    assert!(m.contains("画像が見つかりません"), "{m}");

    let m = msg(core.open_local(dir.path().join("nope").to_string_lossy().into_owned()));
    assert!(m.contains("見つかりません"), "{m}");
}

#[test]
fn image_file_names_are_recognized_by_extension() {
    assert!(is_image_file_name("001.JPG".into()));
    assert!(is_image_file_name("a/b/p.webp".into()));
    assert!(!is_image_file_name("memo.txt".into()));
    assert!(!is_image_file_name("noext".into()));
}

#[test]
fn scan_and_register_split_a_tree_into_books() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let put = |rel: &str, bytes: Vec<u8>| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    };
    put("top.png", png(60, 90, 1));
    put("第1巻/a.png", png(60, 90, 2));
    put("第1巻/b.png", png(60, 90, 3));
    put("第2巻/c.png", png(60, 90, 4));
    put("文字だけ/note.txt", b"t".to_vec());
    // 画像を持つ書庫（CBZ）は 1 冊
    let cbz = dir.path().join("x.cbz");
    make_cbz(&cbz, 1);
    std::fs::copy(&cbz, root.join("第2巻/vol.cbz")).unwrap();

    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    let items = core
        .scan_folder(root.to_string_lossy().into_owned())
        .unwrap();
    let got: Vec<_> = items
        .iter()
        .map(|i| (i.title.as_str(), i.is_folder))
        .collect();
    assert_eq!(
        got,
        vec![
            ("lib", true),
            ("第1巻", true),
            ("第2巻", true),
            ("vol", false)
        ]
    );

    // 一括登録: 開かずに本棚へ入り、ページ数が分かる
    for i in &items {
        core.register_local(i.path.clone()).unwrap();
    }
    let books = core.list_books(String::new(), false, PvSort::Name).unwrap();
    assert_eq!(books.len(), 4);
    let pages = |t: &str| books.iter().find(|b| b.title == t).unwrap().page_count;
    assert_eq!(
        (pages("lib"), pages("第1巻"), pages("第2巻"), pages("vol")),
        (1, 2, 1, 1)
    );
    assert!(books.iter().all(|b| core.thumbnail(b.id).is_some()));
    // 登録し直しても増えない
    core.register_local(items[1].path.clone()).unwrap();
    assert_eq!(
        core.list_books(String::new(), false, PvSort::Name)
            .unwrap()
            .len(),
        4
    );
}

/// 計測用: `PV_BENCH_DIR` のフォルダを一括登録して、直列と並列の時間を比べる（通常の実行では動かさない）。
#[test]
#[ignore]
fn bench_register_folder() {
    let root = std::env::var("PV_BENCH_DIR").expect("PV_BENCH_DIR");
    let run = |threads: usize| {
        let dir = tempfile::tempdir().unwrap();
        let core = PvCore::new(dir.path().to_string_lossy().into_owned()).unwrap();
        let items = core.scan_folder(root.clone()).unwrap();
        let t = std::time::Instant::now();
        let queue = std::sync::Mutex::new(items.iter());
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| loop {
                    let Some(i) = queue.lock().unwrap().next() else {
                        break;
                    };
                    core.register_local(i.path.clone()).unwrap();
                });
            }
        });
        eprintln!(
            "{} books, {threads} threads: {:?}",
            items.len(),
            t.elapsed()
        );
    };
    run(1);
    run(3);
    run(6);
}

/// 画像の無い（文章だけの）EPUB。日本語フォントが無い環境では、その旨のエラーになる。
#[test]
fn text_epub_opens_and_renders_pages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("小説.epub");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: String| {
            z.start_file(name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        };
        add(
            "META-INF/container.xml",
            r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#.into(),
        );
        add(
            "OEBPS/content.opf",
            r#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>
<item id="b" href="b.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#
                .into(),
        );
        for n in ["a", "b"] {
            add(
                &format!("OEBPS/{n}.xhtml"),
                format!(
                    "<html><body><h2>第{n}話</h2><p>{}</p></body></html>",
                    "これは本文です。".repeat(300)
                ),
            );
        }
        z.finish().unwrap();
    }
    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    match core.open_path(path.to_string_lossy().into_owned()) {
        Err(PvError::Failed { msg }) => {
            // フォントの無い環境（CI など）
            assert!(msg.contains("フォント"), "{msg}");
        }
        Ok(r) => {
            assert!(r.page_count() >= 4, "{}", r.page_count());
            assert_eq!(r.title(), "小説");
            r.request_frame(0, 540, 810, 1.0);
            let f = first_frame(&r);
            assert!(f.width > 100 && f.height > 100);
        }
    }
}

#[test]
fn groups_collect_books_and_survive_their_removal() {
    let (d, core, cbz) = setup(2);
    let cbz2 = d.path().join("Vol 2.cbz");
    make_cbz(&cbz2, 3);
    core.clone().open_path(cbz).unwrap();
    core.clone()
        .open_path(cbz2.to_string_lossy().into_owned())
        .unwrap();
    let ids: Vec<i64> = core
        .list_books(String::new(), false, PvSort::Name)
        .unwrap()
        .iter()
        .map(|b| b.id)
        .collect();
    assert_eq!(ids.len(), 2);

    let g = core.create_group("  シリーズ ".into()).unwrap();
    assert_eq!(g.name, "シリーズ");
    assert_eq!(core.create_group("シリーズ".into()).unwrap().id, g.id);
    core.add_to_group(g.id, ids.clone()).unwrap();
    core.add_to_group(g.id, ids.clone()).unwrap(); // 重複は無視
    assert_eq!(core.groups().unwrap()[0].count, 2);
    assert_eq!(core.group_books(g.id).unwrap().len(), 2);

    core.remove_from_group(g.id, vec![ids[0]]).unwrap();
    assert_eq!(core.group_books(g.id).unwrap().len(), 1);
    core.rename_group(g.id, "新しい名前".into()).unwrap();
    assert_eq!(core.groups().unwrap()[0].name, "新しい名前");
    core.delete_group(g.id).unwrap();
    assert!(core.groups().unwrap().is_empty());
    assert_eq!(
        core.list_books(String::new(), false, PvSort::Name)
            .unwrap()
            .len(),
        2
    );
}

/// 文字の大きさ・書体を変えると、開き直した本のページ数が変わる。設定は保存され、範囲外の値は収める。
#[test]
fn text_style_changes_pages_after_reopen_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("小説.epub");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: String| {
            z.start_file(name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        };
        add(
            "META-INF/container.xml",
            r#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#.into(),
        );
        add(
            "c.opf",
            r#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#.into(),
        );
        add(
            "a.xhtml",
            format!(
                "<html><body><p>{}</p></body></html>",
                "これは本文です。".repeat(600)
            ),
        );
        z.finish().unwrap();
    }
    let data = dir.path().join("data").to_string_lossy().into_owned();
    let core = PvCore::new(data.clone()).unwrap();
    assert_eq!(core.text_style().size, 44);
    let p = path.to_string_lossy().into_owned();
    let pages = |core: &Arc<PvCore>| match core.clone().open_path(p.clone()) {
        Ok(r) => Some((r.page_count(), r.is_text(), r.uri())),
        Err(PvError::Failed { msg }) => {
            assert!(msg.contains("フォント"), "{msg}");
            None
        }
    };
    let Some((normal, is_text, uri)) = pages(&core) else {
        return;
    };
    assert!(is_text);
    assert_eq!(uri, p);

    core.set_text_style(PvTextStyle {
        size: 64,
        typeface: PvTypeface::Gothic,
        line_spacing: 160,
        ink: 0x1C1C1C,
        paper: 0xF5F5F5,
        letter_spacing: 0,
        bold: false,
        italic: false,
        vertical: true,
    })
    .unwrap();
    let (large, _, _) = pages(&core).unwrap();
    assert!(large > normal, "{large} > {normal}");

    // 範囲外は収める。再起動しても保存されている
    core.set_text_style(PvTextStyle {
        size: 10_000,
        typeface: PvTypeface::Mincho,
        line_spacing: 9999,
        ink: 0xFF_FFFFFF,
        paper: 0x20_2020,
        letter_spacing: 999,
        bold: true,
        italic: true,
        vertical: false,
    })
    .unwrap();
    let saved = PvCore::new(data).unwrap().text_style();
    assert_eq!(saved.size, pv_container::MAX_FONT_PX);
    assert_eq!(saved.typeface, PvTypeface::Mincho);
    assert_eq!(saved.line_spacing, pv_container::MAX_LINE_SPACING);
    assert_eq!((saved.ink, saved.paper), (0xFF_FFFF, 0x20_2020));
    assert_eq!(saved.letter_spacing, pv_container::MAX_LETTER_SPACING);
    assert!(saved.bold && saved.italic);
    assert!(!saved.vertical, "横書きの設定が保存される");
    assert!(core.available_typefaces().contains(&PvTypeface::Gothic));
}

#[test]
fn browse_library_follows_where_books_were_opened_from() {
    let (d, core, cbz) = setup(1);
    let sub = d.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let cbz2 = sub.join("Vol 2.cbz");
    make_cbz(&cbz2, 2);
    core.clone().open_path(cbz).unwrap();
    core.clone()
        .open_path(cbz2.to_string_lossy().into_owned())
        .unwrap();

    let root = d.path().to_string_lossy().replace('\\', "/");
    let top = core
        .browse_library(root.clone(), false, PvSort::Name)
        .unwrap();
    assert_eq!(top.books.len(), 1, "直下は Vol 1 だけ");
    assert_eq!(top.folders.len(), 1);
    assert_eq!(
        (top.folders[0].name.as_str(), top.folders[0].count),
        ("sub", 1)
    );

    let all = core.browse_library(root, true, PvSort::Name).unwrap();
    assert_eq!(all.books.len(), 2);
    let inner = core
        .browse_library(top.folders[0].path.clone(), false, PvSort::Name)
        .unwrap();
    assert_eq!(inner.books.len(), 1);
    assert_eq!(inner.books[0].title, "Vol 2");
}

fn book(id: i64, title: &str, opened: Option<i64>, added: i64, size: u64) -> PvBook {
    PvBook {
        id,
        uri: format!("file:///b/{id}"),
        title: title.into(),
        page_count: 1,
        status: PvStatus::Unread,
        favorite: false,
        last_opened_at: opened,
        added_at: added,
        size,
    }
}

#[test]
fn books_sort_by_name_date_and_size_in_both_directions() {
    let books = vec![
        book(1, "第10巻", Some(30), 300, 50),
        book(2, "第2巻", None, 100, 90),
        book(3, "第1巻", Some(10), 200, 70),
    ];
    let titles = |sort, desc| -> Vec<String> {
        sort_books(books.clone(), sort, desc)
            .into_iter()
            .map(|b| b.title)
            .collect()
    };
    // 名前は自然順（2 が 10 より前）
    assert_eq!(titles(PvSort::Name, false), ["第1巻", "第2巻", "第10巻"]);
    assert_eq!(titles(PvSort::Name, true), ["第10巻", "第2巻", "第1巻"]);
    // 登録日
    assert_eq!(titles(PvSort::Added, false), ["第2巻", "第1巻", "第10巻"]);
    assert_eq!(titles(PvSort::Added, true), ["第10巻", "第1巻", "第2巻"]);
    // 最近開いた順: 開いていない本は、昇順で先頭・降順で最後
    assert_eq!(
        titles(PvSort::LastOpened, true),
        ["第10巻", "第1巻", "第2巻"]
    );
    assert_eq!(
        titles(PvSort::LastOpened, false),
        ["第2巻", "第1巻", "第10巻"]
    );
    // サイズ
    assert_eq!(titles(PvSort::Size, false), ["第10巻", "第1巻", "第2巻"]);
    assert_eq!(titles(PvSort::Size, true), ["第2巻", "第1巻", "第10巻"]);
}

#[test]
fn equal_sort_keys_fall_back_to_name_then_id() {
    let books = vec![
        book(5, "b", None, 1, 1),
        book(4, "a", None, 1, 1),
        book(3, "a", None, 1, 1),
    ];
    let ids = |desc| -> Vec<i64> {
        sort_books(books.clone(), PvSort::Added, desc)
            .into_iter()
            .map(|b| b.id)
            .collect()
    };
    // 同じ値の本の並びは、昇順・降順で変わらない
    assert_eq!(ids(false), [3, 4, 5]);
    assert_eq!(ids(true), [3, 4, 5]);
}

#[test]
fn reader_exposes_the_epub_toc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("本.epub");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        let mut add = |name: &str, body: Vec<u8>| {
            z.start_file(name, opts).unwrap();
            z.write_all(&body).unwrap();
        };
        add(
            "META-INF/container.xml",
            br#"<container><rootfiles><rootfile full-path="c.opf"/></rootfiles></container>"#
                .to_vec(),
        );
        add(
            "c.opf",
            br#"<package><manifest><item id="n" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/>
<item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="b.xhtml" media-type="application/xhtml+xml"/>
<item id="i1" href="1.png" media-type="image/png"/><item id="i2" href="2.png" media-type="image/png"/></manifest>
<spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#.to_vec(),
        );
        add("nav.xhtml", r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="a.xhtml">序章</a></li><li><a href="b.xhtml">本編</a></li></ol></nav></body></html>"#.as_bytes().to_vec());
        add(
            "a.xhtml",
            br#"<html><body><img src="1.png"/></body></html>"#.to_vec(),
        );
        add(
            "b.xhtml",
            br#"<html><body><img src="2.png"/></body></html>"#.to_vec(),
        );
        add("1.png", png(60, 90, 10));
        add("2.png", png(60, 90, 20));
        z.finish().unwrap();
    }
    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    let r = core.open_path(path.to_string_lossy().into_owned()).unwrap();
    let toc = r.toc();
    assert_eq!(toc.len(), 2);
    assert_eq!(
        (toc[0].title.as_str(), toc[0].page, toc[0].level),
        ("序章", 0, 0)
    );
    assert_eq!((toc[1].title.as_str(), toc[1].page), ("本編", 1));
}

/// 計測用: 多数の本を続けて登録する時間
/// （`cargo test -p pv-ffi --release -- --ignored --nocapture bench_bulk_register`）
#[test]
#[ignore]
fn bench_bulk_register() {
    const BOOKS: usize = 400;
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<String> = (0..BOOKS)
        .map(|n| {
            let p = dir.path().join(format!("book{n:04}.cbz"));
            let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            for i in 0..3 {
                z.start_file(format!("{:03}.png", i + 1), opts).unwrap();
                // 本ごとに中身を変える（同じ内容は同じ本として扱われるため）
                z.write_all(&png(
                    200 + (n % 50) as u32,
                    300 + (n / 50) as u32,
                    i as u8 * 40,
                ))
                .unwrap();
            }
            z.finish().unwrap();
            p.to_string_lossy().into_owned()
        })
        .collect();
    let core = PvCore::new(dir.path().join("data").to_string_lossy().into_owned()).unwrap();
    let t = std::time::Instant::now();
    for p in &paths {
        core.register_local(p.clone()).unwrap();
    }
    let total = t.elapsed();
    println!(
        "registered {BOOKS} books in {total:?} ({:?}/book)",
        total / BOOKS as u32
    );
}
