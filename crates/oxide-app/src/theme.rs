//! The look of the app (colors, rounding, spacing, sliders, fonts), as a
//! theme that can be swapped and edited without touching the code.
//!
//! Built-in themes: Graphite (default), Porcelain (light) and Classic.
//! User themes are TOML files in `~/Library/Application Support/Oxide/themes/`.
//! A theme file can start from a built-in one (`base = "Graphite"`) and only
//! list what it changes. The current theme's file is watched while the app
//! runs, so edits show up right away.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Vec2};
use serde::{Deserialize, Serialize};

/// The theme used when none is chosen (or the chosen one is gone).
pub const DEFAULT_THEME: &str = "Graphite";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    /// Dark or light (decides egui's defaults for anything not set here).
    pub dark: bool,
    pub colors: Colors,
    pub shape: Shape,
    pub font: Font,
}

/// Colors, as "#RRGGBB" or "#RRGGBBAA" in theme files.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Colors {
    /// Side panels, menu bar, status bar.
    #[serde(with = "hex")]
    pub background: Color32,
    /// Floating windows and popups.
    #[serde(with = "hex")]
    pub surface: Color32,
    /// Text fields and other sunken areas.
    #[serde(with = "hex")]
    pub inset: Color32,
    /// Around the canvas.
    #[serde(with = "hex")]
    pub workspace: Color32,
    /// Buttons, slider rails, checkboxes: idle, under the pointer, pressed.
    #[serde(with = "hex")]
    pub widget: Color32,
    #[serde(with = "hex")]
    pub widget_hover: Color32,
    #[serde(with = "hex")]
    pub widget_active: Color32,
    /// Separators and outlines.
    #[serde(with = "hex")]
    pub border: Color32,
    #[serde(with = "hex")]
    pub text: Color32,
    /// Hints and secondary labels.
    #[serde(with = "hex")]
    pub text_weak: Color32,
    /// Selected items, slider fill, focus, links.
    #[serde(with = "hex")]
    pub accent: Color32,
    /// Text on top of the accent color.
    #[serde(with = "hex")]
    pub accent_text: Color32,
    /// Informational notes (e.g. "View: rotated").
    #[serde(with = "hex")]
    pub info: Color32,
    #[serde(with = "hex")]
    pub warning: Color32,
    #[serde(with = "hex")]
    pub error: Color32,
}

/// Rounding, spacing and sizes, in points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    /// Corner rounding of windows, menus, and widgets (buttons, fields).
    pub window_radius: u8,
    pub menu_radius: u8,
    pub widget_radius: u8,
    /// Space between widgets: [horizontal, vertical].
    pub item_spacing: [f32; 2],
    /// Space inside buttons: [horizontal, vertical].
    pub button_padding: [f32; 2],
    /// Minimum height of buttons, sliders and fields.
    pub widget_height: f32,
    /// Width of outlines around widgets and windows (0 = none).
    pub border_width: f32,
    pub slider_width: f32,
    /// Thickness of a slider's track.
    pub slider_rail_height: f32,
    /// "circle" or "rect".
    pub slider_handle: String,
    /// Fill the slider's track up to the handle with the accent color.
    pub slider_fill: bool,
    /// Softness of the shadow under windows and popups (0 = no shadow).
    pub shadow: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Font {
    /// "system" (the Mac's own font, SF Pro), "default" (egui's built-in
    /// font), or the path to a .ttf / .otf / .ttc file.
    pub family: String,
    /// Weight for variable fonts like SF Pro (400 = regular, 600 = semibold).
    pub weight: f32,
    pub size: f32,
    pub small_size: f32,
    pub heading_size: f32,
    pub heading_weight: f32,
    pub monospace_size: f32,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Theme {
    /// The built-in themes, in menu order.
    pub fn built_in() -> Vec<Theme> {
        vec![Theme::graphite(), Theme::porcelain(), Theme::classic()]
    }

    /// Clean, modern dark: cool neutral grays, a clear blue accent, soft corners.
    pub fn graphite() -> Theme {
        Theme {
            name: "Graphite".into(),
            dark: true,
            colors: Colors {
                background: rgb(0x1E1F23),
                surface: rgb(0x25272C),
                inset: rgb(0x16171A),
                workspace: rgb(0x2C2E33),
                widget: rgb(0x2E3036),
                widget_hover: rgb(0x383B42),
                widget_active: rgb(0x43464E),
                border: rgb(0x33363C),
                text: rgb(0xE4E6EB),
                text_weak: rgb(0x8D929B),
                accent: rgb(0x4F8CFF),
                accent_text: rgb(0xFFFFFF),
                info: rgb(0x7FB0F5),
                warning: rgb(0xF0B44C),
                error: rgb(0xFF6B6B),
            },
            shape: Shape {
                window_radius: 10,
                menu_radius: 8,
                widget_radius: 6,
                item_spacing: [8.0, 6.0],
                button_padding: [10.0, 4.0],
                widget_height: 24.0,
                border_width: 1.0,
                slider_width: 120.0,
                slider_rail_height: 4.0,
                slider_handle: "circle".into(),
                slider_fill: true,
                shadow: 16,
            },
            font: Font { family: "system".into(), weight: 440.0, size: 13.0, small_size: 11.0, heading_size: 15.0, heading_weight: 620.0, monospace_size: 12.0 },
        }
    }

    /// Light: white surfaces on soft gray, the same layout and accent.
    pub fn porcelain() -> Theme {
        let g = Theme::graphite();
        Theme {
            name: "Porcelain".into(),
            dark: false,
            colors: Colors {
                background: rgb(0xF3F4F6),
                surface: rgb(0xFFFFFF),
                inset: rgb(0xE8EAEE),
                workspace: rgb(0xD9DCE1),
                widget: rgb(0xE6E8EC),
                widget_hover: rgb(0xDCDFE4),
                widget_active: rgb(0xD0D4DB),
                border: rgb(0xD3D6DC),
                text: rgb(0x1D1F23),
                text_weak: rgb(0x6B717C),
                accent: rgb(0x2F6FEB),
                accent_text: rgb(0xFFFFFF),
                info: rgb(0x2F6FEB),
                warning: rgb(0xB7791F),
                error: rgb(0xD64545),
            },
            font: Font { weight: 420.0, ..g.font },
            shape: Shape { shadow: 12, ..g.shape },
        }
    }

    /// Oxide's original look (egui's dark style with a blue accent).
    pub fn classic() -> Theme {
        Theme {
            name: "Classic".into(),
            dark: true,
            colors: Colors {
                background: Color32::from_gray(32),
                surface: Color32::from_gray(36),
                inset: Color32::from_gray(22),
                workspace: Color32::from_gray(52),
                widget: Color32::from_gray(60),
                widget_hover: Color32::from_gray(70),
                widget_active: Color32::from_gray(55),
                border: Color32::from_gray(60),
                text: Color32::from_gray(180),
                text_weak: Color32::from_gray(130),
                accent: rgb(0x3484E4),
                accent_text: Color32::WHITE,
                info: Color32::from_rgb(120, 170, 235),
                warning: Color32::from_rgb(230, 170, 60),
                error: Color32::from_rgb(255, 0, 0),
            },
            shape: Shape {
                window_radius: 6,
                menu_radius: 6,
                widget_radius: 2,
                item_spacing: [8.0, 3.0],
                button_padding: [4.0, 1.0],
                widget_height: 18.0,
                border_width: 1.0,
                slider_width: 100.0,
                slider_rail_height: 8.0,
                slider_handle: "circle".into(),
                slider_fill: false,
                shadow: 16,
            },
            font: Font { family: "default".into(), weight: 400.0, size: 12.5, small_size: 9.0, heading_size: 18.0, heading_weight: 400.0, monospace_size: 12.0 },
        }
    }

    /// Read a theme file. Missing settings come from its `base` theme
    /// (Graphite if none is named).
    pub fn from_toml(text: &str, fallback_name: &str) -> Result<Theme, String> {
        let mut user: toml::Table = text.parse().map_err(|e: toml::de::Error| e.message().to_string())?;
        let base_name = match user.remove("base") {
            Some(toml::Value::String(s)) => s,
            Some(_) => return Err("`base` should be a theme name, like \"Graphite\".".into()),
            None => DEFAULT_THEME.into(),
        };
        let base = Theme::built_in()
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(&base_name))
            .ok_or_else(|| format!("Unknown base theme \"{base_name}\" (Graphite, Porcelain, Classic)."))?;
        let mut merged = toml::Table::try_from(&base).map_err(|e| e.to_string())?;
        merge(&mut merged, user);
        if !text.lines().any(|l| l.trim_start().starts_with("name")) {
            merged.insert("name".into(), toml::Value::String(fallback_name.into()));
        }
        toml::Value::Table(merged).try_into().map_err(|e: toml::de::Error| e.message().to_string())
    }

    /// A theme file listing every setting (a starting point for your own).
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Apply to egui: colors, shapes, spacing, and fonts.
    pub fn apply(&self, ctx: &egui::Context) {
        set_fonts(ctx, &self.font);
        let egui_theme = if self.dark { egui::Theme::Dark } else { egui::Theme::Light };
        ctx.set_theme(egui_theme);
        let style = self.style();
        ctx.set_style_of(egui_theme, style);
    }

    fn style(&self) -> egui::Style {
        let (c, s, f) = (&self.colors, &self.shape, &self.font);
        let mut style = egui::Style { visuals: if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() }, ..Default::default() };

        // Text sizes.
        let heading = if f.family == "default" { FontFamily::Proportional } else { FontFamily::Name(HEADING.into()) };
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(f.small_size)),
            (TextStyle::Body, FontId::proportional(f.size)),
            (TextStyle::Button, FontId::proportional(f.size)),
            (TextStyle::Heading, FontId::new(f.heading_size, heading)),
            (TextStyle::Monospace, FontId::monospace(f.monospace_size)),
        ]
        .into();

        // Spacing and sizes.
        let sp = &mut style.spacing;
        sp.item_spacing = Vec2::from(s.item_spacing);
        sp.button_padding = Vec2::from(s.button_padding);
        sp.interact_size.y = s.widget_height;
        sp.slider_width = s.slider_width;
        sp.slider_rail_height = s.slider_rail_height;
        sp.menu_margin = Margin::same(6);
        sp.window_margin = Margin::same(10);

        // Colors and shapes.
        let v = &mut style.visuals;
        v.override_text_color = None;
        v.panel_fill = c.background;
        v.window_fill = c.surface;
        v.extreme_bg_color = c.inset;
        v.text_edit_bg_color = Some(c.inset);
        v.faint_bg_color = c.widget;
        v.code_bg_color = c.inset;
        v.hyperlink_color = c.accent;
        v.warn_fg_color = c.warning;
        v.error_fg_color = c.error;
        v.weak_text_color = Some(c.text_weak);
        v.selection.bg_fill = c.accent;
        v.selection.stroke = Stroke::new(1.0, c.accent_text);
        v.window_corner_radius = CornerRadius::same(s.window_radius);
        v.menu_corner_radius = CornerRadius::same(s.menu_radius);
        v.window_stroke = Stroke::new(s.border_width, c.border);
        let shadow = Shadow { offset: [0, (s.shadow / 4) as i8], blur: s.shadow, spread: 0, color: Color32::from_black_alpha(if self.dark { 110 } else { 40 }) };
        v.window_shadow = shadow;
        v.popup_shadow = Shadow { blur: s.shadow / 2, offset: [0, (s.shadow / 8) as i8], ..shadow };
        v.slider_trailing_fill = s.slider_fill;
        v.handle_shape = if s.slider_handle == "rect" { egui::style::HandleShape::Rect { aspect_ratio: 0.5 } } else { egui::style::HandleShape::Circle };
        v.striped = false;
        v.indent_has_left_vline = false;

        let radius = CornerRadius::same(s.widget_radius);
        let w = &mut v.widgets;
        let state = |fill: Color32, border: Color32, text: Color32| egui::style::WidgetVisuals {
            bg_fill: fill,
            weak_bg_fill: fill,
            bg_stroke: Stroke::new(s.border_width, border),
            corner_radius: radius,
            fg_stroke: Stroke::new(1.0, text),
            expansion: 0.0,
        };
        // Labels, separators and frames (not clickable).
        w.noninteractive = state(c.background, c.border, c.text);
        w.inactive = state(c.widget, Color32::TRANSPARENT, c.text);
        w.hovered = state(c.widget_hover, c.border, c.text);
        w.active = state(c.widget_active, c.accent, c.text);
        w.open = state(c.widget_active, c.border, c.text);
        // A slider handle a touch larger when grabbed.
        w.hovered.expansion = 1.0;
        w.active.expansion = 1.0;
        style
    }
}

/// Merge `user` over `base`, table by table.
fn merge(base: &mut toml::Table, user: toml::Table) {
    for (key, value) in user {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(b)), toml::Value::Table(u)) => merge(b, u),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

const UI: &str = "oxide-ui";
const HEADING: &str = "oxide-heading";

/// Font file bytes, read once per path (fonts are kept for the app's life).
fn font_bytes(path: &Path) -> Option<&'static [u8]> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, &'static [u8]>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    if let Some(bytes) = cache.get(path) {
        return Some(bytes);
    }
    let bytes: &'static [u8] = Box::leak(fs::read(path).ok()?.into_boxed_slice());
    cache.insert(path.to_path_buf(), bytes);
    Some(bytes)
}

/// The font file a theme asks for (None = egui's built-in font).
fn font_path(family: &str) -> Option<PathBuf> {
    match family {
        "default" | "" => None,
        "system" => ["/System/Library/Fonts/SFNS.ttf"].iter().map(PathBuf::from).find(|p| p.exists()),
        path => {
            let path = if let Some(rest) = path.strip_prefix("~/") { dirs::home_dir()?.join(rest) } else { PathBuf::from(path) };
            path.exists().then_some(path)
        }
    }
}

/// Install the theme's font (only when it changed: fonts are costly to rebuild).
fn set_fonts(ctx: &egui::Context, font: &Font) {
    static LAST: Mutex<Option<(String, u32, u32)>> = Mutex::new(None);
    let key = (font.family.clone(), font.weight.to_bits(), font.heading_weight.to_bits());
    if let Ok(mut last) = LAST.lock() {
        if last.as_ref() == Some(&key) {
            return;
        }
        *last = Some(key);
    }
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = font_path(&font.family).and_then(|p| font_bytes(&p)) {
        let weighted = |weight: f32| {
            let mut data = FontData::from_static(bytes);
            data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
            data
        };
        fonts.font_data.insert(UI.into(), weighted(font.weight).into());
        fonts.font_data.insert(HEADING.into(), weighted(font.heading_weight).into());
        // egui's fonts stay behind ours for symbols ours doesn't have (◐ ⏷ ➰ …).
        let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, UI.into());
        fonts.families.insert(FontFamily::Name(HEADING.into()), std::iter::once(HEADING.to_string()).chain(fallbacks).collect());
    } else {
        // (The heading family must exist even without a custom font.)
        let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        fonts.families.insert(FontFamily::Name(HEADING.into()), proportional);
    }
    // Phosphor icons (tool buttons), as a fallback behind the text font.
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}

/// A quiet outline for swatches and chips: visible on dark and light themes.
pub fn outline(ui: &egui::Ui) -> Color32 {
    ui.visuals().weak_text_color().gamma_multiply(0.6)
}

// ----- Theme files -----

/// Where user themes live.
pub fn themes_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Oxide").join("themes"))
}

/// A theme the menu can offer: built in, or a file.
#[derive(Clone, Debug)]
pub struct ThemeChoice {
    pub name: String,
    pub file: Option<PathBuf>,
}

/// Every theme by name: user files (a file can replace a built-in theme of
/// the same name), then the built-in ones.
pub fn available() -> Vec<ThemeChoice> {
    let mut files: Vec<ThemeChoice> = themes_dir()
        .and_then(|dir| fs::read_dir(dir).ok())
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .filter_map(|p| Some(ThemeChoice { name: p.file_stem()?.to_string_lossy().into_owned(), file: Some(p) }))
        .collect();
    files.sort_by_key(|c| c.name.to_lowercase());
    let mut all: Vec<ThemeChoice> =
        Theme::built_in().into_iter().filter(|t| !files.iter().any(|f| f.name.eq_ignore_ascii_case(&t.name))).map(|t| ThemeChoice { name: t.name, file: None }).collect();
    all.extend(files);
    all
}

/// Load a theme by name.
pub fn load(choice: &ThemeChoice) -> Result<Theme, String> {
    match &choice.file {
        Some(path) => {
            let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Theme::from_toml(&text, &choice.name).map_err(|e| format!("Theme \"{}\": {e}", choice.name))
        }
        None => Theme::built_in().into_iter().find(|t| t.name == choice.name).ok_or_else(|| format!("No theme named \"{}\".", choice.name)),
    }
}

/// Make sure the themes folder exists, with a starter theme to edit if it's empty.
pub fn prepare_themes_dir() -> Result<PathBuf, String> {
    let dir = themes_dir().ok_or("Couldn't find the Application Support folder.")?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let empty = fs::read_dir(&dir).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).all(|e| e.path().extension().is_none_or(|x| x != "toml"));
    if empty {
        let starter = Theme { name: "My Theme".into(), ..Theme::graphite() };
        let text = format!("{STARTER_HEADER}{}", starter.to_toml());
        fs::write(dir.join("My Theme.toml"), text).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

const STARTER_HEADER: &str = "\
# An Oxide theme. Choose it in View → Theme; edits show up as soon as you save.
#
# Start from a built-in theme and keep only the lines you change, e.g.:
#   base = \"Graphite\"        # or \"Porcelain\" (light) or \"Classic\"
#   [colors]
#   accent = \"#E8590C\"
#
# Colors are \"#RRGGBB\" or \"#RRGGBBAA\". Sizes are in points.
# font.family: \"system\" (SF Pro), \"default\" (egui's font), or a font file path.
# shape.slider_handle: \"circle\" or \"rect\".

";

/// "#RRGGBB" / "#RRGGBBAA" ↔ Color32.
mod hex {
    use eframe::egui::Color32;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(c: &Color32, s: S) -> Result<S::Ok, S::Error> {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let text = if a == 255 { format!("#{r:02X}{g:02X}{b:02X}") } else { format!("#{r:02X}{g:02X}{b:02X}{a:02X}") };
        s.serialize_str(&text)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color32, D::Error> {
        let text = String::deserialize(d)?;
        parse(&text).ok_or_else(|| serde::de::Error::custom(format!("\"{text}\" isn't a color (use \"#RRGGBB\" or \"#RRGGBBAA\")")))
    }

    pub fn parse(text: &str) -> Option<Color32> {
        let h = text.trim().strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_round_trip_through_toml() {
        for theme in Theme::built_in() {
            let back = Theme::from_toml(&theme.to_toml(), "x").unwrap();
            assert_eq!(back, theme);
        }
    }

    #[test]
    fn a_theme_file_only_lists_what_it_changes() {
        let t = Theme::from_toml("base = \"Porcelain\"\n[colors]\naccent = \"#E8590C\"\n[shape]\nwidget_radius = 0\n", "Orange").unwrap();
        assert_eq!(t.name, "Orange");
        assert!(!t.dark, "from Porcelain");
        assert_eq!(t.colors.accent, Color32::from_rgb(0xE8, 0x59, 0x0C));
        assert_eq!(t.shape.widget_radius, 0);
        assert_eq!(t.colors.text, Theme::porcelain().colors.text, "the rest comes from the base");
        // No base: Graphite.
        assert_eq!(Theme::from_toml("", "Empty").unwrap().colors, Theme::graphite().colors);
    }

    #[test]
    fn bad_theme_files_say_what_is_wrong() {
        let e = Theme::from_toml("[colors]\naccent = \"blue\"\n", "x").unwrap_err();
        assert!(e.contains("isn't a color"), "{e}");
        let e = Theme::from_toml("base = \"Neon\"\n", "x").unwrap_err();
        assert!(e.contains("Unknown base theme"), "{e}");
        assert!(Theme::from_toml("[colors\n", "x").is_err());
    }

    #[test]
    fn hex_colors_parse_with_and_without_alpha() {
        assert_eq!(hex::parse("#4F8CFF"), Some(Color32::from_rgb(0x4F, 0x8C, 0xFF)));
        assert_eq!(hex::parse("#00000080"), Some(Color32::from_rgba_unmultiplied(0, 0, 0, 0x80)));
        assert_eq!(hex::parse("4F8CFF"), None);
        assert_eq!(hex::parse("#12345"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_system_font_is_a_variable_font_with_weight() {
        let Some(path) = font_path("system") else { return };
        let data = FontData::from_static(font_bytes(&path).unwrap());
        assert!(data.variation_axes().iter().any(|a| a.tag == "wght"), "SF Pro has a weight axis");
    }
}
