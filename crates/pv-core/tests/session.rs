use std::path::Path;
use std::time::{Duration, Instant};

use pv_core::{Action, BookSession, CoreEvent, Direction, Settings, Size, ViewMode};

fn write_png(path: &Path, w: u32, h: u32, v: u8) {
    image::RgbaImage::from_pixel(w, h, image::Rgba([v, v, v, 255]))
        .save(path)
        .unwrap();
}

fn book(pages: &[(u32, u32)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (i, &(w, h)) in pages.iter().enumerate() {
        write_png(
            &dir.path().join(format!("{:03}.png", i + 1)),
            w,
            h,
            (i as u8 + 1) * 10,
        );
    }
    dir
}

fn settings() -> Settings {
    let mut s = Settings::default();
    s.cache.l1_bytes = Some(64 << 20);
    s
}

fn open(dir: &tempfile::TempDir, s: &Settings) -> BookSession {
    BookSession::open(dir.path().to_str().unwrap(), s).unwrap()
}

fn wait_event(rx: &crossbeam_channel::Receiver<CoreEvent>) -> CoreEvent {
    rx.recv_timeout(Duration::from_secs(5))
        .expect("event within 5s")
}

const VP: Size = Size { w: 200, h: 200 };

#[test]
fn opens_and_renders_single_page_fit() {
    let dir = book(&[(100, 200), (100, 200), (100, 200)]);
    let s = open(&dir, &settings());
    assert_eq!(s.page_count(), 3);
    assert_eq!(s.spread_count(), 2); // 表紙単独 + 見開き
    let rx = s.events();
    let t = s.request_frame(0, VP, 1.0);
    match wait_event(&rx) {
        CoreEvent::FrameReady { ticket, frame } => {
            assert_eq!(ticket, t);
            assert_eq!(frame.pages, [0]);
            assert_eq!((frame.bitmap.size.w, frame.bitmap.size.h), (100, 200));
            assert_eq!(&frame.bitmap.data[..4], &[10, 10, 10, 255]);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn right_to_left_spread_puts_next_page_on_the_left() {
    let dir = book(&[(100, 200), (100, 200), (100, 200)]);
    let s = open(&dir, &settings());
    let rx = s.events();
    s.request_frame(1, VP, 1.0);
    let CoreEvent::FrameReady { frame, .. } = wait_event(&rx) else {
        panic!()
    };
    assert_eq!(frame.pages, [2, 1]); // 画面左がページ3、右がページ2
    assert_eq!(frame.bitmap.size.w, 100 * 2);
    assert_eq!(frame.bitmap.data[0], 30); // 左端はページ3の色
}

#[test]
fn wide_page_is_laid_out_alone() {
    let dir = book(&[(100, 200), (300, 100), (100, 200), (100, 200)]);
    let s = open(&dir, &settings());
    assert_eq!(s.spread_count(), 3);
}

#[test]
fn go_to_cancels_stale_requests() {
    let dir = book(&[(400, 400); 6]);
    let mut s = open(&dir, &settings());
    let rx = s.events();
    let old = s.request_frame(1, VP, 1.0);
    s.go_to(2); // 世代が進む。old は完了済みかキャンセルのどちらか
    let new = s.request_frame(2, VP, 1.0);
    let mut got_new = false;
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && !got_new {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(CoreEvent::FrameReady { ticket, .. }) if ticket == new => got_new = true,
            Ok(CoreEvent::FrameReady { ticket, .. } | CoreEvent::Cancelled { ticket }) => {
                assert_eq!(ticket, old, "only the stale ticket may resolve otherwise");
            }
            Ok(CoreEvent::Error { message, .. }) => panic!("{message}"),
            Err(_) => {}
        }
    }
    assert!(got_new);
}

#[test]
fn prefetch_warms_l1_after_go_to() {
    let dir = book(&[(300, 300); 8]);
    let mut s = open(&dir, &settings());
    let rx = s.events();
    s.request_frame(0, VP, 1.0);
    wait_event(&rx);
    let before = s.l1_bytes();
    s.go_to(1);
    s.request_frame(1, VP, 1.0);
    wait_event(&rx);
    let end = Instant::now() + Duration::from_secs(5);
    while s.l1_bytes() <= before * 3 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        s.l1_bytes() > before * 3,
        "prefetch should have cached following spreads"
    );
}

#[test]
fn identical_requests_share_one_job_and_all_get_notified() {
    let dir = book(&[(300, 300), (300, 300)]);
    let s = open(&dir, &settings());
    let rx = s.events();
    let a = s.request_frame(0, VP, 1.0);
    let b = s.request_frame(0, VP, 1.0);
    let mut seen = vec![];
    for _ in 0..2 {
        if let CoreEvent::FrameReady { ticket, .. } = wait_event(&rx) {
            seen.push(ticket);
        }
    }
    seen.sort_by_key(|t| t.id);
    assert_eq!(seen, [a, b]);
}

#[test]
fn corrupt_page_reports_error_without_crashing() {
    let dir = book(&[(100, 100), (100, 100)]);
    std::fs::write(dir.path().join("002.png"), b"not really a png").unwrap();
    let mut st = settings();
    st.layout.mode = ViewMode::Single;
    let s = open(&dir, &st);
    let rx = s.events();
    s.request_frame(1, VP, 1.0);
    assert!(matches!(wait_event(&rx), CoreEvent::Error { .. }));
    // 他のページは引き続き表示できる
    s.request_frame(0, VP, 1.0);
    assert!(matches!(wait_event(&rx), CoreEvent::FrameReady { .. }));
}

#[test]
fn out_of_range_request_is_an_error_event() {
    let dir = book(&[(100, 100)]);
    let s = open(&dir, &settings());
    let rx = s.events();
    s.request_frame(99, VP, 1.0);
    assert!(matches!(wait_event(&rx), CoreEvent::Error { .. }));
}

#[test]
fn navigation_actions_and_shift() {
    let dir = book(&[(100, 200); 5]);
    let mut s = open(&dir, &settings());
    assert_eq!(s.cursor(), 0);
    assert!(s.apply(&Action::NextSpread));
    assert_eq!(s.cursor(), 1);
    assert_eq!(s.current_page(), 1);
    assert!(s.apply(&Action::LastPage));
    assert_eq!(s.cursor(), s.spread_count() - 1);
    assert!(s.apply(&Action::FirstPage));
    assert!(s.apply(&Action::PrevSpread));
    assert_eq!(s.cursor(), 0); // 端で止まる
    assert!(!s.apply(&Action::AddBookmark)); // UI 側で処理するもの

    s.go_to(1);
    assert!(s.apply(&Action::ShiftSpread));
    assert!(s.settings().layout.shift);
    assert_eq!(s.spread_count(), 3); // 先頭が見開きになり位相が変わる
    assert_eq!(
        s.spread_of_page(1),
        Some(s.cursor()),
        "ずれ補正後も同じページを表示し続ける"
    );
}

#[test]
fn ltr_layout_via_set_layout() {
    let dir = book(&[(100, 200); 3]);
    let mut s = open(&dir, &settings());
    let mut l = s.settings().layout;
    l.direction = Direction::LeftToRight;
    s.set_layout(l);
    let rx = s.events();
    s.request_frame(1, VP, 1.0);
    let CoreEvent::FrameReady { frame, .. } = wait_event(&rx) else {
        panic!()
    };
    assert_eq!(frame.pages, [1, 2]);
}

#[test]
fn trim_memory_keeps_window_and_frees_the_rest() {
    let dir = book(&[(200, 200); 12]);
    let mut st = settings();
    st.layout.mode = ViewMode::Single;
    st.cache.prefetch_forward = 1;
    st.cache.prefetch_back = 0;
    let mut s = open(&dir, &st);
    let rx = s.events();
    for i in 0..12 {
        s.go_to(i);
        s.request_frame(i, VP, 1.0);
        wait_event(&rx);
    }
    let full = s.l1_bytes();
    s.go_to(11);
    s.trim_memory();
    assert!(s.l1_bytes() < full / 2);
}

#[test]
fn empty_book_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("readme.txt"), b"x").unwrap();
    assert!(BookSession::open(dir.path().to_str().unwrap(), &settings()).is_err());
}

#[test]
fn set_color_changes_rendered_pixels_and_resets_with_identity() {
    use pv_core::ColorAdjust;
    let dir = book(&[(100, 200), (100, 200)]);
    let mut s = open(&dir, &settings());
    let rx = s.events();
    let pixel = |s: &BookSession, rx: &crossbeam_channel::Receiver<CoreEvent>| {
        s.request_frame(0, VP, 1.0);
        match wait_event(rx) {
            CoreEvent::FrameReady { frame, .. } => frame.bitmap.data[0],
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(pixel(&s, &rx), 10);
    s.set_color(ColorAdjust {
        invert: true,
        ..Default::default()
    });
    assert_eq!(pixel(&s, &rx), 245);
    s.set_color(ColorAdjust::default());
    assert_eq!(pixel(&s, &rx), 10);
}

#[test]
fn page_thumbnails_are_small_and_do_not_need_the_session_lock() {
    let dir = book(&[(400, 200), (100, 300)]);
    let s = open(&dir, &settings());
    let thumbs = s.thumbnailer();
    let t = thumbs.get(0, 100).unwrap();
    assert_eq!((t.size.w, t.size.h), (100, 50));
    let small = thumbs.get(1, 1000).unwrap();
    assert_eq!((small.size.w, small.size.h), (100, 300), "no upscaling");
    assert!(thumbs.get(9, 100).is_err());
}

#[test]
fn color_override_is_merged_per_book() {
    use pv_core::{BookOverride, ColorAdjust};
    let o = BookOverride {
        color: Some(ColorAdjust {
            sepia: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(Settings::default().merged(&o).color.sepia);
    assert!(!Settings::default().color.sepia);
}

/// read_prefix の呼び出し回数を数えるコンテナ（寸法の取得が行われたかを確かめる）。
struct Counting {
    inner: pv_container::FolderContainer,
    prefix_calls: std::sync::atomic::AtomicUsize,
}

impl pv_core::Container for Counting {
    fn entries(&self) -> &[pv_container::EntryMeta] {
        self.inner.entries()
    }
    fn read(&self, id: pv_container::EntryId) -> pv_container::Result<bytes::Bytes> {
        self.inner.read(id)
    }
    fn open_reader(
        &self,
        id: pv_container::EntryId,
    ) -> pv_container::Result<Box<dyn std::io::Read + Send>> {
        self.inner.open_reader(id)
    }
    fn read_prefix(
        &self,
        id: pv_container::EntryId,
        n: usize,
    ) -> pv_container::Result<bytes::Bytes> {
        self.prefix_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.inner.read_prefix(id, n)
    }
}

fn counting(dir: &tempfile::TempDir) -> Arc<Counting> {
    Arc::new(Counting {
        inner: pv_container::FolderContainer::open(dir.path()).unwrap(),
        prefix_calls: Default::default(),
    })
}

use std::sync::Arc;

#[test]
fn page_sizes_are_probed_once_and_cached_sizes_skip_probing() {
    use std::sync::atomic::Ordering::SeqCst;
    // 3 枚目だけ横長 → 見開きの組み方が変わる
    let dir = book(&[(100, 200), (100, 200), (300, 200), (100, 200), (100, 200)]);
    let s = settings();

    let c = counting(&dir);
    let first = BookSession::open_container(c.clone(), 1, &s).unwrap();
    assert_eq!(c.prefix_calls.load(SeqCst), 5, "全ページを 1 回ずつ調べる");
    let sizes = first.page_sizes().expect("寸法を取得済み");
    assert_eq!(
        sizes,
        [(100, 200), (100, 200), (300, 200), (100, 200), (100, 200)]
    );

    // 保存済みの寸法を渡せば、読み込みは行わず、同じ見開きになる
    let c2 = counting(&dir);
    let second = BookSession::open_container_with_sizes(c2.clone(), 1, &s, Some(&sizes)).unwrap();
    assert_eq!(c2.prefix_calls.load(SeqCst), 0);
    assert_eq!(second.spread_count(), first.spread_count());
    for i in 0..first.spread_count() {
        assert_eq!(second.spread(i), first.spread(i));
    }

    // ページ数の合わないキャッシュは使わない（読み直す）
    let c3 = counting(&dir);
    let _ = BookSession::open_container_with_sizes(c3.clone(), 1, &s, Some(&sizes[..2])).unwrap();
    assert_eq!(c3.prefix_calls.load(SeqCst), 5);
}

#[test]
fn single_page_mode_does_not_probe_until_layout_needs_sizes() {
    use std::sync::atomic::Ordering::SeqCst;
    let dir = book(&[(100, 200), (300, 200), (100, 200)]);
    let mut s = settings();
    s.layout.mode = ViewMode::Single;
    let c = counting(&dir);
    let mut session = BookSession::open_container(c.clone(), 1, &s).unwrap();
    assert_eq!(c.prefix_calls.load(SeqCst), 0);
    assert!(session.page_sizes().is_none(), "未取得のときは保存しない");

    let mut layout = session.settings().layout;
    layout.mode = ViewMode::Double;
    session.set_layout(layout);
    assert_eq!(c.prefix_calls.load(SeqCst), 3);
    assert!(session.page_sizes().is_some());
    // 以後のレイアウト変更では、読み直さない
    layout.direction = Direction::LeftToRight;
    session.set_layout(layout);
    assert_eq!(c.prefix_calls.load(SeqCst), 3);
}
