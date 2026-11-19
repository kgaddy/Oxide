//! Help → Oxide Help: the wiki's "Using Oxide" pages inside the app, with a
//! contents list, search, Back, and links between pages.
//!
//! The pages are read (never written) from a copy of the wiki:
//! `scripts/bundle-macos.sh` copies the wiki's Markdown into
//! `Oxide.app/Contents/Resources/help/`. Running from a source checkout, the
//! wiki clone next to the repo is used instead. With neither, the window
//! offers the online wiki. Wiki links (`[[text|Page]]`) are turned into
//! in-app links in memory as each page is loaded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_phosphor::regular as icons;

use super::OxideApp;
use crate::panel_layout::Section;
use crate::tools::Tool;

/// The online wiki, for pages that aren't bundled (and the "Online" button).
pub const ONLINE: &str = "https://github.com/kgaddy/Oxide/wiki";
/// The first page, and the wiki's landing page.
const HOME: &str = "Home";
/// Link destinations that stay inside the help window.
const SCHEME: &str = "oxide-help:";

/// The help page for a tool, e.g. `Oxide-Ink-Tool`.
pub fn tool_page(tool: Tool) -> String {
    format!("Oxide-{}-Tool", tool.label().replace(' ', "-"))
}

/// The help page for a panel section (the "?" in its title).
pub fn section_page(section: Section, tool: Tool) -> String {
    match section {
        Section::Tools | Section::Properties => tool_page(tool),
        Section::BrushPresets => "Oxide-Tools-&-Brush-Properties".into(),
        Section::Paper => "Oxide-Paper-&-Canvas-Surfaces".into(),
        _ => "Oxide-UI-Layout".into(),
    }
}

pub struct Help {
    pub open: bool,
    /// Loaded the first time the window opens (Err: no help pages found).
    book: Option<Result<Book, String>>,
    /// The page shown (file name without `.md`).
    page: String,
    back: Vec<String>,
    search: String,
    cache: CommonMarkCache,
    scroll_to_top: bool,
}

impl Default for Help {
    fn default() -> Self {
        Self {
            open: false,
            book: None,
            page: HOME.into(),
            back: Vec::new(),
            search: String::new(),
            cache: CommonMarkCache::default(),
            scroll_to_top: false,
        }
    }
}

impl Help {
    /// Open the window on a page.
    pub fn show_page(&mut self, page: &str) {
        self.open = true;
        self.search.clear();
        self.go(page.to_string());
    }

    fn go(&mut self, page: String) {
        if page != self.page {
            self.back.push(std::mem::replace(&mut self.page, page));
            self.scroll_to_top = true;
        }
    }

    fn load(&mut self, ctx: &egui::Context) {
        let book = match help_dir() {
            Some(dir) => Book::load(&dir),
            None => Err("The help pages aren't included in this copy of Oxide.".into()),
        };
        if let Ok(book) = &book {
            for key in book.pages.keys() {
                self.cache.add_link_hook(format!("{SCHEME}{key}"));
            }
            egui_extras::install_image_loaders(ctx);
        }
        self.book = Some(book);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let Help { book, page, back, search, cache, scroll_to_top, .. } = self;
        let mut go: Option<String> = None;
        let mut go_back = false;

        ui.horizontal(|ui| {
            if ui.add_enabled(!back.is_empty(), egui::Button::new(format!("{} Back", icons::ARROW_LEFT))).clicked() {
                go_back = true;
            }
            ui.add(egui::TextEdit::singleline(search).hint_text(format!("{} Search help", icons::MAGNIFYING_GLASS)).desired_width(220.0));
            if !search.is_empty() && ui.small_button(icons::X).on_hover_text("Clear the search").clicked() {
                search.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(format!("{} Online", icons::GLOBE)).on_hover_text("Open this page in the online wiki").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(online_url(page)));
                }
            });
        });
        ui.separator();

        let book = match book {
            Some(Ok(book)) => book,
            Some(Err(message)) => {
                ui.label(message.as_str());
                ui.add_space(6.0);
                if ui.button(format!("{} Open the online wiki", icons::GLOBE)).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(ONLINE));
                }
                return;
            }
            None => return,
        };
        let shown = if book.pages.contains_key(page.as_str()) { page.as_str() } else { HOME };

        let height = ui.available_height().max(240.0);
        ui.horizontal_top(|ui| {
            // Contents, or search results.
            ui.allocate_ui_with_layout(egui::vec2(200.0, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(200.0);
                egui::ScrollArea::vertical().id_salt("help-contents").auto_shrink([false, false]).show(ui, |ui| {
                    let query = search.trim().to_lowercase();
                    if query.is_empty() {
                        for entry in &book.contents {
                            match entry {
                                Entry::Heading(text) => {
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(text).strong());
                                }
                                Entry::Link { text, page: key, hint } => {
                                    let mut r = ui.selectable_label(shown == key, text);
                                    if !hint.is_empty() {
                                        r = r.on_hover_text(hint);
                                    }
                                    if r.clicked() {
                                        go = Some(key.clone());
                                    }
                                }
                            }
                        }
                    } else {
                        let hits = book.search(&query);
                        if hits.is_empty() {
                            ui.weak("No pages match.");
                        }
                        for (key, title) in hits {
                            if ui.selectable_label(shown == key, title).clicked() {
                                go = Some(key.to_string());
                            }
                        }
                    }
                });
            });
            ui.separator();
            // The page.
            let mut area = egui::ScrollArea::vertical().id_salt(("help-page", shown)).auto_shrink([false, false]).max_height(height);
            if std::mem::take(scroll_to_top) {
                area = area.vertical_scroll_offset(0.0);
            }
            area.show(ui, |ui| {
                if let Some(p) = book.pages.get(shown) {
                    ui.heading(&p.title);
                    ui.add_space(4.0);
                    CommonMarkViewer::new().show(ui, cache, &p.markdown);
                }
            });
        });

        // A link to another help page was clicked.
        if let Some((hook, _)) = cache.link_hooks().iter().find(|(_, clicked)| **clicked) {
            go = Some(hook.trim_start_matches(SCHEME).to_string());
        }
        if go_back {
            if let Some(previous) = self.back.pop() {
                self.page = previous;
                self.scroll_to_top = true;
            }
        } else if let Some(page) = go {
            self.search.clear();
            self.go(page);
        }
    }
}

impl OxideApp {
    /// Help → Oxide Help.
    pub(super) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help.open {
            return;
        }
        if self.help.book.is_none() {
            self.help.load(ctx);
        }
        let mut open = true;
        egui::Window::new("Oxide Help")
            .open(&mut open)
            .default_size([820.0, 580.0])
            .min_size([520.0, 320.0])
            .resizable(true)
            .show(ctx, |ui| self.help.ui(ui));
        self.help.open &= open;
    }
}

/// Where the help pages are: an override, the copy inside Oxide.app, or the
/// wiki clone next to a source checkout.
fn help_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = std::env::var_os("OXIDE_HELP_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    if let Some(macos) = std::env::current_exe().ok().as_deref().and_then(Path::parent) {
        candidates.push(macos.join("../Resources/help"));
    }
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../Oxide.wiki/Oxide.wiki"));
    candidates.into_iter().find(|dir| dir.join("Home.md").is_file())
}

fn online_url(page: &str) -> String {
    if page == HOME { ONLINE.to_string() } else { format!("{ONLINE}/{page}") }
}

/// The help pages, ready to show.
struct Book {
    contents: Vec<Entry>,
    pages: HashMap<String, Page>,
}

struct Page {
    title: String,
    /// Markdown with wiki links and image paths rewritten for the app.
    markdown: String,
    /// The source, lowercased, for search.
    lower: String,
}

#[derive(Debug, PartialEq)]
enum Entry {
    Heading(String),
    Link { text: String, page: String, hint: String },
}

impl Book {
    fn load(dir: &Path) -> Result<Book, String> {
        let mut sources = HashMap::new();
        let read = std::fs::read_dir(dir).map_err(|e| format!("Couldn't read the help pages: {e}"))?;
        for entry in read.flatten() {
            let path = entry.path();
            let Some(key) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            // _Sidebar and _Footer are wiki chrome, not pages.
            if path.extension().is_some_and(|e| e == "md")
                && !key.starts_with('_')
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                sources.insert(key.to_string(), text);
            }
        }
        Ok(Book::from_sources(sources, &|name| {
            let path = dir.join(name);
            path.is_file().then_some(path)
        }))
    }

    /// `sources`: page name → wiki Markdown. `image`: finds a bundled image.
    fn from_sources(sources: HashMap<String, String>, image: &dyn Fn(&str) -> Option<PathBuf>) -> Book {
        let keys: HashMap<String, String> = sources.keys().map(|k| (k.to_lowercase(), k.clone())).collect();
        let mut contents = parse_contents(sources.get(HOME).map_or("", String::as_str), &keys);
        if contents.len() <= 1 {
            // No "Using Oxide" list: list every page.
            let mut all: Vec<&String> = sources.keys().filter(|k| *k != HOME).collect();
            all.sort();
            contents.extend(all.into_iter().map(|k| Entry::Link { text: title_of(k), page: k.clone(), hint: String::new() }));
        }
        let pages = sources
            .into_iter()
            .map(|(key, src)| {
                let page = Page { title: title_of(&key), markdown: rewrite(&src, &keys, image), lower: src.to_lowercase() };
                (key, page)
            })
            .collect();
        Book { contents, pages }
    }

    /// Pages containing `query` (lowercase): title matches first.
    fn search(&self, query: &str) -> Vec<(&str, &str)> {
        let mut hits: Vec<(bool, &str, &str)> = self
            .pages
            .iter()
            .filter_map(|(key, p)| {
                let in_title = p.title.to_lowercase().contains(query);
                (in_title || p.lower.contains(query)).then_some((!in_title, p.title.as_str(), key.as_str()))
            })
            .collect();
        hits.sort();
        hits.into_iter().map(|(_, title, key)| (key, title)).collect()
    }
}

/// `Oxide-Oils-&-Acrylics-Tool` → "Oils & Acrylics Tool".
fn title_of(key: &str) -> String {
    if key == HOME {
        return "Welcome".into();
    }
    key.strip_prefix("Oxide-").unwrap_or(key).replace('-', " ")
}

/// A wiki page name ("Oxide Ink Tool", "Oxide-Ink-Tool#section") → the
/// bundled page's key, if there is one.
fn find_page(target: &str, keys: &HashMap<String, String>) -> Option<String> {
    let name = target.split('#').next().unwrap_or("").trim().replace(' ', "-");
    keys.get(&name.to_lowercase()).cloned()
}

fn link_to(target: &str, keys: &HashMap<String, String>) -> String {
    match find_page(target, keys) {
        Some(key) => format!("{SCHEME}{key}"),
        None => format!("{ONLINE}/{}", target.trim().replace(' ', "-")),
    }
}

/// The "Using Oxide" section of Home: bold lines become headings, wiki links
/// become entries (hover shows the rest of the line).
fn parse_contents(home: &str, keys: &HashMap<String, String>) -> Vec<Entry> {
    let mut entries = vec![Entry::Link { text: title_of(HOME), page: HOME.into(), hint: String::new() }];
    let Some(section) = home.split("\n## ").find(|s| s.starts_with("Using Oxide")) else { return entries };
    for line in section.lines().skip(1) {
        let line = line.trim().trim_start_matches("- ").trim();
        if !line.contains("[[") {
            // "**Tools** (shortcut in brackets)" → "Tools".
            if let Some((heading, _)) = line.strip_prefix("**").and_then(|l| l.split_once("**")) {
                entries.push(Entry::Heading(heading.to_string()));
            }
            continue;
        }
        let hint = plain_text(line);
        for (text, target) in wiki_links(line) {
            if let Some(page) = find_page(target, keys) {
                entries.push(Entry::Link { text: text.to_string(), page, hint: hint.clone() });
            }
        }
    }
    entries
}

/// Every `[[text|Page]]` / `[[Page]]` in a line, as (text, page).
fn wiki_links(line: &str) -> Vec<(&str, &str)> {
    let mut links = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else { break };
        let inner = &rest[start + 2..start + 2 + len];
        let (text, target) = inner.split_once('|').unwrap_or((inner, inner));
        links.push((text.trim(), target.trim()));
        rest = &rest[start + 2 + len + 2..];
    }
    links
}

/// A contents line without its Markdown, for the hover text.
fn plain_text(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else { break };
        out.push_str(&rest[..start]);
        let inner = &rest[start + 2..start + 2 + len];
        out.push_str(inner.split_once('|').map_or(inner, |(text, _)| text));
        rest = &rest[start + 2 + len + 2..];
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "")
}

/// Wiki Markdown → Markdown the viewer can show: wiki links become in-app
/// links (or online ones, for pages that aren't bundled), links to other
/// wiki pages by file name likewise, and bundled images become file paths.
fn rewrite(src: &str, keys: &HashMap<String, String>, image: &dyn Fn(&str) -> Option<PathBuf>) -> String {
    let src = html_pictures(&tables_to_lists(src));
    // Wiki links.
    let mut text = String::with_capacity(src.len());
    let mut rest = src.as_str();
    while let Some(start) = rest.find("[[") {
        let Some(len) = rest[start + 2..].find("]]") else { break };
        text.push_str(&rest[..start]);
        let inner = &rest[start + 2..start + 2 + len];
        let (label, target) = inner.split_once('|').unwrap_or((inner, inner));
        text.push_str(&format!("[{}]({})", label.trim(), link_to(target, keys)));
        rest = &rest[start + 2 + len + 2..];
    }
    text.push_str(rest);

    // Markdown links and images: `[x](dest)`, `![x](dest)`.
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find("](") {
        let Some(len) = rest[i + 2..].find(')') else { break };
        let dest = &rest[i + 2..i + 2 + len];
        let opener = rest[..i].rfind('[');
        let is_image = opener.is_some_and(|o| o > 0 && rest.as_bytes()[o - 1] == b'!');
        let replacement = if dest.starts_with("Attachments/") {
            match image(dest) {
                Some(path) => Some(format!("<file://{}>", path.display())),
                None if is_image => {
                    // Not bundled: say so instead of a broken picture.
                    let o = opener.unwrap_or(i);
                    let alt = &rest[o + 1..i];
                    out.push_str(&rest[..o - 1]);
                    out.push_str(&format!("*[picture: {alt}]({ONLINE})*"));
                    rest = &rest[i + 2 + len + 1..];
                    continue;
                }
                None => None,
            }
        } else if !dest.contains(':') && !dest.starts_with('#') && !dest.contains(' ') {
            find_page(dest, keys).map(|key| format!("{SCHEME}{key}"))
        } else {
            None
        };
        out.push_str(&rest[..i + 2]);
        out.push_str(replacement.as_deref().unwrap_or(dest));
        out.push(')');
        rest = &rest[i + 2 + len + 1..];
    }
    out.push_str(rest);
    out
}

/// The text between `open` and `close` in `s`, and what follows `close`.
fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<(&'a str, &'a str)> {
    let start = s.find(open)? + open.len();
    let len = s[start..].find(close)?;
    Some((&s[start..start + len], &s[start + len + close.len()..]))
}

/// Pages can show pictures with HTML (`<table><td><img src="https://…"><em>caption</em>`),
/// which the viewer would print as raw tags, and which need the internet. Show
/// each picture as a captioned link to it instead; other tags are dropped.
fn html_pictures(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("<table") {
        let Some(end) = rest[start..].find("</table>") else { break };
        out.push_str(&rest[..start]);
        let block = &rest[start..start + end];
        for cell in block.split("<td").skip(1) {
            let Some((url, _)) = between(cell, "src=\"", "\"") else { continue };
            let caption = between(cell, "<em>", "</em>")
                .map(|(c, _)| c)
                .or_else(|| between(cell, "alt=\"", "\"").map(|(a, _)| a))
                .unwrap_or("Picture");
            out.push_str(&format!("*{}*: [open the picture online]({url})\n\n", caption.trim()));
        }
        rest = &rest[start + end + "</table>".len()..];
    }
    out.push_str(rest);
    out
}

/// The viewer's tables never wrap their cells, so wide ones run off the side
/// of the window. Turn each table into a list instead, which wraps:
/// `| Lines | Across or Along |` → `- **Lines**: Across or Along`. With more
/// than two columns, the others are labelled with their headings.
fn tables_to_lists(src: &str) -> String {
    fn cells(line: &str) -> Vec<String> {
        let line = line.trim().trim_start_matches('|');
        let line = line.strip_suffix('|').filter(|_| !line.ends_with("\\|")).unwrap_or(line);
        let mut cells = vec![String::new()];
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&'|') => {
                    chars.next();
                    cells.last_mut().unwrap().push('|');
                }
                '|' => cells.push(String::new()),
                c => cells.last_mut().unwrap().push(c),
            }
        }
        cells.into_iter().map(|c| c.trim().to_string()).collect()
    }
    fn is_separator(line: &str) -> bool {
        let line = line.trim();
        line.contains('-') && line.contains('|') && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
    }

    let lines: Vec<&str> = src.lines().collect();
    let mut out = String::with_capacity(src.len());
    let mut fenced = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if !fenced && line.contains('|') && lines.get(i + 1).is_some_and(|l| is_separator(l)) {
            let header = cells(line);
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                let row = cells(lines[i]);
                let mut item = format!("- **{}**", row.first().map_or("", String::as_str));
                let rest: Vec<(usize, &String)> = row.iter().enumerate().skip(1).filter(|(_, c)| !c.is_empty()).collect();
                for (n, (col, text)) in rest.iter().enumerate() {
                    item.push_str(if n == 0 { ": " } else { " · " });
                    // Two columns read as a sentence; more get their headings.
                    match header.get(*col) {
                        Some(h) if header.len() > 2 && !h.is_empty() => item.push_str(&format!("*{h}:* {text}")),
                        _ => item.push_str(text),
                    }
                }
                out.push_str(&item);
                out.push('\n');
                i += 1;
            }
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    if !src.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(pages: &[(&str, &str)]) -> Book {
        let sources = pages.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Book::from_sources(sources, &|name| (name == "Attachments/here.png").then(|| PathBuf::from("/help/Attachments/here.png")))
    }

    const HOME_MD: &str = "Intro\n\n## Using Oxide\n\nStart with **[[Tools|Oxide Tools & Brush Properties]]**: how.\n\n**Tools** (shortcut)\n- [[Ink|Oxide Ink Tool]] (N): pen and ink\n- [[Missing|Oxide Nowhere]]\n\n## Developing Oxide\n\n- [[Setup|Oxide Setup]]\n";

    #[test]
    fn contents_come_from_using_oxide() {
        let b = book(&[("Home", HOME_MD), ("Oxide-Tools-&-Brush-Properties", ""), ("Oxide-Ink-Tool", ""), ("Oxide-Setup", "")]);
        assert_eq!(
            b.contents,
            vec![
                Entry::Link { text: "Welcome".into(), page: "Home".into(), hint: String::new() },
                Entry::Link { text: "Tools".into(), page: "Oxide-Tools-&-Brush-Properties".into(), hint: "Start with Tools: how.".into() },
                Entry::Heading("Tools".into()),
                Entry::Link { text: "Ink".into(), page: "Oxide-Ink-Tool".into(), hint: "Ink (N): pen and ink".into() },
            ]
        );
    }

    #[test]
    fn wiki_links_become_app_or_online_links() {
        let b = book(&[("Home", ""), ("Oxide-Ink-Tool", "Back to [[Oxide|Home]] · [[Oxide Ink Tool]] · [[x|Oxide Gone]] · [y](Oxide-Ink-Tool#a)")]);
        assert_eq!(
            b.pages["Oxide-Ink-Tool"].markdown,
            format!("Back to [Oxide]({SCHEME}Home) · [Oxide Ink Tool]({SCHEME}Oxide-Ink-Tool) · [x]({ONLINE}/Oxide-Gone) · [y]({SCHEME}Oxide-Ink-Tool)")
        );
        assert_eq!(b.pages["Oxide-Ink-Tool"].title, "Ink Tool");
    }

    #[test]
    fn images_point_at_bundled_files_or_say_they_are_online() {
        let b = book(&[("Home", "![a](Attachments/here.png) ![b](Attachments/gone.png) `f(x)` [c](https://e.com)")]);
        assert_eq!(b.pages["Home"].markdown, format!("![a](<file:///help/Attachments/here.png>) *[picture: b]({ONLINE})* `f(x)` [c](https://e.com)"));
    }

    #[test]
    fn tables_become_wrapping_lists() {
        let src = "Before\n\n| Setting | What it does |\n|---|---|\n| Spacing | Pixels \\| apart |\n| Fade | Thins |\n\nAfter\n```\n| a | b |\n|---|---|\n```\n";
        assert_eq!(
            tables_to_lists(src),
            "Before\n\n- **Spacing**: Pixels | apart\n- **Fade**: Thins\n\n\nAfter\n```\n| a | b |\n|---|---|\n```\n"
        );
        let wide = "| Panel | Does | Tier |\n|---|---|---|\n| Layers | A list | v1 |\n";
        assert_eq!(tables_to_lists(wide), "- **Layers**: *Does:* A list · *Tier:* v1\n\n");
    }

    #[test]
    fn html_picture_tables_become_links() {
        let src = "a\n<table align=\"center\">\n <tr>\n  <td align=\"center\">\n   <img width=\"3\" alt=\"Shot 1\" src=\"https://e.com/1\"><br>\n   <em>Hatch Feature</em>\n  </td>\n  <td><img alt=\"Shot 2\" src=\"https://e.com/2\"></td>\n </tr>\n</table>\nb";
        assert_eq!(
            html_pictures(src),
            "a\n*Hatch Feature*: [open the picture online](https://e.com/1)\n\n*Shot 2*: [open the picture online](https://e.com/2)\n\n\nb"
        );
    }

    #[test]
    fn search_puts_title_matches_first() {
        let b = book(&[("Home", "ink everywhere"), ("Oxide-Ink-Tool", "pens"), ("Oxide-Pencil-Tool", "no")]);
        assert_eq!(b.search("ink"), vec![("Oxide-Ink-Tool", "Ink Tool"), ("Home", "Welcome")]);
    }

    #[test]
    fn every_tool_has_a_page_name() {
        assert_eq!(tool_page(Tool::OilsAcrylics), "Oxide-Oils-&-Acrylics-Tool");
        assert_eq!(tool_page(Tool::Ink), "Oxide-Ink-Tool");
    }
}
