//! 閲覧画面: Rust コアからフレーム（ビットマップ）を受け取って描画するだけの薄い UI 層。

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Key, TextureHandle, TextureOptions};
use pv_core::{
    tap_zone, Action, BookOverride, BookSession, CoreEvent, Direction, FitMode, Frame, FrameTicket,
    InputKey, InputMap, Size, ViewMode,
};
use pv_library::{now_ms, Book};
use pv_render::{clamp_pan, clamp_zoom, zoom_at, Vec2 as RVec2};

use crate::data::Data;

pub enum ReaderExit {
    ToShelf,
}

pub struct Reader {
    session: BookSession,
    book: Book,
    input: InputMap,
    texture: Option<(TextureHandle, std::sync::Arc<Frame>)>,
    pending: Option<(FrameTicket, Size, f32)>,
    shown_spread: Option<usize>,
    zoom: f32,
    pan: RVec2,
    seekbar: bool,
    saved_page: Option<usize>,
    toast: Option<(String, Instant)>,
    scroll_cooldown: Instant,
}

fn r2e(v: RVec2) -> egui::Vec2 {
    egui::vec2(v.x, v.y)
}

fn e2r(v: egui::Vec2) -> RVec2 {
    RVec2::new(v.x, v.y)
}

impl Reader {
    pub fn new(session: BookSession, book: Book) -> Self {
        let dir = session.settings().layout.direction;
        let input = rebuild_input(&session.settings().input, dir);
        let mut r = Self {
            session,
            book,
            input,
            texture: None,
            pending: None,
            shown_spread: None,
            zoom: 1.0,
            pan: RVec2::ZERO,
            seekbar: false,
            saved_page: None,
            toast: None,
            scroll_cooldown: Instant::now(),
        };
        r.reset_view();
        r
    }

    fn reset_view(&mut self) {
        self.zoom = 1.0;
        self.pan = RVec2::ZERO;
        self.shown_spread = None; // 次のフレーム到着時に先頭へ寄せ直す
    }

    fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn current_override(&self) -> BookOverride {
        let s = self.session.settings();
        BookOverride {
            mode: Some(s.layout.mode),
            direction: Some(s.layout.direction),
            first_page_single: Some(s.layout.first_page_single),
            shift: Some(s.layout.shift),
            fit: Some(s.fit),
            color: Some(self.session.settings().color),
        }
    }

    /// 読書位置を保存する（ページが変わったときだけ）。
    pub fn autosave(&mut self, data: &Data, force: bool) {
        let page = self.session.current_page();
        if force || self.saved_page != Some(page) {
            if let Err(e) =
                data.library
                    .save_progress(self.book.id, self.session.cursor(), page, now_ms())
            {
                eprintln!("progress not saved: {e}");
            }
            self.saved_page = Some(page);
        }
    }

    fn settings_changed(&mut self, data: &Data) {
        data.save_override(self.book.id, &self.current_override());
        let dir = self.session.settings().layout.direction;
        self.input = rebuild_input(&self.session.settings().input, dir);
        self.reset_view();
    }

    fn handle(&mut self, action: Action, data: &Data) {
        if self.session.apply(&action) {
            if matches!(
                action,
                Action::ShiftSpread
                    | Action::FitWidth
                    | Action::FitHeight
                    | Action::FitPage
                    | Action::FitFill
            ) {
                self.settings_changed(data);
            } else {
                self.zoom = 1.0;
                self.pan = RVec2::ZERO;
                self.shown_spread = None;
            }
            return;
        }
        match action {
            Action::ZoomIn => self.zoom = clamp_zoom(self.zoom * 1.25),
            Action::ZoomOut => self.zoom = clamp_zoom(self.zoom / 1.25),
            Action::OpenThumbnails => self.seekbar = !self.seekbar,
            Action::AddBookmark => {
                let page = self.session.current_page();
                match data.library.add_bookmark(self.book.id, page, "", now_ms()) {
                    Ok(()) => self.toast(format!("しおりを追加: {}ページ", page + 1)),
                    Err(e) => self.toast(format!("しおりを追加できません: {e}")),
                }
            }
            Action::SavePage => self.save_page(),
            other => self.toast(format!("未対応の操作: {other:?}")),
        }
    }

    fn save_page(&mut self) {
        let page = self.session.current_page();
        let name = self
            .session
            .page_path(page)
            .and_then(|p| p.rsplit('/').next().map(str::to_owned))
            .unwrap_or_else(|| format!("page{}.png", page + 1));
        let Some(dest) = rfd::FileDialog::new().set_file_name(name).save_file() else {
            return;
        };
        // 処理前の元バイト列をそのまま書き出すので劣化しない
        let res = self
            .session
            .page_bytes(page)
            .map_err(|e| e.to_string())
            .and_then(|b| std::fs::write(&dest, &b).map_err(|e| e.to_string()));
        match res {
            Ok(()) => self.toast("ページを保存しました"),
            Err(e) => self.toast(format!("保存に失敗: {e}")),
        }
    }

    fn pump_events(&mut self, ctx: &egui::Context) {
        let rx = self.session.events();
        while let Ok(ev) = rx.try_recv() {
            match ev {
                CoreEvent::FrameReady { ticket, frame } => {
                    if ticket.spread != self.session.cursor() {
                        continue;
                    }
                    let img = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.bitmap.size.w as usize, frame.bitmap.size.h as usize],
                        &frame.bitmap.data,
                    );
                    let tex = ctx.load_texture("page", img, TextureOptions::LINEAR);
                    if self.pending.map(|p| p.0) == Some(ticket) {
                        self.pending = None;
                    }
                    self.texture = Some((tex, frame));
                }
                CoreEvent::Cancelled { ticket } => {
                    if self.pending.map(|p| p.0) == Some(ticket) {
                        self.pending = None;
                    }
                }
                CoreEvent::Error { ticket, message } => {
                    if self.pending.map(|p| p.0) == Some(ticket) {
                        self.pending = None;
                    }
                    self.toast(format!("表示エラー: {message}"));
                }
            }
        }
    }

    /// 必要なら（ページ移動・リサイズ・ズーム変更）コアへフレームを要求する。
    fn request_if_needed(&mut self, vp_px: Size) {
        let scale = (self.zoom * 2.0).ceil() / 2.0; // 0.5 刻みに丸めて再要求の頻度を抑える
        let spread = self.session.cursor();
        let satisfied = self
            .texture
            .as_ref()
            .is_some_and(|(_, f)| f.spread == spread && f.viewport == vp_px && f.scale >= scale);
        let in_flight = self
            .pending
            .is_some_and(|(t, v, s)| t.spread == spread && v == vp_px && s >= scale);
        if !satisfied && !in_flight && vp_px.w > 0 && vp_px.h > 0 {
            let t = self.session.request_frame(spread, vp_px, scale);
            self.pending = Some((t, vp_px, scale));
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, data: &Data) -> Option<ReaderExit> {
        let ctx = ui.ctx().clone();
        let mut exit = None;
        self.pump_events(&ctx);
        self.top_bar(ui, data, &mut exit);
        if self.seekbar {
            self.seek_bar(ui);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::BLACK))
            .show(ui, |ui| {
                let rect = ui.available_rect_before_wrap();
                let ppp = ctx.pixels_per_point();
                let vp_px = Size::new((rect.width() * ppp) as u32, (rect.height() * ppp) as u32);
                self.request_if_needed(vp_px);
                let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                self.draw_page(ui, rect, ppp);
                self.pointer_input(&ctx, &resp, rect, data);
            });

        self.keyboard_input(&ctx, data, &mut exit);
        self.autosave(data, false);
        self.draw_toast(&ctx);
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(8));
        }
        exit
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, data: &Data, exit: &mut Option<ReaderExit>) {
        egui::Panel::top("reader_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← 本棚").clicked() {
                    *exit = Some(ReaderExit::ToShelf);
                }
                ui.label(&self.book.title);
                ui.label(format!(
                    "{} / {}",
                    self.session.current_page() + 1,
                    self.session.page_count()
                ));
                ui.separator();
                let mut layout = self.session.settings().layout;
                let before = layout;
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(match layout.mode {
                        ViewMode::Single => "単ページ",
                        ViewMode::Double => "見開き",
                        ViewMode::Auto => "自動見開き",
                        ViewMode::Webtoon => "縦スクロール",
                    })
                    .show_ui(ui, |ui| {
                        for (m, l) in [
                            (ViewMode::Single, "単ページ"),
                            (ViewMode::Double, "見開き"),
                            (ViewMode::Auto, "自動見開き"),
                            (ViewMode::Webtoon, "縦スクロール"),
                        ] {
                            ui.selectable_value(&mut layout.mode, m, l);
                        }
                    });
                let rtl = layout.direction == Direction::RightToLeft;
                if ui.button(if rtl { "右綴じ" } else { "左綴じ" }).clicked() {
                    layout.direction = if rtl {
                        Direction::LeftToRight
                    } else {
                        Direction::RightToLeft
                    };
                }
                if layout != before {
                    if layout.mode == ViewMode::Webtoon && before.mode != ViewMode::Webtoon {
                        self.session.set_fit(FitMode::Width);
                    }
                    self.session.set_layout(layout);
                    self.settings_changed(data);
                }
                ui.separator();
                for (mode, label, act) in [
                    (FitMode::Width, "幅", Action::FitWidth),
                    (FitMode::Height, "高さ", Action::FitHeight),
                    (FitMode::Page, "全体", Action::FitPage),
                    (FitMode::Fill, "全画面", Action::FitFill),
                ] {
                    if ui
                        .selectable_label(self.session.settings().fit == mode, label)
                        .clicked()
                    {
                        self.handle(act, data);
                    }
                }
                ui.separator();
                if ui.button("★ しおり").clicked() {
                    self.handle(Action::AddBookmark, data);
                }
                if ui.button("シークバー").clicked() {
                    self.seekbar = !self.seekbar;
                }
            });
        });
    }

    fn seek_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("seek").show(ui, |ui| {
            ui.horizontal(|ui| {
                let last = self.session.page_count().saturating_sub(1);
                let mut page = self.session.current_page();
                // 右綴じではスライダーの向きも読む方向に合わせる
                let rtl = self.session.settings().layout.direction == Direction::RightToLeft;
                ui.spacing_mut().slider_width = (ui.available_width() - 120.0).max(100.0);
                let mut slider = egui::Slider::new(&mut page, 0..=last).show_value(false);
                if rtl {
                    slider = slider.trailing_fill(false);
                }
                let r = ui.add(slider);
                ui.label(format!("{} / {}", page + 1, last + 1));
                if r.drag_stopped() || (r.changed() && !r.dragged()) {
                    self.session.go_to_page(page);
                    self.zoom = 1.0;
                    self.pan = RVec2::ZERO;
                    self.shown_spread = None;
                }
            });
        });
    }

    fn draw_page(&mut self, ui: &mut egui::Ui, rect: egui::Rect, ppp: f32) {
        let Some((tex, frame)) = &self.texture else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "読み込み中…",
                egui::FontId::proportional(16.0),
                Color32::GRAY,
            );
            return;
        };
        // フレームは「ビューポート×scale」に収めて作られているので、scale 1 相当の大きさに戻して zoom を掛ける
        let base = egui::vec2(frame.bitmap.size.w as f32, frame.bitmap.size.h as f32)
            / (frame.scale * ppp);
        let shown = base * self.zoom;
        let vp = rect.size();

        // ページが切り替わった直後は、幅フィットで縦にはみ出す場合に上端へ寄せる
        if self.shown_spread != Some(frame.spread) {
            self.shown_spread = Some(frame.spread);
            let overflow_y = ((shown.y - vp.y) / 2.0).max(0.0);
            self.pan = RVec2::new(0.0, overflow_y);
        }
        self.pan = clamp_pan(self.pan, e2r(shown), e2r(vp));
        let center = rect.center() + r2e(self.pan);
        let img_rect = egui::Rect::from_center_size(center, shown);
        ui.painter_at(rect).image(
            tex.id(),
            img_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    fn shown_size(&self, ppp: f32) -> RVec2 {
        self.texture.as_ref().map_or(RVec2::ZERO, |(_, f)| {
            let s = egui::vec2(f.bitmap.size.w as f32, f.bitmap.size.h as f32) / (f.scale * ppp)
                * self.zoom;
            e2r(s)
        })
    }

    fn pointer_input(
        &mut self,
        ctx: &egui::Context,
        resp: &egui::Response,
        rect: egui::Rect,
        data: &Data,
    ) {
        let ppp = ctx.pixels_per_point();
        let vp = e2r(rect.size());

        if resp.dragged() && self.zoom > 1.0 + f32::EPSILON
            || (resp.dragged() && self.shown_size(ppp).y > vp.y)
        {
            let d = e2r(resp.drag_delta());
            self.pan = RVec2::new(self.pan.x + d.x, self.pan.y + d.y);
        }
        if resp.double_clicked() {
            let key = InputKey::DoubleTap;
            if self.input.lookup(&key) == Some(&Action::ZoomIn) {
                self.zoom = if self.zoom > 1.0 { 1.0 } else { 2.5 };
                self.pan = RVec2::ZERO;
            } else if let Some(a) = self.input.lookup(&key).cloned() {
                self.handle(a, data);
            }
        } else if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let z = tap_zone(
                    pos.x - rect.min.x,
                    pos.y - rect.min.y,
                    rect.width(),
                    rect.height(),
                );
                if let Some(a) = self.input.lookup(&InputKey::TapZone(z)).cloned() {
                    self.handle(a, data);
                }
            }
        }

        if resp.hovered() {
            let (scroll, ctrl, hover) = ctx.input(|i| {
                (
                    i.smooth_scroll_delta.y,
                    i.modifiers.command,
                    i.pointer.hover_pos(),
                )
            });
            if scroll != 0.0 {
                if ctrl {
                    let anchor = hover.map_or(RVec2::ZERO, |p| e2r(p - rect.center()));
                    let (z, p) = zoom_at(self.zoom, self.pan, anchor, (scroll / 200.0).exp());
                    self.zoom = z;
                    self.pan = p;
                } else {
                    self.wheel_scroll(scroll, ppp, vp, data);
                }
            }
        }
    }

    /// ホイール: はみ出している間はパン、端まで来たら（少し間を置いて）次/前のページへ。
    fn wheel_scroll(&mut self, scroll: f32, ppp: f32, vp: RVec2, data: &Data) {
        let shown = self.shown_size(ppp);
        let limit = ((shown.y - vp.y) / 2.0).max(0.0);
        let next = self.pan.y + scroll;
        let at_edge = limit == 0.0
            || (scroll < 0.0 && self.pan.y <= -limit)
            || (scroll > 0.0 && self.pan.y >= limit);
        if !at_edge {
            self.pan = RVec2::new(self.pan.x, next.clamp(-limit, limit));
        } else if self.scroll_cooldown.elapsed() > Duration::from_millis(250) {
            self.scroll_cooldown = Instant::now();
            let a = if scroll < 0.0 {
                Action::NextSpread
            } else {
                Action::PrevSpread
            };
            self.handle(a, data);
            if scroll > 0.0 && limit > 0.0 {
                // 前のページへ戻るときは下端から読み始められるようにする
                self.pan.y = -limit;
            }
        }
    }

    fn keyboard_input(&mut self, ctx: &egui::Context, data: &Data, exit: &mut Option<ReaderExit>) {
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key {
                key,
                pressed: true,
                repeat: _,
                modifiers,
                ..
            } = ev
            else {
                continue;
            };
            if modifiers.command {
                continue;
            }
            match key {
                Key::Escape => *exit = Some(ReaderExit::ToShelf),
                Key::F11 => {
                    let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
                }
                k => {
                    let name = format!("{k:?}");
                    if let Some(a) = self.input.lookup(&InputKey::Key(name)).cloned() {
                        self.handle(a, data);
                    }
                }
            }
        }
    }

    fn draw_toast(&mut self, ctx: &egui::Context) {
        if let Some((msg, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(3) {
                self.toast = None;
                return;
            }
            #[allow(deprecated)]
            let area = egui::Area::new(egui::Id::new("toast"));
            area.anchor(egui::Align2::CENTER_BOTTOM, [0.0, -40.0])
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(msg));
                });
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    /// 一覧に戻る・終了するときの保存。
    pub fn finish(mut self, data: &Data) {
        self.autosave(data, true);
    }
}

/// 標準プロファイルは綴じ方向に追従して作り直す。ユーザーが編集済みの割り当てはそのまま使う。
fn rebuild_input(current: &InputMap, dir: Direction) -> InputMap {
    if current.profile == "標準" {
        InputMap::standard(dir)
    } else {
        current.clone()
    }
}
