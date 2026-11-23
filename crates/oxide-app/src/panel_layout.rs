//! Which sections (Tools, Properties, Layers, …) belong to the left and right
//! rails and in what order; saved with the app settings.

use serde::{Deserialize, Serialize};

/// One block of the interface: a rail button that opens it as a popover.
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

/// A popover left open in the minimal layout, remembered between runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPopover {
    pub section: Section,
    /// Its top-left corner if it was dragged somewhere; otherwise it sits beside its rail.
    #[serde(default)]
    pub pos: Option<[f32; 2]>,
    /// Its height if it was resized; otherwise it fits its content.
    #[serde(default)]
    pub height: Option<f32>,
}

/// The sections on each rail, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    pub left: Vec<Section>,
    pub right: Vec<Section>,
}

impl Default for PanelLayout {
    fn default() -> Self {
        let on = |side| Section::ALL.into_iter().filter(|s| s.default_side() == side).collect();
        PanelLayout { left: on(Side::Left), right: on(Side::Right) }
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
    fn repair_fixes_repeats_and_missing_sections() {
        let mut layout: PanelLayout = toml::from_str("left = [\"Layers\", \"Tools\"]\nright = [\"Tools\", \"Color\"]").unwrap();
        layout.repair();
        // Missing sections go back at about their default place.
        assert_eq!(layout.left, [Section::Layers, Section::Tools, Section::Quick, Section::Properties, Section::BrushPresets]);
        assert_eq!(layout.right, [Section::Navigator, Section::Color, Section::WetMedia, Section::Paper]);
        // A saved layout from before the Quick section gets it below Tools.
        let mut old: PanelLayout =
            toml::from_str("left = [\"Tools\", \"Properties\", \"BrushPresets\"]\nright = [\"Navigator\", \"Color\", \"WetMedia\", \"Layers\", \"Paper\"]")
                .unwrap();
        old.repair();
        assert_eq!(old.left, [Section::Tools, Section::Quick, Section::Properties, Section::BrushPresets]);
    }
}
