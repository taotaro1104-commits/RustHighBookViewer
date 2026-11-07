//! 文字設定ウィンドウ: 文章の EPUB の書体を選び、フォントを取り込む・削除する。
//! 変えた設定は、次に本を開いたときから反映される。

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use eframe::egui;
use pv_container::{FontInfo, Typeface, MAX_FONT_BYTES};

use crate::data::Data;

/// ファイルを読んでフォントとして取り込む。大きすぎるファイルは、読む前に断る。
pub fn import_from_path(path: &Path) -> Result<FontInfo, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FONT_BYTES {
        return Err(format!(
            "ファイルが大きすぎます（{} MB まで）",
            MAX_FONT_BYTES >> 20
        ));
    }
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    pv_container::import_font(&name, &data).map_err(|e| e.to_string())
}

type ImportResult = Result<FontInfo, String>;

#[derive(Default)]
pub struct TextSettingsWindow {
    pub open: bool,
    error: Option<String>,
    /// 一覧の読み直しが要るか（開いたとき・取り込み／削除のあと）。毎フレームのディレクトリ走査を避ける
    stale: bool,
    fonts: Vec<FontInfo>,
    has_mincho: bool,
    /// 取り込み中の結果の受け取り口（読み込みと検査は別スレッドで行う）
    importing: Option<Receiver<ImportResult>>,
}

impl TextSettingsWindow {
    fn reload(&mut self) {
        self.fonts = pv_container::list_fonts();
        self.has_mincho = pv_container::has_typeface(&Typeface::Mincho);
        self.stale = false;
    }

    /// 別スレッドで取り込みを始める（大きなファイルでも画面が固まらない）。
    fn start_import(&mut self, ctx: &egui::Context, path: PathBuf) {
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(import_from_path(&path));
            ctx.request_repaint();
        });
        self.importing = Some(rx);
    }

    /// 取り込みが終わっていれば結果を反映する。設定を変えたら true。
    fn poll_import(&mut self, data: &mut Data) -> bool {
        let Some(rx) = &self.importing else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("取り込みに失敗しました".to_string()),
        };
        self.importing = None;
        self.stale = true;
        match result {
            Ok(info) => {
                self.error = None;
                data.settings.text.typeface = Typeface::Custom(info.id);
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, data: &mut Data) {
        if !self.open {
            self.stale = true; // 次に開いたときに読み直す
            return;
        }
        let mut changed = self.poll_import(data);
        if self.stale {
            self.reload();
        }
        let mut open = self.open;
        egui::Window::new("文字設定")
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("文章の EPUB の書体です。変更は、次に本を開いたときから反映されます。");
                ui.separator();
                let current = data.settings.text.typeface.clone();
                let mut choice = current.clone();
                ui.radio_value(&mut choice, Typeface::Gothic, "ゴシック体");
                if self.has_mincho {
                    ui.radio_value(&mut choice, Typeface::Mincho, "明朝体");
                }
                for f in &self.fonts {
                    ui.radio_value(&mut choice, Typeface::Custom(f.id.clone()), &f.name);
                }
                if choice != current {
                    data.settings.text.typeface = choice;
                    changed = true;
                }

                ui.separator();
                let busy = self.importing.is_some();
                if ui
                    .add_enabled(!busy, egui::Button::new("フォントを取り込む…"))
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("フォント", &["ttf", "otf", "ttc"])
                        .pick_file()
                    {
                        self.start_import(ctx, p);
                    }
                }
                if busy {
                    ui.label("取り込み中…");
                }
                ui.label(
                    "ttf / otf / ttc（64 MB まで）。日本語の字形が無い文字はゴシック体で表示されます。",
                );
                if let Some(e) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                }

                if !self.fonts.is_empty() {
                    ui.separator();
                    ui.label("取り込んだフォント");
                }
                let mut remove = None;
                for f in &self.fonts {
                    ui.horizontal(|ui| {
                        ui.label(&f.name);
                        if ui.button("削除").clicked() {
                            remove = Some(f.id.clone());
                        }
                    });
                }
                if let Some(id) = remove {
                    match pv_container::remove_font(&id) {
                        Ok(()) => {
                            self.stale = true;
                            if data.settings.text.typeface == Typeface::Custom(id) {
                                data.settings.text.typeface = Typeface::Gothic;
                                changed = true;
                            }
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            });
        if changed {
            data.save_settings();
        }
        self.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取り込み元の検査: フォントでないファイルと、上限を超えるファイルは断る（中身は読まない）
    #[test]
    fn import_from_path_rejects_large_and_non_font() {
        let dir = tempfile::tempdir().unwrap();
        pv_container::set_font_dir(dir.path().join("fonts"));

        let small = dir.path().join("a.ttf");
        std::fs::write(&small, b"x").unwrap();
        assert!(import_from_path(&small).is_err());

        let big = dir.path().join("big.ttf");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(pv_container::MAX_FONT_BYTES + 1)
            .unwrap();
        let err = import_from_path(&big).unwrap_err();
        assert!(err.contains("64"), "{err}");

        assert!(import_from_path(&dir.path().join("missing.ttf")).is_err());
        assert!(pv_container::list_fonts().is_empty());
    }
}
