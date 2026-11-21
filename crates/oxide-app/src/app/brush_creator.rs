//! The Brush Creator window: edit the current tool's brush *shape* with a
//! live preview (painted with the current medium), and save the shape as a preset.

use eframe::egui::{self, Vec2};

use super::OxideApp;
use crate::settings::DialogFolder;
use crate::tools::BrushShape;
use oxide_engine::Brush;

use crate::presets::render_preview;

const PREVIEW_W: usize = 300;
const PREVIEW_H: usize = 90;

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Stroke,
    Shape,
}

#[derive(Default)]
pub struct BrushCreator {
    pub open: bool,
    tab: Tab,
    /// Name typed into the "Save as preset" box.
    name: String,
    /// The group to save it in (empty = My Brushes).
    group: String,
    /// Result of the last save, shown under the button.
    message: Option<Result<String, String>>,
    preview: Option<egui::TextureHandle>,
    /// The brush the preview was rendered for, so we only re-render when
    /// something changed.
    preview_for: Option<Brush>,
}

impl OxideApp {
    pub(super) fn brush_creator_window(&mut self, ctx: &egui::Context) {
        let mut open = self.brush_creator.open;
        egui::Window::new("Brush Creator").open(&mut open).default_width(320.0).resizable(false).show(ctx, |ui| self.brush_creator_contents(ui));
        self.brush_creator.open = open;
    }

    /// File → Import Brushes… and Brush Creator → Import brushes…: pick brush
    /// files (or, with `images`, pictures to use as tips), add their tips to
    /// the library, and switch to the first one. Brush files only by default,
    /// so a Downloads folder full of pictures doesn't bury them.
    pub(super) fn import_tips(&mut self, ctx: &egui::Context, images: bool) {
        let dialog = self.dialog(DialogFolder::Tips);
        let dialog = if images {
            dialog.set_title("Choose pictures to use as brush tips").add_filter("Pictures", &["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"])
        } else {
            dialog.set_title("Choose brush files to import").add_filter("Brush files", &["abr", "brush", "brushset", "gbr", "gih"])
        };
        let Some(paths) = dialog.pick_files() else {
            return;
        };
        if let Some(first) = paths.first() {
            self.remember_folder(DialogFolder::Tips, first);
        }
        self.import_tip_files(&paths, ctx);
    }

    /// Add the tips in these files to the library and use the first one.
    pub(super) fn import_tip_files(&mut self, paths: &[std::path::PathBuf], ctx: &egui::Context) {
        let (mut added, mut errors) = (Vec::new(), Vec::new());
        for path in paths {
            match oxide_commands::tip_import::import_tips(path) {
                Ok(tips) => {
                    // Each tip also becomes a brush preset, grouped by the file it came from.
                    let source = path.file_stem().map_or("Imported".into(), |s| s.to_string_lossy().into_owned());
                    let group = format!("Imported: {source}");
                    for &(tip, settings) in &tips {
                        // Texture-tip defaults, then whatever the file said (spacing, jitter...).
                        let mut shape = BrushShape { radius: 24.0, spacing: 0.25, jitter_angle: 1.0, tip, ..BrushShape::default() };
                        settings.apply(&mut shape);
                        if let Err(e) = self.presets.save_in(ctx, tip.name(), &group, &shape) {
                            errors.push(e);
                        }
                    }
                    added.extend(tips);
                }
                Err(e) => errors.push(e),
            }
        }
        if let Some(&(first, settings)) = added.first() {
            let shape = &mut self.state_mut().shape;
            shape.tip = first;
            settings.apply(shape);
        }
        match (added.len(), errors.first()) {
            (0, Some(e)) => self.error(e.clone()),
            (n, None) => self.info(format!(
                "Imported {n} brush{} as presets (now using \"{}\"). Edit them in the Brush Creator.",
                if n == 1 { "" } else { "es" },
                added[0].0.name()
            )),
            (n, Some(e)) => self.error(format!("Imported {n} tips, but: {e}")),
        }
    }

    fn brush_creator_contents(&mut self, ui: &mut egui::Ui) {
        // --- Live preview, painted with the current tool's medium ---
        let current = self.stroke_brush_preview();
        let creator = &mut self.brush_creator;
        if creator.preview_for.as_ref() != Some(&current) {
            let image = render_preview(&current, PREVIEW_W, PREVIEW_H);
            match &mut creator.preview {
                Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                None => creator.preview = Some(ui.ctx().load_texture("brush-preview", image, egui::TextureOptions::LINEAR)),
            }
            creator.preview_for = Some(current);
        }
        if let Some(texture) = &creator.preview {
            ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::new(PREVIEW_W as f32, PREVIEW_H as f32)));
        }
        ui.weak(format!("Editing the {} brush", self.tool.label()));

        // --- Tabs ---
        let creator = &mut self.brush_creator;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut creator.tab, Tab::Stroke, "Stroke");
            ui.selectable_value(&mut creator.tab, Tab::Shape, "Shape & Grain");
        });
        ui.separator();

        let shape = &mut self.tool_states[self.tool.index()].shape;
        let (mut import_tips, mut import_pictures, mut remove_tip) = (false, false, None);
        match creator.tab {
            Tab::Stroke => {
                ui.add(egui::Slider::new(&mut shape.radius, 0.5..=200.0).logarithmic(true).text("Size"));
                ui.add(egui::Slider::new(&mut shape.spacing, 0.02..=1.0).text("Spacing"))
                    .on_hover_text("Distance between dabs, as a fraction of the brush width");
                ui.horizontal(|ui| {
                    ui.label("Rotation");
                    egui::ComboBox::from_id_salt("rotation").selected_text(shape.rotation.label()).show_ui(ui, |ui| {
                        for mode in oxide_engine::Rotation::ALL {
                            let item = ui.add(egui::Button::selectable(shape.rotation == mode, mode.label()));
                            let item = if mode.needs_pen() { item.on_hover_text("Uses the pen; with a mouse the tip stays fixed") } else { item };
                            if item.clicked() {
                                shape.rotation = mode;
                            }
                        }
                    });
                })
                .response
                .on_hover_text("None: the tip keeps its angle.\nFollow stroke: turns with the stroke direction (flat brushes, knives).\nPen tilt: turns toward where the pen leans.\nPen rotation: turns with the pen barrel (falls back to tilt).");
                ui.add(egui::Slider::new(&mut shape.jitter_angle, 0.0..=1.0).text("Jitter angle"))
                    .on_hover_text("Turn each dab at random (1 = any angle). Makes texture tips look natural instead of stamped.");
                ui.add(egui::Slider::new(&mut shape.jitter_size, 0.0..=1.0).text("Jitter size")).on_hover_text("Make each dab a random size, down to 30% at 1");
                ui.add(egui::Slider::new(&mut shape.scatter, 0.0..=1.0).text("Scatter"))
                    .on_hover_text("Throw each dab off the stroke's path, up to one brush radius at 1");
                ui.add(egui::Slider::new(&mut shape.lift_off, 0.0..=1.0).text("Lift-off"))
                    .on_hover_text("How much the end of a stroke thins, fades, and breaks up as the brush lifts off");
                super::panels::taper_controls(ui, shape);
                ui.add(egui::Slider::new(&mut shape.size_pressure, 0.0..=1.0).text("Size ← pressure"))
                    .on_hover_text("How much pen pressure changes the size: 0 = none, 1 = light touch is very thin");
                ui.add(egui::Slider::new(&mut shape.opacity_pressure, 0.0..=1.0).text("Opacity ← pressure"))
                    .on_hover_text("How much pen pressure changes the opacity: 0 = none, 1 = light touch is very faint");
                ui.weak("Stroke and pressure smoothing: Window → Pen settings.");
            }
            Tab::Shape => {
                ui.horizontal(|ui| {
                    ui.label("Tip");
                    egui::ComboBox::from_id_salt("tip").selected_text(shape.tip.name()).show_ui(ui, |ui| {
                        for tip in oxide_engine::Tip::ALL.into_iter().chain(oxide_engine::image_tips()) {
                            ui.selectable_value(&mut shape.tip, tip, tip.name());
                        }
                    });
                })
                .response
                .on_hover_text("Round: a smooth ellipse. The others stamp a texture (sponge, chalk, splatter, dry brush, rag, speckle, or tips you imported); turn on Jitter (Stroke tab) so it doesn't repeat.");
                ui.horizontal(|ui| {
                    import_tips = ui
                        .button("Import brushes…")
                        .on_hover_text("Choose downloaded brush files: Photoshop (.abr), Procreate (.brush, .brushset) or GIMP (.gbr, .gih)")
                        .clicked();
                    import_pictures = ui
                        .button("Picture as tip…")
                        .on_hover_text("Use a picture (PNG, JPEG, TIFF, WebP) as a tip: black on white, white on black, or transparent")
                        .clicked();
                    if let oxide_engine::Tip::Image(image) = shape.tip {
                        if ui.button("Remove tip").on_hover_text("Delete this imported tip from Oxide's tip library").clicked() {
                            remove_tip = Some(image.name.clone());
                        }
                        if image.frame_count() > 1 {
                            ui.weak(format!("{} variations", image.frame_count()));
                        }
                    }
                });
                ui.add_enabled(shape.tip == oxide_engine::Tip::Round, egui::Slider::new(&mut shape.hardness, 0.0..=1.0).text("Hardness"))
                    .on_disabled_hover_text("Texture tips have their own edges");
                ui.add(egui::Slider::new(&mut shape.roundness, 0.05..=1.0).text("Roundness"))
                    .on_hover_text("1 = round; lower = flat, like a chisel or calligraphy nib");
                ui.add(egui::Slider::new(&mut shape.angle, -90.0..=90.0).suffix("°").max_decimals(0).text("Angle"));
                ui.add(egui::Slider::new(&mut shape.roughness, 0.0..=1.0).text("Roughness")).on_hover_text("Ragged, grainy edges instead of a perfect outline");
                ui.add(egui::Slider::new(&mut shape.streaks, 0.0..=1.0).text("Bristle streaks"))
                    .on_hover_text("Streaks along the stroke, like the hairs of a real brush");
                ui.add(egui::Slider::new(&mut shape.grain, 0.0..=1.0).text("Grain"))
                    .on_hover_text("How much the paper texture shows: high = catches the tooth (dry brush, pencil); 0 = ignores the paper");
                ui.separator();
                let mut is_bristle = shape.bristles.is_some();
                if ui
                    .checkbox(&mut is_bristle, "Bristle brush")
                    .on_hover_text("Made of separate hairs that each carry paint, lag a little, run dry on their own, and leave their own ridge in thick paint. Off = a stamped tip.")
                    .changed()
                {
                    shape.bristles = is_bristle.then(oxide_engine::Bristles::default);
                }
                if let Some(b) = &mut shape.bristles {
                    let mut count = b.count as f32;
                    if ui
                        .add(egui::Slider::new(&mut count, 4.0..=300.0).logarithmic(true).max_decimals(0).text("Bristles"))
                        .on_hover_text("How many hairs. Few = sparse, scratchy strokes; many = full strokes with fine streaks")
                        .changed()
                    {
                        b.count = count.round() as u32;
                    }
                    ui.add(egui::Slider::new(&mut b.thickness, 0.0..=1.0).text("Thickness")).on_hover_text(
                        "How thick each hair is: 0 = thin hairs with gaps between them (scratchy), 0.5 = solid with fine streaks, 1 = fat hairs",
                    );
                    ui.add(egui::Slider::new(&mut b.spread, 0.0..=1.0).text("Spread")).on_hover_text("How much the hairs splay apart when you press harder");
                    ui.add(egui::Slider::new(&mut b.stiffness, 0.0..=1.0).text("Stiffness"))
                        .on_hover_text("Stiff hairs follow the brush exactly; soft hairs lag and fan out on curves");
                    ui.weak("With bristles, Hardness, Roughness and Bristle streaks don't apply: the hairs make the texture.");
                }
            }
        }
        ui.weak("Opacity, loading and oiliness belong to the medium: see Properties.");

        // --- Save as preset ---
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut creator.name).hint_text("Brush name").desired_width(180.0));
            if ui.button("Save as preset").clicked() {
                creator.message = Some(
                    self.presets
                        .save_in(ui.ctx(), &creator.name, &creator.group, &self.tool_states[self.tool.index()].shape)
                        .map(|()| format!("Saved \"{}\"", creator.name.trim())),
                );
            }
        });
        ui.horizontal(|ui| {
            ui.label("Group");
            ui.add(egui::TextEdit::singleline(&mut creator.group).hint_text(crate::presets::MY_BRUSHES).desired_width(130.0))
                .on_hover_text("Which group in the Brushes panel. Type a new name to start a new group.");
            egui::ComboBox::from_id_salt("preset-group").selected_text("▾").width(30.0).show_ui(ui, |ui| {
                for group in self.presets.groups() {
                    if ui.selectable_label(creator.group == group, &group).clicked() {
                        creator.group = group;
                    }
                }
            });
        });
        match &creator.message {
            Some(Ok(text)) => {
                ui.weak(text);
            }
            Some(Err(text)) => {
                ui.colored_label(ui.visuals().error_fg_color, text);
            }
            None => {}
        }

        if import_tips {
            self.import_tips(&ui.ctx().clone(), false);
        }
        if import_pictures {
            self.import_tips(&ui.ctx().clone(), true);
        }
        if let Some(name) = remove_tip {
            match oxide_commands::tip_import::remove_tip(&name) {
                Ok(()) => {
                    self.state_mut().shape.tip = oxide_engine::Tip::Round;
                    self.info(format!("Removed the tip \"{name}\""));
                }
                Err(e) => self.error(e),
            }
        }
    }
}

impl OxideApp {
    /// What the preview paints with: the current tool's brush in a dark
    /// color, so light paint colors still show up on the paper.
    fn stroke_brush_preview(&self) -> Brush {
        let tool = if self.tool.paints() { self.tool } else { crate::tools::Tool::Ink };
        self.state().brush(tool, oxide_engine::Color::rgb(0.15, 0.15, 0.18))
    }
}
