mod data;
mod reader;
mod shelf;

use std::path::{Path, PathBuf};

use eframe::egui;

use data::Data;
use reader::{Reader, ReaderExit};
use shelf::{Shelf, ShelfAction};

struct App {
    data: Data,
    shelf: Shelf,
    reader: Option<Reader>,
    error: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, data: Data, initial: Option<PathBuf>) -> Self {
        install_cjk_font(&cc.egui_ctx);
        let mut app = Self {
            data,
            shelf: Shelf::new(),
            reader: None,
            error: None,
        };
        if let Some(p) = initial {
            app.open(&p);
        }
        app
    }

    fn open(&mut self, path: &Path) {
        match self.data.open_book(path) {
            Ok(o) => {
                self.error = None;
                self.reader = Some(Reader::new(o.session, o.book));
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    fn close_reader(&mut self) {
        if let Some(r) = self.reader.take() {
            r.finish(&self.data);
            self.shelf.mark_dirty();
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // ドラッグ＆ドロップで開く
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(p) = dropped {
            self.close_reader();
            self.open(&p);
        }

        if let Some(msg) = self.error.clone() {
            egui::Panel::bottom("error").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::LIGHT_RED, msg);
                    if ui.button("閉じる").clicked() {
                        self.error = None;
                    }
                });
            });
        }

        if let Some(reader) = &mut self.reader {
            if let Some(ReaderExit::ToShelf) = reader.ui(ui, &self.data) {
                self.close_reader();
            }
            return;
        }

        match self.shelf.ui(ui, &self.data) {
            Some(ShelfAction::Open(p)) => self.open(&p),
            Some(ShelfAction::OpenDialogFile) => {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("コミック書庫", &["zip", "cbz"])
                    .pick_file()
                {
                    self.open(&p);
                }
            }
            Some(ShelfAction::OpenDialogFolder) => {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    self.open(&p);
                }
            }
            None => {}
        }
    }

    fn on_exit(&mut self) {
        self.close_reader();
        self.data.save_settings();
    }
}

/// egui 既定フォントには日本語がないため、OS の日本語フォントを探して追加する。
fn install_cjk_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "C:/Windows/Fonts/YuGothM.ttc",
        "C:/Windows/Fonts/meiryo.ttc",
        "C:/Windows/Fonts/msgothic.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ];
    let Some(bytes) = CANDIDATES.iter().find_map(|p| std::fs::read(p).ok()) else {
        eprintln!("no Japanese font found; labels may not render");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".into(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("cjk".into());
    }
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result {
    let data = match Data::open() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot start: {e}");
            std::process::exit(1);
        }
    };
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 800.0])
            .with_title("PerfectViewer"),
        ..Default::default()
    };
    eframe::run_native(
        "PerfectViewer",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, data, initial)))),
    )
}
