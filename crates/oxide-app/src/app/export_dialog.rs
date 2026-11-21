//! File → Export…: pick a format and its options, then where to save.

use eframe::egui;
use oxide_engine::Color;

use super::{OxideApp, to_color32};
use crate::export::{ExportFormat, export};
use crate::settings::DialogFolder;

#[derive(Default)]
pub struct ExportDialog {
    pub open: bool,
}

impl OxideApp {
    pub(super) fn export_dialog(&mut self, ctx: &egui::Context) {
        if !self.export_dialog.open {
            return;
        }
        let mut options = self.app_settings.export;
        let mut go = false;
        let mut cancel = false;

        let modal = egui::Modal::new(egui::Id::new("export")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.heading("Export");
            ui.add_space(6.0);

            egui::Grid::new("export-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Format");
                egui::ComboBox::from_id_salt("export-format").selected_text(options.format.label()).width(260.0).show_ui(ui, |ui| {
                    for format in ExportFormat::ALL {
                        ui.selectable_value(&mut options.format, format, format.label());
                    }
                });
                ui.end_row();

                ui.label("");
                ui.weak(options.format.description());
                ui.end_row();

                if options.format.supports_transparency() {
                    ui.label("Paper");
                    ui.checkbox(&mut options.include_paper, "Include paper (off = transparent background)");
                    ui.end_row();
                }

                if options.format == ExportFormat::Jpeg {
                    ui.label("Quality");
                    ui.add(egui::Slider::new(&mut options.jpeg_quality, 1..=100));
                    ui.end_row();

                    ui.label("Background");
                    ui.horizontal(|ui| {
                        let mut custom = options.jpeg_background.is_some();
                        ui.radio_value(&mut custom, false, "Paper");
                        ui.radio_value(&mut custom, true, "Color");
                        if custom {
                            let [r, g, b] = options.jpeg_background.unwrap_or([1.0, 1.0, 1.0]);
                            let mut c = to_color32(Color::rgb(r, g, b));
                            ui.color_edit_button_srgba(&mut c);
                            let [r, g, b, _] = c.to_array();
                            options.jpeg_background = Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]);
                        } else {
                            options.jpeg_background = None;
                        }
                    })
                    .response
                    .on_hover_text("What fills areas with no paint (JPEG has no transparency)");
                    ui.end_row();
                }

                if options.format.supports_16_bit() {
                    ui.label("Bit depth");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut options.sixteen_bit, false, "8-bit");
                        ui.radio_value(&mut options.sixteen_bit, true, "16-bit");
                    })
                    .response
                    .on_hover_text("16-bit keeps smoother gradients (bigger files)");
                    ui.end_row();
                }

                ui.label("Size");
                let doc = &self.document;
                let inches = |px: usize| px as f32 / doc.dpi();
                ui.weak(format!("{} × {} px at {:.0} DPI ({:.2} × {:.2} in)", doc.width(), doc.height(), doc.dpi(), inches(doc.width()), inches(doc.height())));
                ui.end_row();
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("  Export…  ").fill(ui.visuals().selection.bg_fill)).clicked() {
                        go = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });

        if options != self.app_settings.export {
            self.app_settings.export = options;
            self.save_app_settings();
        }
        if modal.should_close() {
            cancel = true;
        }
        if go {
            self.export_dialog.open = false;
            self.export_to_file();
        } else if cancel {
            self.export_dialog.open = false;
        }
    }

    /// Ask where to save, then write the file with the current export options.
    fn export_to_file(&mut self) {
        let options = self.app_settings.export;
        let ext = options.format.extension();
        let Some(path) =
            self.dialog(DialogFolder::Exports).add_filter(options.format.label(), &[ext]).set_file_name(format!("{}.{ext}", self.file_stem())).save_file()
        else {
            return;
        };
        let path = path.with_extension(ext);
        self.remember_folder(DialogFolder::Exports, &path);
        match export(&self.document, &path, &options) {
            Ok(()) => self.info(format!("Exported {}", path.display())),
            Err(e) => self.error(format!("Couldn't export {}: {e}", path.display())),
        }
    }
}
