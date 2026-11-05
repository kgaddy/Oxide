//! Which sections (Tools, Properties, Layers, …) sit in which side panel, and
//! in what order. Each section can be dragged by its title to another spot,
//! on either side; the arrangement is saved with the app settings.

use serde::{Deserialize, Serialize};

/// One block of a side panel, with its own title and fold arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Section {
    Tools,
    /// The controls you picked for the current tool (see `quick_panel`).
    Quick,
    Properties,
    BrushPresets,
    Navigator,
    Color,
    WetMedia,
    Layers,
    Paper,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::Tools,
        Section::Quick,
        Section::Properties,
        Section::BrushPresets,
        Section::Navigator,
        Section::Color,
        Section::WetMedia,
        Section::Layers,
        Section::Paper,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Tools => "Tools",
            Section::Quick => "Quick",
            Section::Properties => "Properties",
            Section::BrushPresets => "Brush presets",
            Section::Navigator => "Navigator",
            Section::Color => "Color",
            Section::WetMedia => "Wet Media",
            Section::Layers => "Layers",
            Section::Paper => "Paper",
        }
    }

    /// The side it starts on.
    fn default_side(self) -> Side {
        match self {
            Section::Tools | Section::Quick | Section::Properties | Section::BrushPresets => Side::Left,
            _ => Side::Right,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// The sections in each side panel, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    pub left: Vec<Section>,
    pub right: Vec<Section>,
    /// Sections folded shut (click a title to fold or open it).
    pub folded: Vec<Section>,
}

impl Default for PanelLayout {
    fn default() -> Self {
        let on = |side| Section::ALL.into_iter().filter(|s| s.default_side() == side).collect();
        // The Quick section has the controls used most, so the full lists start folded.
        PanelLayout { left: on(Side::Left), right: on(Side::Right), folded: vec![Section::Properties, Section::BrushPresets] }
    }
}

impl PanelLayout {
    pub fn side(&self, side: Side) -> &[Section] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn side_mut(&mut self, side: Side) -> &mut Vec<Section> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }

    /// Every section exactly once: drops repeats (left wins) and puts any
    /// missing one (say, added in a newer version) back on its starting side,
    /// below the section it follows by default.
    pub fn repair(&mut self) {
        let mut seen = Vec::new();
        for side in [Side::Left, Side::Right] {
            self.side_mut(side).retain(|s| {
                let new = !seen.contains(s);
                seen.push(*s);
                new
            });
        }
        let default = PanelLayout::default();
        for section in Section::ALL {
            if !seen.contains(&section) {
                let side = section.default_side();
                let before = default.side(side).iter().take_while(|s| **s != section).copied().collect::<Vec<_>>();
                let list = self.side_mut(side);
                // Below the nearest section that comes before it by default, else at the top.
                let at = before.iter().rev().find_map(|b| list.iter().position(|s| s == b)).map_or(0, |i| i + 1);
                list.insert(at, section);
            }
        }
        self.folded.dedup();
    }

    pub fn is_folded(&self, section: Section) -> bool {
        self.folded.contains(&section)
    }

    pub fn set_folded(&mut self, section: Section, folded: bool) {
        self.folded.retain(|s| *s != section);
        if folded {
            self.folded.push(section);
        }
    }

    /// Move `section` so it sits at `index` in `side` (an index into the list
    /// as it was before the move, as the drop line shows it). Returns whether
    /// anything changed.
    pub fn move_section(&mut self, section: Section, side: Side, mut index: usize) -> bool {
        let before = self.clone();
        for from in [Side::Left, Side::Right] {
            if let Some(at) = self.side(from).iter().position(|s| *s == section) {
                self.side_mut(from).remove(at);
                if from == side && at < index {
                    index -= 1;
                }
            }
        }
        let list = self.side_mut(side);
        list.insert(index.min(list.len()), section);
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_has_every_section_once() {
        let layout = PanelLayout::default();
        assert_eq!(layout.left, [Section::Tools, Section::Quick, Section::Properties, Section::BrushPresets]);
        assert_eq!(layout.left.len() + layout.right.len(), Section::ALL.len());
    }

    #[test]
    fn moving_within_a_side_reorders_it() {
        let mut layout = PanelLayout { left: vec![Section::Tools, Section::Properties, Section::BrushPresets], ..PanelLayout::default() };
        // Drop Properties above Tools.
        assert!(layout.move_section(Section::Properties, Side::Left, 0));
        assert_eq!(layout.left, [Section::Properties, Section::Tools, Section::BrushPresets]);
        // Drop Properties below Brush presets (index 3 = after the last one).
        assert!(layout.move_section(Section::Properties, Side::Left, 3));
        assert_eq!(layout.left, [Section::Tools, Section::BrushPresets, Section::Properties]);
        // Dropping it right where it is changes nothing.
        assert!(!layout.move_section(Section::Properties, Side::Left, 2));
        assert!(!layout.move_section(Section::Properties, Side::Left, 3));
    }

    #[test]
    fn moving_across_sides() {
        let mut layout = PanelLayout::default();
        assert!(layout.move_section(Section::Layers, Side::Left, 1));
        assert_eq!(layout.left, [Section::Tools, Section::Layers, Section::Quick, Section::Properties, Section::BrushPresets]);
        assert!(!layout.right.contains(&Section::Layers));
        // A side can be emptied, and filled again.
        for s in layout.left.clone() {
            layout.move_section(s, Side::Right, 0);
        }
        assert!(layout.left.is_empty());
        layout.move_section(Section::Tools, Side::Left, 5);
        assert_eq!(layout.left, [Section::Tools]);
    }

    #[test]
    fn repair_fixes_repeats_and_missing_sections() {
        let mut layout: PanelLayout = toml::from_str("left = [\"Layers\", \"Tools\"]\nright = [\"Tools\", \"Color\"]").unwrap();
        layout.repair();
        // Missing sections go back at about their default place.
        assert_eq!(layout.left, [Section::Layers, Section::Tools, Section::Quick, Section::Properties, Section::BrushPresets]);
        assert_eq!(layout.right, [Section::Navigator, Section::Color, Section::WetMedia, Section::Paper]);
        // A saved layout from before the Quick section gets it below Tools.
        let mut old: PanelLayout = toml::from_str("left = [\"Tools\", \"Properties\", \"BrushPresets\"]\nright = [\"Navigator\", \"Color\", \"WetMedia\", \"Layers\", \"Paper\"]").unwrap();
        old.repair();
        assert_eq!(old.left, [Section::Tools, Section::Quick, Section::Properties, Section::BrushPresets]);
    }

    #[test]
    fn folding_is_remembered_once() {
        let mut layout = PanelLayout::default();
        layout.set_folded(Section::Color, true);
        layout.set_folded(Section::Color, true);
        assert_eq!(layout.folded.iter().filter(|s| **s == Section::Color).count(), 1);
        layout.set_folded(Section::Color, false);
        assert!(!layout.is_folded(Section::Color));
    }
}
