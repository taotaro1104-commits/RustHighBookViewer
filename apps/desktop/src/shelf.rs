//! 本棚画面: サムネイル一覧・履歴・お気に入り・検索。

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions, Vec2};
use pv_library::{Book, BookFilter, BookId, BookSort, ReadStatus};

use crate::data::Data;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    All,
    Recent,
    Favorites,
}

pub enum ShelfAction {
    Open(std::path::PathBuf),
    OpenDialogFile,
    OpenDialogFolder,
    OpenTextSettings,
}

pub struct Shelf {
    tab: Tab,
    sort: BookSort,
    query: String,
    books: Vec<Book>,
    dirty: bool,
    textures: HashMap<BookId, TextureHandle>,
    requested: HashSet<BookId>,
}

impl Shelf {
    pub fn new() -> Self {
        Self {
            tab: Tab::Recent,
            sort: BookSort::LastOpened,
            query: String::new(),
            books: Vec::new(),
            dirty: true,
            textures: HashMap::new(),
            requested: HashSet::new(),
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn refresh(&mut self, data: &Data) {
        let filter = BookFilter {
            favorite_only: self.tab == Tab::Favorites,
            query: (!self.query.is_empty()).then(|| self.query.clone()),
            ..Default::default()
        };
        let res = match self.tab {
            Tab::Recent => data.library.recent(200).map(|v| {
                let q = self.query.to_lowercase();
                v.into_iter()
                    .filter(|b| q.is_empty() || b.title.to_lowercase().contains(&q))
                    .collect()
            }),
            _ => data.library.list_books(&filter, self.sort),
        };
        match res {
            Ok(b) => self.books = b,
            Err(e) => eprintln!("library: {e}"),
        }
        self.dirty = false;
    }

    fn pump_thumbnails(&mut self, ctx: &egui::Context, data: &Data) {
        while let Ok((id, bmp)) = data.thumb_results.try_recv() {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [bmp.size.w as usize, bmp.size.h as usize],
                &bmp.data,
            );
            let tex = ctx.load_texture(format!("thumb{id}"), img, TextureOptions::LINEAR);
            self.textures.insert(id, tex);
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, data: &Data) -> Option<ShelfAction> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        self.pump_thumbnails(&ctx, data);

        egui::Panel::top("shelf_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("ファイルを開く").clicked() {
                    action = Some(ShelfAction::OpenDialogFile);
                }
                if ui.button("フォルダを開く").clicked() {
                    action = Some(ShelfAction::OpenDialogFolder);
                }
                if ui.button("文字設定").clicked() {
                    action = Some(ShelfAction::OpenTextSettings);
                }
                ui.separator();
                for (tab, label) in [
                    (Tab::Recent, "履歴"),
                    (Tab::All, "すべて"),
                    (Tab::Favorites, "お気に入り"),
                ] {
                    if ui.selectable_label(self.tab == tab, label).clicked() {
                        self.tab = tab;
                        self.dirty = true;
                    }
                }
                ui.separator();
                if ui
                    .add(egui::TextEdit::singleline(&mut self.query).hint_text("タイトル検索"))
                    .changed()
                {
                    self.dirty = true;
                }
                if self.tab == Tab::All || self.tab == Tab::Favorites {
                    let before = self.sort;
                    egui::ComboBox::from_id_salt("sort")
                        .selected_text(match self.sort {
                            BookSort::Name => "名前",
                            BookSort::LastOpened => "最終閲覧",
                            BookSort::Added => "追加日",
                            BookSort::Size => "サイズ",
                        })
                        .show_ui(ui, |ui| {
                            for (s, l) in [
                                (BookSort::Name, "名前"),
                                (BookSort::LastOpened, "最終閲覧"),
                                (BookSort::Added, "追加日"),
                                (BookSort::Size, "サイズ"),
                            ] {
                                ui.selectable_value(&mut self.sort, s, l);
                            }
                        });
                    if before != self.sort {
                        self.dirty = true;
                    }
                }
            });
        });

        if self.dirty {
            self.refresh(data);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            if self.books.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("本がありません。ファイルかフォルダを開く、またはここへドラッグ＆ドロップしてください。");
                });
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::splat(12.0);
                    let books = self.books.clone();
                    for book in &books {
                        if let Some(a) = self.card(ui, data, book) {
                            action = Some(a);
                        }
                    }
                });
            });
        });
        action
    }

    fn card(&mut self, ui: &mut egui::Ui, data: &Data, book: &Book) -> Option<ShelfAction> {
        let mut action = None;
        if !self.textures.contains_key(&book.id) && self.requested.insert(book.id) {
            data.request_thumbnail(book);
        }
        let size = Vec2::new(140.0, 230.0);
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        let painter = ui.painter_at(rect);
        let bg = if resp.hovered() {
            Color32::from_gray(60)
        } else {
            Color32::from_gray(40)
        };
        painter.rect_filled(rect, 6.0, bg);

        let img_rect =
            egui::Rect::from_min_size(rect.min + Vec2::splat(6.0), Vec2::new(128.0, 180.0));
        match self.textures.get(&book.id) {
            Some(tex) => {
                let t = tex.size_vec2();
                let k = (img_rect.width() / t.x).min(img_rect.height() / t.y);
                let r = egui::Rect::from_center_size(img_rect.center(), t * k);
                painter.image(
                    tex.id(),
                    r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                painter.rect_filled(img_rect, 4.0, Color32::from_gray(25));
            }
        }
        if book.favorite {
            painter.text(
                rect.min + Vec2::new(10.0, 10.0),
                egui::Align2::LEFT_TOP,
                "★",
                egui::FontId::proportional(18.0),
                Color32::GOLD,
            );
        }
        let status = match book.status {
            ReadStatus::Unread => "",
            ReadStatus::Reading => "読書中",
            ReadStatus::Finished => "読了",
        };
        painter.text(
            rect.min + Vec2::new(8.0, 190.0),
            egui::Align2::LEFT_TOP,
            truncate(&book.title, 14),
            egui::FontId::proportional(13.0),
            Color32::WHITE,
        );
        painter.text(
            rect.min + Vec2::new(8.0, 210.0),
            egui::Align2::LEFT_TOP,
            format!("{}p {status}", book.page_count),
            egui::FontId::proportional(11.0),
            Color32::from_gray(170),
        );

        if resp.clicked() {
            action = Some(ShelfAction::Open(book.uri.clone().into()));
        }
        resp.context_menu(|ui| {
            let label = if book.favorite {
                "お気に入りを外す"
            } else {
                "お気に入りにする"
            };
            if ui.button(label).clicked() {
                let _ = data.library.set_favorite(book.id, !book.favorite);
                self.dirty = true;
                ui.close();
            }
            if ui.button("既読にする").clicked() {
                let _ = data.library.set_status(book.id, ReadStatus::Finished);
                self.dirty = true;
                ui.close();
            }
            if ui
                .button(RichText::new("ライブラリから削除").color(Color32::LIGHT_RED))
                .clicked()
            {
                let _ = data.library.delete_book(book.id);
                self.dirty = true;
                ui.close();
            }
        });
        action
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}
