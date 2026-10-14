//! Choosing, loading and live-reloading the UI theme (see `crate::theme`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;

use super::menus::{Entry, check, item};
use super::{Command, OxideApp};
use crate::theme::{self, Theme, ThemeChoice};

/// How often the current theme's file is checked for edits.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// The theme in use, and the file it came from (watched for edits).
pub struct ThemeState {
    pub theme: Theme,
    /// (file, when it was last changed), for user themes.
    file: Option<(PathBuf, Option<SystemTime>)>,
    checked: Instant,
}

impl ThemeState {
    /// Load the named theme (or the default if it's gone or broken) and apply it.
    /// Returns an error to show if the named theme couldn't be used.
    pub fn start(ctx: &egui::Context, name: &str) -> (ThemeState, Option<String>) {
        let choice = theme::available().into_iter().find(|c| c.name == name);
        let (theme, file, problem) = match choice.as_ref().map(|c| (c, theme::load(c))) {
            Some((c, Ok(t))) => (t, c.file.clone(), None),
            Some((c, Err(e))) => (Theme::graphite(), c.file.clone(), Some(e)),
            None => (Theme::graphite(), None, None),
        };
        theme.apply(ctx);
        let file = file.map(|p| {
            let changed = modified(&p);
            (p, changed)
        });
        (ThemeState { theme, file, checked: Instant::now() }, problem)
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl OxideApp {
    /// View → Theme: every theme (checked = current), then the folder actions.
    pub(super) fn theme_entries(&self) -> Vec<Entry> {
        let current = &self.app_settings.theme;
        let mut entries: Vec<Entry> =
            theme::available().iter().take(30).enumerate().map(|(n, c)| check(c.name.clone(), Command::Theme(n as u8), &c.name == current, true)).collect();
        entries.push(Entry::Separator);
        entries.push(item("Open Themes Folder", Command::OpenThemesFolder));
        entries.push(item("Reload Themes", Command::ReloadThemes));
        entries
    }

    /// Switch to theme `n` of the menu.
    pub(super) fn choose_theme(&mut self, n: usize, ctx: &egui::Context) {
        if let Some(choice) = theme::available().into_iter().nth(n) {
            self.use_theme(&choice, ctx);
        }
    }

    fn use_theme(&mut self, choice: &ThemeChoice, ctx: &egui::Context) {
        match theme::load(choice) {
            Ok(t) => {
                t.apply(ctx);
                self.themes.theme = t;
                self.themes.file = choice.file.clone().map(|p| {
                    let changed = modified(&p);
                    (p, changed)
                });
                if self.app_settings.theme != choice.name {
                    self.app_settings.theme = choice.name.clone();
                    self.save_app_settings();
                }
            }
            Err(e) => self.error(e),
        }
    }

    /// Reload the current theme from its file (after edits, or Reload Themes).
    pub(super) fn reload_theme(&mut self, ctx: &egui::Context) {
        let name = self.app_settings.theme.clone();
        match theme::available().into_iter().find(|c| c.name == name) {
            Some(choice) => {
                self.use_theme(&choice, ctx);
                self.info(format!("Theme \"{name}\" reloaded"));
            }
            None => self.use_theme(&ThemeChoice { name: theme::DEFAULT_THEME.into(), file: None }, ctx),
        }
    }

    /// Open the themes folder in Finder (with a starter theme if it's empty).
    pub(super) fn open_themes_folder(&mut self) {
        match theme::prepare_themes_dir() {
            Ok(dir) => {
                if let Err(e) = std::process::Command::new("open").arg(&dir).spawn() {
                    self.error(format!("Couldn't open {}: {e}", dir.display()));
                } else {
                    self.info("Edit or add .toml themes here; choose them in View → Theme");
                }
            }
            Err(e) => self.error(format!("Couldn't make the themes folder: {e}")),
        }
    }

    /// Re-apply the current theme when its file is saved (checked about once a second).
    pub(super) fn watch_theme_file(&mut self, ctx: &egui::Context) {
        let Some((path, seen)) = self.themes.file.clone() else { return };
        ctx.request_repaint_after(WATCH_EVERY);
        if self.themes.checked.elapsed() < WATCH_EVERY {
            return;
        }
        self.themes.checked = Instant::now();
        let now = modified(&path);
        if now == seen {
            return;
        }
        self.themes.file = Some((path.clone(), now));
        let choice = ThemeChoice { name: self.app_settings.theme.clone(), file: Some(path) };
        match theme::load(&choice) {
            Ok(t) => {
                t.apply(ctx);
                self.themes.theme = t;
                self.info(format!("Theme \"{}\" updated", choice.name));
            }
            // Keep the last good look while the file is being edited.
            Err(e) => self.error(e),
        }
    }
}
