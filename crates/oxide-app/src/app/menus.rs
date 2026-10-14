//! The menus, described once ([`OxideApp::menu_spec`]) and shown either in
//! the macOS menu bar at the top of the screen (see `native`) or, elsewhere,
//! as a menu bar inside the window.

use eframe::egui;

use super::{Command, OxideApp, SHORTCUTS};

pub enum Entry {
    Item { label: String, command: Command, enabled: bool, checked: Option<bool> },
    Separator,
    /// A submenu whose items change (File → Open Recent).
    Submenu { label: &'static str, entries: Vec<Entry> },
}

pub struct Menu {
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

fn item(label: impl Into<String>, command: Command) -> Entry {
    Entry::Item { label: label.into(), command, enabled: true, checked: None }
}

fn item_if(label: impl Into<String>, command: Command, enabled: bool) -> Entry {
    Entry::Item { label: label.into(), command, enabled, checked: None }
}

fn check(label: impl Into<String>, command: Command, checked: bool, enabled: bool) -> Entry {
    Entry::Item { label: label.into(), command, enabled, checked: Some(checked) }
}

/// The keyboard shortcuts that aren't in a menu (single keys and gestures).
pub const OTHER_SHORTCUTS: &[(&str, &str)] = &[
    ("Tools", "B oils & acrylics · N ink · P pencil · M marker · U airbrush · E eraser · S smudge · K palette knife · L select · I picker"),
    ("Oils paint modes", "1 Paint · 2 Paint & Mix · 3 Paint & Blend · 4 Blend · 5 Erase"),
    ("Blend / Erase", "V Blend · A Erase (tap = switch, hold = only while held)"),
    ("Brush size", "[ smaller · ] bigger · ⌘-drag on the canvas: size (←→) and loading (↑↓)"),
    ("View", "R-drag rotate · Shift+F flip · G greyscale · 0 = 100% · . = fit"),
    ("Pan and zoom", "Space+drag or middle-drag to pan · pinch or ⌘+scroll to zoom"),
    ("Layers", "/ lock transparency"),
    ("Select tool (L)", "drag: rectangle / ellipse / lasso · Shift add · ⌥ subtract · Shift+⌥ overlap · click to deselect · ⌫ clears what's selected"),
    ("Mix Color", "hold X and pick a color (swatch, canvas with the Picker, palette, reference image): it mixes into the current color like paint"),
    ("Transform (⌘T)", "drag inside = move · handles = scale (Shift = free) · outside = rotate (Shift = 15°) · arrows nudge · Enter apply · Esc cancel"),
    ("Reference image", "scroll = zoom · drag = pan · double-click = fit · ⌥-click (or Picker) = pick a color"),
];

impl OxideApp {
    /// Every menu, with labels, check marks and enabled states as they are now.
    pub(super) fn menu_spec(&self) -> Vec<Menu> {
        let doc = &self.document;
        let undo = doc.undo_description();
        let redo = doc.redo_description();
        let active = doc.active_index();
        let layer = &doc.layers()[active];
        let count = doc.layers().len();
        // While transforming, layer changes wait until it's applied or cancelled.
        let free = self.transform.is_none();
        use Command::*;
        vec![
            Menu {
                title: "File",
                entries: vec![
                    item("New…", New),
                    item("Open…", Open),
                    Entry::Submenu { label: "Open Recent", entries: self.recent_entries() },
                    Entry::Separator,
                    item("Save", Save),
                    item("Save As…", SaveAs),
                    item("Save Iteration", SaveIteration),
                    Entry::Separator,
                    item("Import Image as Layer…", ImportImage),
                    item("Open Reference Image…", OpenReference),
                    item("Export…", Export),
                ],
            },
            Menu {
                title: "Edit",
                entries: vec![
                    item_if(undo.map_or("Undo".into(), |d| format!("Undo {d}")), Undo, undo.is_some() || !free),
                    item_if(redo.map_or("Redo".into(), |d| format!("Redo {d}")), Redo, redo.is_some()),
                    Entry::Separator,
                    item_if("Cut", Cut, free),
                    item_if("Copy", Copy, free),
                    item_if("Paste", Paste, free),
                    Entry::Separator,
                    check("Natural Colors (mix like paint)", NaturalColors, self.app_settings.natural_colors, true),
                    check("MultiColored Brush", MultiColor, self.multicolor_on, true),
                    item("Swap Primary/Secondary Color", SwapColors),
                ],
            },
            Menu {
                title: "View",
                entries: vec![
                    item("Fit to Window", Fit),
                    item("Actual Size", ActualSize),
                    item("Zoom In", ZoomIn),
                    item("Zoom Out", ZoomOut),
                    Entry::Separator,
                    item("Rotate View Left", RotateLeft),
                    item("Rotate View Right", RotateRight),
                    item_if("Reset Rotation", ResetRotation, self.view.rotation != 0.0),
                    Entry::Separator,
                    check("Flip Horizontal", FlipView, self.view.flip, true),
                    check("Greyscale", Greyscale, self.texture.greyscale(), true),
                ],
            },
            Menu {
                title: "Layer",
                entries: vec![
                    item(if free { "Transform…" } else { "Apply Transform" }, Transform),
                    Entry::Separator,
                    item_if("New Layer", NewLayer, free),
                    item_if("Duplicate Layer", DuplicateLayer, free),
                    item_if("Delete Layer", DeleteLayer, free && count > 1),
                    item_if("Clear Layer", ClearLayer, free),
                    Entry::Separator,
                    item_if(if doc.selection().is_some() { "Add Mask from Selection" } else { "Add Mask" }, AddMask, free && layer.mask.is_none()),
                    check("Paint on Mask", ToggleMaskPainting, self.editing_mask && layer.mask.is_some(), free && layer.mask.is_some()),
                    check("Mask On", ToggleMask, layer.mask.as_ref().is_some_and(|m| m.enabled), free && layer.mask.is_some()),
                    item_if("Apply Mask", ApplyMask, free && layer.mask.is_some()),
                    item_if("Delete Mask", DeleteMask, free && layer.mask.is_some()),
                    Entry::Separator,
                    item_if("Group Layer", GroupLayer, free && layer.group.is_none()),
                    item_if("Ungroup", Ungroup, free && layer.group.is_some()),
                    Entry::Separator,
                    item_if("Merge Down", MergeDown, free && active > 0),
                    item_if("Merge Visible", MergeVisible, free && count > 1),
                    item_if("Merge Visible to New Layer", MergeVisibleToNew, free),
                    Entry::Separator,
                    check("Lock Layer", LockLayer, layer.locked, free),
                    check("Lock Transparency", LockTransparency, layer.lock_transparency(), free),
                    check("Tracing Layer", TracingLayer, layer.tracing, free),
                    Entry::Separator,
                    item_if("Move Layer Up", MoveLayerUp, free && active + 1 < count),
                    item_if("Move Layer Down", MoveLayerDown, free && active > 0),
                    Entry::Separator,
                    item_if("Dry Layer", DryLayer, free),
                    item_if("Dry All Layers", DryAll, free),
                    item_if("Rewet Layer", RewetLayer, free),
                ],
            },
            Menu {
                title: "Select",
                entries: {
                    let has = doc.selection().is_some();
                    vec![
                        item("All", SelectAll),
                        item_if("Deselect", Deselect, has),
                        item_if("Invert", InvertSelection, has),
                        Entry::Separator,
                        item_if("Feather 2 px", Feather(2), has),
                        item_if("Feather 5 px", Feather(5), has),
                        item_if("Feather 10 px", Feather(10), has),
                        item_if("Feather 25 px", Feather(25), has),
                        Entry::Separator,
                        item_if("Fill with Current Color", FillSelection, has && free),
                        item_if("Clear (⌫)", ClearSelection, has && free),
                    ]
                },
            },
            Menu {
                title: "Window",
                entries: vec![
                    check("Tools & Brushes (left)", ToggleLeft, self.show_left, true),
                    check("Navigator, Color & Layers (right)", ToggleRight, self.show_right, true),
                    Entry::Separator,
                    check("Mixing Palette", MixingPalette, self.mixing_palette.open, true),
                    check("Reference Image", ToggleReference, self.reference.open, true),
                    check("Brush Creator", ToggleBrushCreator, self.brush_creator.open, true),
                    check("Pen Settings", TogglePenSettings, self.show_pen_settings, true),
                ],
            },
            Menu { title: "Help", entries: vec![item("Keyboard Shortcuts…", ShowShortcuts)] },
        ]
    }

    /// File → Open Recent: the last paintings (missing ones greyed), then Clear Menu.
    fn recent_entries(&self) -> Vec<Entry> {
        let mut entries: Vec<Entry> = self
            .app_settings
            .recent_files
            .iter()
            .take(10)
            .enumerate()
            .map(|(n, path)| {
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                item_if(name, Command::OpenRecent(n as u8), path.exists())
            })
            .collect();
        if entries.is_empty() {
            entries.push(item_if("No Recent Paintings", Command::ClearRecent, false));
        } else {
            entries.push(Entry::Separator);
            entries.push(item("Clear Menu", Command::ClearRecent));
        }
        entries
    }

    /// The in-window menu bar (used when there's no macOS menu bar).
    pub(super) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let mut clicked = None;
        let menus = self.menu_spec();
        egui::MenuBar::new().ui(ui, |ui| {
            for menu in &menus {
                ui.menu_button(menu.title, |ui| {
                    show_entries(ui, &menu.entries, &mut clicked);
                    if menu.title == "File" {
                        ui.separator();
                        if ui.button("Quit").clicked() {
                            clicked = Some(Command::Quit);
                        }
                    }
                });
            }
        });
        if let Some(command) = clicked {
            self.run(command, &ui.ctx().clone());
        }
    }

    /// Help → Keyboard Shortcuts…
    pub(super) fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_shortcuts;
        egui::Window::new("Keyboard Shortcuts").open(&mut open).default_width(560.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                ui.strong("Menu commands");
                egui::Grid::new("menu-shortcuts").num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for menu in self.menu_spec() {
                        for entry in &menu.entries {
                            if let Entry::Item { label, command, .. } = entry
                                && let Some((_, shortcut)) = SHORTCUTS.iter().find(|(c, _)| c == command)
                            {
                                ui.monospace(ctx.format_shortcut(shortcut));
                                ui.label(label.trim_end_matches('…'));
                                ui.end_row();
                            }
                        }
                    }
                });
                ui.add_space(8.0);
                ui.strong("Keys and gestures");
                egui::Grid::new("other-shortcuts").num_columns(2).spacing([16.0, 4.0]).striped(true).show(ui, |ui| {
                    for (what, keys) in OTHER_SHORTCUTS {
                        ui.label(*what);
                        ui.label(*keys);
                        ui.end_row();
                    }
                });
            });
        });
        self.show_shortcuts = open;
    }
}

/// Menu entries inside the in-window menu bar (submenus recurse).
fn show_entries(ui: &mut egui::Ui, entries: &[Entry], clicked: &mut Option<Command>) {
    for entry in entries {
        match entry {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Submenu { label, entries } => {
                ui.menu_button(*label, |ui| show_entries(ui, entries, clicked));
            }
            Entry::Item { label, command, enabled, checked } => {
                let text = match checked {
                    Some(true) => format!("✔ {label}"),
                    _ => label.clone(),
                };
                let mut button = egui::Button::new(text);
                if let Some((_, shortcut)) = SHORTCUTS.iter().find(|(c, _)| c == command) {
                    button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
                }
                if ui.add_enabled(*enabled, button).clicked() {
                    *clicked = Some(*command);
                }
            }
        }
    }
}

/// The real macOS menu bar (top of the screen), built with `muda`.
#[cfg(target_os = "macos")]
pub mod native {
    use std::collections::HashMap;
    use std::sync::mpsc::{Receiver, channel};

    use eframe::egui::{self, Key, KeyboardShortcut};
    use muda::accelerator::Accelerator;
    use muda::{AboutMetadata, CheckMenuItem, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

    use super::{Command, Entry, Menu};
    use crate::app::SHORTCUTS;

    enum Native {
        Plain(MenuItem),
        Check(CheckMenuItem),
    }

    /// What an item currently shows, so we only touch AppKit when it changes.
    #[derive(PartialEq, Clone)]
    struct Shown {
        label: String,
        enabled: bool,
        checked: Option<bool>,
    }

    /// A submenu item as shown: (label, command, enabled); an empty label is a separator.
    type ShownItem = (String, Command, bool);

    pub struct NativeMenu {
        _menu: muda::Menu,
        items: Vec<(Native, Shown)>,
        commands: HashMap<MenuId, Command>,
        /// Submenus whose items change (Open Recent), with what they show now.
        dynamic: Vec<(Submenu, Vec<ShownItem>)>,
        /// Commands whose shortcut the menu bar handles (macOS sends those keys
        /// to the menu, so the app must not also react to them).
        handled_shortcuts: Vec<Command>,
        events: Receiver<MenuId>,
    }

    /// egui shortcut → menu accelerator (None if the key has no equivalent).
    fn accelerator(shortcut: &KeyboardShortcut) -> Option<Accelerator> {
        let m = shortcut.modifiers;
        let mut text = String::new();
        if m.command || m.mac_cmd {
            text.push_str("cmd+");
        }
        if m.shift {
            text.push_str("shift+");
        }
        if m.alt {
            text.push_str("alt+");
        }
        if m.ctrl && !m.command {
            text.push_str("ctrl+");
        }
        let key = match shortcut.logical_key {
            Key::Plus => return None, // ⌘= already zooms in
            key => key.name(),
        };
        text.push_str(key);
        text.parse().ok()
    }

    /// macOS shows the app menu's *own* title in bold (not the item's), and
    /// without an .app bundle it falls back to the program file name
    /// ("oxide-app"). eframe names its default menu this way too.
    fn name_app_menu(name: &str) {
        let Some(mtm) = objc2_foundation::MainThreadMarker::new() else { return };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(main_menu) = app.mainMenu()
            && let Some(item) = main_menu.itemAtIndex(0)
            && let Some(app_menu) = item.submenu()
        {
            app_menu.setTitle(&objc2_foundation::NSString::from_str(name));
        }
    }

    impl NativeMenu {
        /// Build the menu bar from `menus` and install it. Call once, after the window exists.
        pub fn install(menus: &[Menu], ctx: &egui::Context) -> Option<Self> {
            let menu = muda::Menu::new();
            let mut items = Vec::new();
            let mut commands = HashMap::new();
            let mut handled_shortcuts = Vec::new();
            let mut dynamic = Vec::new();

            // The app menu (named after the app by macOS).
            let quit = MenuItem::with_id("Quit", "Quit Oxide", true, "cmd+q".parse().ok());
            commands.insert(quit.id().clone(), Command::Quit);
            handled_shortcuts.push(Command::Quit);
            let about = PredefinedMenuItem::about(
                Some("About Oxide"),
                Some(AboutMetadata {
                    name: Some("Oxide".into()),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    comments: Some("Natural-media painting: oils & acrylics, pen & ink.".into()),
                    ..Default::default()
                }),
            );
            let app_menu = Submenu::with_items(
                "Oxide",
                true,
                &[
                    &about,
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::services(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::hide(None),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::show_all(None),
                    &PredefinedMenuItem::separator(),
                    &quit,
                ],
            )
            .ok()?;
            menu.append(&app_menu).ok()?;

            for m in menus {
                let submenu = Submenu::new(m.title, true);
                for entry in &m.entries {
                    match entry {
                        Entry::Separator => {
                            submenu.append(&PredefinedMenuItem::separator()).ok()?;
                        }
                        Entry::Submenu { label, .. } => {
                            // Filled in by `sync` (its items change).
                            let inner = Submenu::new(*label, true);
                            submenu.append(&inner).ok()?;
                            dynamic.push((inner, Vec::new()));
                        }
                        Entry::Item { label, command, enabled, checked } => {
                            let accel = SHORTCUTS.iter().find(|(c, _)| c == command).and_then(|(_, s)| accelerator(s));
                            if accel.is_some() {
                                handled_shortcuts.push(*command);
                            }
                            let id = format!("{command:?}");
                            let native = match checked {
                                Some(on) => {
                                    let item = CheckMenuItem::with_id(id, label, *enabled, *on, accel);
                                    submenu.append(&item).ok()?;
                                    Native::Check(item)
                                }
                                None => {
                                    let item = MenuItem::with_id(id, label, *enabled, accel);
                                    submenu.append(&item).ok()?;
                                    Native::Plain(item)
                                }
                            };
                            let item_id = match &native {
                                Native::Plain(i) => i.id().clone(),
                                Native::Check(i) => i.id().clone(),
                            };
                            commands.insert(item_id, *command);
                            items.push((native, Shown { label: label.clone(), enabled: *enabled, checked: *checked }));
                        }
                    }
                }
                if m.title == "Window" {
                    submenu.set_as_windows_menu_for_nsapp();
                }
                if m.title == "Help" {
                    submenu.set_as_help_menu_for_nsapp();
                }
                menu.append(&submenu).ok()?;
            }
            menu.init_for_nsapp();
            name_app_menu("Oxide");

            let (tx, rx) = channel();
            let ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                tx.send(event.id).ok();
                ctx.request_repaint();
            }));
            let mut native = NativeMenu { _menu: menu, items, commands, dynamic, handled_shortcuts, events: rx };
            native.sync(menus);
            Some(native)
        }

        /// Menu items chosen since the last frame.
        pub fn chosen(&self) -> Vec<Command> {
            self.events.try_iter().filter_map(|id| self.commands.get(&id).copied()).collect()
        }

        /// Does the menu bar handle this command's keyboard shortcut?
        pub fn handles_shortcut(&self, command: Command) -> bool {
            self.handled_shortcuts.contains(&command)
        }

        /// Update labels, check marks and enabled states (same structure as at install).
        pub fn sync(&mut self, menus: &[Menu]) {
            // Submenus that change: rebuild when their list differs.
            let submenus = menus.iter().flat_map(|m| &m.entries).filter_map(|e| match e {
                Entry::Submenu { entries, .. } => Some(entries),
                _ => None,
            });
            for ((submenu, shown), entries) in self.dynamic.iter_mut().zip(submenus) {
                let now: Vec<ShownItem> = entries
                    .iter()
                    .map(|e| match e {
                        Entry::Item { label, command, enabled, .. } => (label.clone(), *command, *enabled),
                        _ => (String::new(), Command::ClearRecent, false), // separator
                    })
                    .collect();
                if *shown == now {
                    continue;
                }
                while submenu.remove_at(0).is_some() {}
                for (label, command, enabled) in &now {
                    if label.is_empty() {
                        submenu.append(&PredefinedMenuItem::separator()).ok();
                    } else {
                        let item = MenuItem::with_id(format!("{command:?}"), label, *enabled, None);
                        self.commands.insert(item.id().clone(), *command);
                        submenu.append(&item).ok();
                    }
                }
                *shown = now;
            }
            let entries = menus.iter().flat_map(|m| &m.entries).filter_map(|e| match e {
                Entry::Item { label, enabled, checked, .. } => Some(Shown { label: label.clone(), enabled: *enabled, checked: *checked }),
                _ => None,
            });
            for ((native, shown), now) in self.items.iter_mut().zip(entries) {
                // macOS ticks a check item itself when it's clicked, so compare with what it really shows.
                let check_differs = matches!(native, Native::Check(item) if item.is_checked() != now.checked.unwrap_or(false));
                if *shown == now && !check_differs {
                    continue;
                }
                match native {
                    Native::Plain(item) => {
                        item.set_text(&now.label);
                        item.set_enabled(now.enabled);
                    }
                    Native::Check(item) => {
                        item.set_text(&now.label);
                        item.set_enabled(now.enabled);
                        item.set_checked(now.checked.unwrap_or(false));
                    }
                }
                *shown = now;
            }
        }
    }
}
