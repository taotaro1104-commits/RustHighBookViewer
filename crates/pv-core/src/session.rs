//! BookSession: 1 冊を開いている状態のファサード。UI はこれだけを触る。

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use pv_container::{open_path, Container, EntryKind};
use pv_decode::{Decoder, ImageDecoder, Size};
use pv_layout::{build_spreads, spread_of_page, LayoutOptions, PageMeta, Spread, ViewMode};
use pv_process::ColorAdjust;
use pv_render::FitMode;

use crate::cache::ByteLru;
use crate::error::{CoreError, Result};
use crate::frame::{CoreEvent, Frame, FrameTicket};
use crate::input::Action;
use crate::jobs::JobQueue;
use crate::render::Inner;
use crate::settings::Settings;

/// 画像ヘッダから寸法を読むための先頭バイト数
const PROBE_BYTES: usize = 64 * 1024;
/// 先読みで作るビットマップの倍率の上限
const PREFETCH_MAX_SCALE: f32 = 2.0;
const PLACEHOLDER: (u32, u32) = (100, 150);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FrameKey {
    spread: usize,
    vw: u32,
    vh: u32,
    scale_bits: u32,
    fit: u8,
    color: ColorAdjust,
    generation: u64,
}

type Inflight = Arc<Mutex<HashMap<FrameKey, Vec<FrameTicket>>>>;

pub struct BookSession {
    inner: Arc<Inner>,
    queue: JobQueue,
    tx: Sender<CoreEvent>,
    rx: Receiver<CoreEvent>,
    inflight: Inflight,
    next_ticket: AtomicU64,
    last_view: Mutex<Option<(Size, f32)>>,
    settings: Settings,
    metas: Vec<PageMeta>,
    spreads: Vec<Spread>,
    cursor: usize,
}

/// ページ一覧用のサムネイル生成。BookSession から独立して並列に呼べる。
#[derive(Clone)]
pub struct PageThumbs(Arc<Inner>);

impl PageThumbs {
    /// 処理前の元バイト列。セッションのロックを取らないので、遅い読み出しが他の操作を止めない。
    pub fn bytes(&self, page: usize) -> Result<Bytes> {
        self.0.raw(page)
    }

    pub fn get(&self, page: usize, edge: u32) -> Result<pv_decode::Bitmap> {
        self.0.page_thumbnail(page, edge)
    }
}

fn hash_str(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn fit_id(f: FitMode) -> u8 {
    match f {
        FitMode::Width => 0,
        FitMode::Height => 1,
        FitMode::Page => 2,
    }
}

impl BookSession {
    pub fn open(uri: &str, settings: &Settings) -> Result<Self> {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        let container = open_path(Path::new(path))?;
        Self::open_container(container, hash_str(uri), settings)
    }

    pub fn open_container(
        container: Arc<dyn Container>,
        book_id: u64,
        settings: &Settings,
    ) -> Result<Self> {
        let pages: Vec<_> = container
            .entries()
            .iter()
            .filter(|e| e.kind == EntryKind::Image)
            .map(|e| e.id)
            .collect();
        if pages.is_empty() {
            return Err(CoreError::NoPages);
        }
        let cache = &settings.cache;
        let inner = Arc::new(Inner {
            container,
            book_id,
            pages,
            decoder: ImageDecoder::default(),
            l1: Mutex::new(ByteLru::new(cache.l1_budget())),
            l2: Mutex::new(ByteLru::new(cache.l2_bytes)),
            generation: AtomicU64::new(0),
        });
        let (tx, rx) = crossbeam_channel::unbounded();
        let workers = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 4));
        let mut s = Self {
            metas: probe_metas(&inner, needs_sizes(&settings.layout)),
            inner,
            queue: JobQueue::new(workers),
            tx,
            rx,
            inflight: Arc::default(),
            next_ticket: AtomicU64::new(1),
            last_view: Mutex::new(None),
            settings: settings.clone(),
            spreads: Vec::new(),
            cursor: 0,
        };
        s.relayout();
        Ok(s)
    }

    fn relayout(&mut self) {
        self.spreads = build_spreads(&self.metas, &self.settings.layout);
        self.cursor = self.cursor.min(self.spreads.len().saturating_sub(1));
    }

    pub fn page_count(&self) -> usize {
        self.inner.pages.len()
    }

    pub fn spread_count(&self) -> usize {
        self.spreads.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn spread(&self, idx: usize) -> Option<Spread> {
        self.spreads.get(idx).copied()
    }

    pub fn page_path(&self, page: usize) -> Option<String> {
        let id = *self.inner.pages.get(page)?;
        self.inner
            .container
            .entries()
            .get(id.0 as usize)
            .map(|e| e.path.clone())
    }

    /// 現在のスプレッドの先頭ページ（読む順）。読書位置の保存に使う。
    pub fn current_page(&self) -> usize {
        self.spreads
            .get(self.cursor)
            .and_then(|s| s.pages(self.settings.layout.direction).first().copied())
            .unwrap_or(0)
    }

    pub fn spread_of_page(&self, page: usize) -> Option<usize> {
        spread_of_page(&self.spreads, page)
    }

    /// 処理前の元バイト列（画像保存・共有用。劣化しない）。
    pub fn page_bytes(&self, page: usize) -> Result<Bytes> {
        self.inner.raw(page)
    }

    pub fn events(&self) -> Receiver<CoreEvent> {
        self.rx.clone()
    }

    /// スプレッドへ移動する。世代を進めて古いジョブを破棄し、先読み窓を更新する。
    pub fn go_to(&mut self, idx: usize) {
        self.cursor = idx.min(self.spreads.len().saturating_sub(1));
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        self.prefetch();
    }

    pub fn go_to_page(&mut self, page: usize) {
        if let Some(i) = self.spread_of_page(page) {
            self.go_to(i);
        }
    }

    fn prefetch(&self) {
        let Some((vp, scale)) = *self.last_view.lock() else {
            return;
        };
        let c = &self.settings.cache;
        let fwd = (1..=c.prefetch_forward).map(|d| self.cursor + d);
        let back = (1..=c.prefetch_back).filter_map(|d| self.cursor.checked_sub(d));
        // 拡大中でも先読みは等倍付近に抑える（8 倍の全面ビットマップを複数枚同時に作ると Android で OOM になる）
        let scale = scale.min(PREFETCH_MAX_SCALE);
        for idx in fwd.chain(back).filter(|&i| i < self.spreads.len()) {
            self.enqueue(idx, vp, scale, None);
        }
    }

    pub fn request_frame(&self, idx: usize, viewport: Size, scale: f32) -> FrameTicket {
        *self.last_view.lock() = Some((viewport, scale));
        let ticket = FrameTicket {
            id: self.next_ticket.fetch_add(1, Ordering::Relaxed),
            spread: idx,
            generation: self.inner.generation.load(Ordering::Acquire),
        };
        if idx >= self.spreads.len() {
            let _ = self.tx.send(CoreEvent::Error {
                ticket,
                message: format!("spread {idx} out of range"),
            });
        } else {
            self.enqueue(idx, viewport, scale, Some(ticket));
        }
        ticket
    }

    fn enqueue(&self, idx: usize, vp: Size, scale: f32, waiter: Option<FrameTicket>) {
        let gen = self.inner.generation.load(Ordering::Acquire);
        let fit = self.settings.fit;
        let color = self.settings.color;
        let key = FrameKey {
            spread: idx,
            vw: vp.w,
            vh: vp.h,
            scale_bits: scale.to_bits(),
            fit: fit_id(fit),
            color,
            generation: gen,
        };
        {
            let mut map = self.inflight.lock();
            if let Some(waiters) = map.get_mut(&key) {
                waiters.extend(waiter); // 同じキーのジョブは 1 つにまとめる
                return;
            }
            map.insert(key, waiter.into_iter().collect());
        }
        let dir = self.settings.layout.direction;
        // 画面左→右の順に並べる（右綴じの見開きは読む順と逆）
        let mut pages = self.spreads[idx].pages(dir);
        if dir == pv_layout::Direction::RightToLeft {
            pages.reverse();
        }
        let prio = match idx as isize - self.cursor as isize {
            0 => 4,
            1 => 3,
            -1 => 2,
            _ => 1,
        };
        let (inner, inflight, tx) = (
            Arc::clone(&self.inner),
            Arc::clone(&self.inflight),
            self.tx.clone(),
        );
        self.queue.submit(prio, move || {
            let res = if inner.generation.load(Ordering::Acquire) != gen {
                Err(CoreError::Cancelled)
            } else {
                catch_unwind(AssertUnwindSafe(|| {
                    inner.render_spread(&pages, vp, scale, fit, color, gen)
                }))
                .unwrap_or_else(|_| Err(CoreError::Panic("render".into())))
            };
            let waiters = inflight.lock().remove(&key).unwrap_or_default();
            let frame = res.as_ref().ok().map(|bmp| {
                Arc::new(Frame {
                    spread: idx,
                    pages: pages.clone(),
                    bitmap: bmp.clone(),
                    viewport: vp,
                    scale,
                })
            });
            for t in waiters {
                let ev = match (&res, &frame) {
                    (Ok(_), Some(f)) => CoreEvent::FrameReady {
                        ticket: t,
                        frame: Arc::clone(f),
                    },
                    (Err(CoreError::Cancelled), _) => CoreEvent::Cancelled { ticket: t },
                    (Err(e), _) => CoreEvent::Error {
                        ticket: t,
                        message: e.to_string(),
                    },
                    (Ok(_), None) => unreachable!("frame is built whenever res is Ok"),
                };
                let _ = tx.send(ev);
            }
        });
    }

    pub fn set_layout(&mut self, layout: LayoutOptions) {
        let page = self.current_page();
        if needs_sizes(&layout) && !needs_sizes(&self.settings.layout) {
            self.metas = probe_metas(&self.inner, true);
        }
        self.settings.layout = layout;
        self.relayout();
        self.go_to_page(page);
    }

    pub fn set_fit(&mut self, fit: FitMode) {
        self.settings.fit = fit;
        self.go_to(self.cursor);
    }

    /// 色調整を変える。先読み済みのフレームは古い色なので世代を進めて作り直す。
    pub fn set_color(&mut self, color: ColorAdjust) {
        self.settings.color = color.clamped();
        self.go_to(self.cursor);
    }

    /// ページ一覧用サムネイルを作る窓口。セッションのロックを持たずに呼べる。
    pub fn thumbnailer(&self) -> PageThumbs {
        PageThumbs(Arc::clone(&self.inner))
    }

    /// ナビゲーション・レイアウト系の Action を処理する。UI 側で扱うものは false を返す。
    pub fn apply(&mut self, action: &Action) -> bool {
        let dir = self.settings.layout.direction;
        let cur = self.spreads.get(self.cursor).map(|s| s.pages(dir));
        match action {
            Action::NextSpread => self.go_to((self.cursor + 1).min(self.spreads.len() - 1)),
            Action::PrevSpread => self.go_to(self.cursor.saturating_sub(1)),
            Action::NextPage => {
                if let Some(p) = cur.and_then(|p| p.last().copied()) {
                    self.go_to_page((p + 1).min(self.page_count() - 1));
                }
            }
            Action::PrevPage => {
                if let Some(p) = cur.and_then(|p| p.first().copied()) {
                    self.go_to_page(p.saturating_sub(1));
                }
            }
            Action::FirstPage => self.go_to(0),
            Action::LastPage => self.go_to(self.spreads.len() - 1),
            Action::ShiftSpread => {
                let l = LayoutOptions {
                    shift: !self.settings.layout.shift,
                    ..self.settings.layout
                };
                self.set_layout(l);
            }
            Action::FitWidth => self.set_fit(FitMode::Width),
            Action::FitHeight => self.set_fit(FitMode::Height),
            Action::FitPage => self.set_fit(FitMode::Page),
            _ => return false,
        }
        true
    }

    /// onTrimMemory 相当: 先読み窓の外の L1 を解放する。
    pub fn trim_memory(&self) {
        let c = &self.settings.cache;
        let lo = self.cursor.saturating_sub(c.prefetch_back);
        let hi = self.cursor + c.prefetch_forward;
        let dir = self.settings.layout.direction;
        let keep: Vec<u32> = (lo..=hi)
            .filter_map(|i| self.spreads.get(i))
            .flat_map(|s| s.pages(dir))
            .filter_map(|p| self.inner.pages.get(p).map(|id| id.0))
            .collect();
        self.inner.l1.lock().retain(|k| keep.contains(&k.entry));
    }

    pub fn l1_bytes(&self) -> usize {
        self.inner.l1.lock().bytes()
    }
}

impl Drop for BookSession {
    fn drop(&mut self) {
        // 待機中のジョブを世代違いとして破棄させる
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
    }
}

fn needs_sizes(l: &LayoutOptions) -> bool {
    matches!(l.mode, ViewMode::Double | ViewMode::Auto)
        && (l.wide_page_alone || l.mode == ViewMode::Auto)
}

/// 先頭だけ読んでヘッダから寸法を得る。判別できないページは縦長の仮寸法を使う。
fn probe_metas(inner: &Inner, probe: bool) -> Vec<PageMeta> {
    inner
        .pages
        .iter()
        .map(|&id| {
            let size = probe
                .then(|| {
                    let head = inner.container.read_prefix(id, PROBE_BYTES).ok()?;
                    inner.decoder.probe_size(&head).ok().or_else(|| {
                        // 先頭 64KB に SOF が収まらない JPEG 等は全体を読んで再試行
                        let all = inner.container.read(id).ok()?;
                        inner.decoder.probe_size(&all).ok()
                    })
                })
                .flatten();
            let (width, height) = size.map_or(PLACEHOLDER, |s| (s.w, s.h));
            PageMeta {
                width,
                height,
                force_single: false,
            }
        })
        .collect()
}
