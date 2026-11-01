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
    assert_eq!(r.tap_action(1), None);
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
