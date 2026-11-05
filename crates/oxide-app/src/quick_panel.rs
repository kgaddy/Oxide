//! The Quick section: a short list of the controls you use most, chosen per
//! tool, so painting doesn't mean scrolling through every panel. Items can
//! come from Properties, Color and Wet Media.

use serde::{Deserialize, Serialize};

use crate::tools::Tool;

/// One control that can be put in the Quick section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuickItem {
    // Tool properties.
    Brush,
    Size,
    /// Opacity (Strength for Smudge).
    Opacity,
    Taper,
    /// Oils: the paint modes. Knife: Spread / Scrape.
    PaintMode,
    Loading,
    Length,
    Oiliness,
    Thickness,
    DirtyBrush,
    /// The mouse pressure slider.
    Pressure,
    HatchKind,
    HatchOptions,
    SelectOptions,
    PickerSample,
    // Color.
    Colors,
    Wheel,
    RgbSliders,
    Pigments,
    RecentColors,
    MultiColor,
    MixingPalette,
    // Wet paint.
    Dry,
    ShowWet,
}

/// The groups the items are offered in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickGroup {
    Tool,
    Color,
    WetPaint,
}

impl QuickGroup {
    pub const ALL: [QuickGroup; 3] = [QuickGroup::Tool, QuickGroup::Color, QuickGroup::WetPaint];

    pub fn label(self) -> &'static str {
        match self {
            QuickGroup::Tool => "Tool properties",
            QuickGroup::Color => "Color",
            QuickGroup::WetPaint => "Wet paint",
        }
    }
}

impl QuickItem {
    pub const ALL: [QuickItem; 24] = [
        QuickItem::Brush,
        QuickItem::Size,
        QuickItem::Opacity,
        QuickItem::Taper,
        QuickItem::PaintMode,
        QuickItem::Loading,
        QuickItem::Length,
        QuickItem::Oiliness,
        QuickItem::Thickness,
        QuickItem::DirtyBrush,
        QuickItem::Pressure,
        QuickItem::HatchKind,
        QuickItem::HatchOptions,
        QuickItem::SelectOptions,
        QuickItem::PickerSample,
        QuickItem::Colors,
        QuickItem::Wheel,
        QuickItem::RgbSliders,
        QuickItem::Pigments,
        QuickItem::RecentColors,
        QuickItem::MultiColor,
        QuickItem::MixingPalette,
        QuickItem::Dry,
        QuickItem::ShowWet,
    ];

    pub fn label(self, tool: Tool) -> &'static str {
        match self {
            QuickItem::Brush => "Brush",
            QuickItem::Size => "Size",
            QuickItem::Opacity if tool == Tool::Smudge => "Strength",
            QuickItem::Opacity => "Opacity",
            QuickItem::Taper => "Taper",
            QuickItem::PaintMode if tool == Tool::Knife => "Spread / Scrape",
            QuickItem::PaintMode => "Paint mode",
            QuickItem::Loading => "Loading",
            QuickItem::Length => "Length",
            QuickItem::Oiliness => "Oiliness",
            QuickItem::Thickness => "Thickness",
            QuickItem::DirtyBrush => "Dirty brush",
            QuickItem::Pressure => "Mouse pressure",
            QuickItem::HatchKind => "Hatch kind",
            QuickItem::HatchOptions => "Hatch settings",
            QuickItem::SelectOptions => "Selection options",
            QuickItem::PickerSample => "Sample size",
            QuickItem::Colors => "Current colors",
            QuickItem::Wheel => "Color wheel",
            QuickItem::RgbSliders => "RGB sliders",
            QuickItem::Pigments => "Pigment swatches",
            QuickItem::RecentColors => "Recent colors",
            QuickItem::MultiColor => "MultiColored brush",
            QuickItem::MixingPalette => "Mixing Palette button",
            QuickItem::Dry => "Dry & Rewet buttons",
            QuickItem::ShowWet => "Show wet paint",
        }
    }

    pub fn group(self) -> QuickGroup {
        use QuickItem::*;
        match self {
            Colors | Wheel | RgbSliders | Pigments | RecentColors | MultiColor | MixingPalette => QuickGroup::Color,
            Dry | ShowWet => QuickGroup::WetPaint,
            _ => QuickGroup::Tool,
        }
    }

    /// Whether this item means anything for `tool`.
    pub fn applies(self, tool: Tool) -> bool {
        use QuickItem::*;
        let brush = tool.paints() || matches!(tool, Tool::Eraser | Tool::Smudge);
        match self {
            Brush | Size | Opacity => brush,
            Taper => tool.paints() || tool == Tool::Eraser,
            PaintMode | Loading | Length | Oiliness | Thickness | DirtyBrush => tool.is_wet_medium(),
            Pressure => tool.paints() || tool == Tool::Smudge,
            HatchKind | HatchOptions => tool == Tool::Hatch,
            SelectOptions => tool == Tool::Select,
            PickerSample => tool == Tool::Picker,
            _ => true,
        }
    }

    /// What the Quick section holds for `tool` until you change it.
    pub fn defaults(tool: Tool) -> Vec<QuickItem> {
        use QuickItem::*;
        match tool {
            Tool::OilsAcrylics => vec![Brush, Size, PaintMode, Loading, Oiliness, Colors, Pigments, RecentColors, Dry],
            Tool::Knife => vec![Size, PaintMode, Loading, Oiliness, Colors, Pigments, Dry],
            Tool::Ink | Tool::Pencil | Tool::Marker | Tool::Airbrush => vec![Brush, Size, Opacity, Colors, Pigments, RecentColors],
            Tool::Hatch => vec![HatchKind, Size, Taper, HatchOptions, Colors],
            Tool::Eraser => vec![Brush, Size, Opacity],
            Tool::Smudge => vec![Brush, Size, Opacity, Dry],
            Tool::Select => vec![SelectOptions],
            Tool::Picker => vec![PickerSample, Colors, RecentColors],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_only_hold_items_that_apply() {
        for tool in Tool::ALL {
            let items = QuickItem::defaults(tool);
            assert!(!items.is_empty(), "{}", tool.label());
            for item in items {
                assert!(item.applies(tool), "{:?} for {}", item, tool.label());
            }
        }
    }

    #[test]
    fn all_lists_every_item_once() {
        for (i, a) in QuickItem::ALL.iter().enumerate() {
            assert!(!QuickItem::ALL[i + 1..].contains(a), "{a:?} twice");
        }
    }
}
