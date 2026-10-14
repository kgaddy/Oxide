//! Layer groups (one level): grouping, group opacity and visibility.

use super::*;

/// A layer group (one level; ⌥⌘G). Groups pass through
/// (Photoshop's default): their layers blend with everything below, and the
/// group's opacity fades the result as one picture.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerGroup {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    /// Shown open in the Layers panel.
    pub open: bool,
}

/// All groups plus which group each layer is in (for undo).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupState {
    pub(super) groups: Vec<LayerGroup>,
    pub(super) membership: Vec<Option<u32>>,
}

impl Document {
    pub fn groups(&self) -> &[LayerGroup] {
        &self.groups
    }

    pub fn group(&self, id: u32) -> Option<&LayerGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Is layer `i` shown (its eye, and its group's eye, are on)?
    pub fn layer_shown(&self, i: usize) -> bool {
        let l = &self.layers[i];
        l.visible && l.group.and_then(|id| self.group(id)).is_none_or(|g| g.visible)
    }

    /// How much layer `i` counts in the picture: its opacity times its
    /// group's (0 if hidden).
    pub fn layer_weight(&self, i: usize) -> f32 {
        if !self.layer_shown(i) {
            return 0.0;
        }
        let l = &self.layers[i];
        l.opacity * l.group.and_then(|id| self.group(id)).map_or(1.0, |g| g.opacity)
    }

    /// Put the current layer in a new group (⌥⌘G). Returns the group's id,
    /// or None if the layer is already in a group.
    pub fn group_active_layer(&mut self) -> Option<u32> {
        if self.layers[self.active].group.is_some() {
            return None;
        }
        let id = self.next_group_id;
        let used: Vec<String> = self.groups.iter().map(|g| g.name.clone()).collect();
        let name = (1..).map(|n| format!("Group {n}")).find(|n| !used.contains(n)).unwrap_or_default();
        let active = self.active;
        self.change_groups("Group layer", |doc| {
            doc.groups.push(LayerGroup { id, name, visible: true, opacity: 1.0, open: true });
            doc.layers[active].group = Some(id);
        });
        self.next_group_id += 1;
        Some(id)
    }

    /// Dissolve a group; its layers stay where they are.
    pub fn ungroup(&mut self, id: u32) {
        self.change_groups("Ungroup", |doc| {
            doc.groups.retain(|g| g.id != id);
            for l in &mut doc.layers {
                if l.group == Some(id) {
                    l.group = None;
                }
            }
        });
    }

    pub fn set_group_visible(&mut self, id: u32, visible: bool) {
        self.change_groups("Show/hide group", |doc| {
            if let Some(g) = doc.group_mut(id) {
                g.visible = visible;
            }
        });
    }

    /// Group opacity (0..=1); slider drags merge into one undo step.
    pub fn set_group_opacity(&mut self, id: u32, opacity: f32) {
        let opacity = opacity.clamp(0.0, 1.0);
        let before = self.group_state();
        if let Some(g) = self.group_mut(id) {
            g.opacity = opacity;
        }
        if self.group_state() != before {
            self.all_dirty = true;
            self.history.record(Edit::Groups { other: before, label: "Group opacity" });
        }
    }

    pub fn rename_group(&mut self, id: u32, name: String) {
        if self.group(id).is_some_and(|g| g.name != name) {
            self.change_groups("Rename group", |doc| {
                if let Some(g) = doc.group_mut(id) {
                    g.name = name;
                }
            });
        }
    }

    /// Open or close a group in the Layers panel (not an undo step).
    pub fn set_group_open(&mut self, id: u32, open: bool) {
        if let Some(g) = self.group_mut(id) {
            g.open = open;
        }
    }

    fn group_mut(&mut self, id: u32) -> Option<&mut LayerGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub(super) fn group_state(&self) -> GroupState {
        GroupState { groups: self.groups.clone(), membership: self.layers.iter().map(|l| l.group).collect() }
    }

    /// Make a change to groups/membership as one undo step (if anything changed).
    pub(super) fn change_groups(&mut self, label: &'static str, change: impl FnOnce(&mut Self)) {
        let before = self.group_state();
        change(self);
        if self.group_state() != before {
            self.all_dirty = true;
            self.history.seal();
            self.history.record(Edit::Groups { other: before, label });
            self.history.seal();
        }
    }

    /// Restore groups from a file (ids as saved).
    pub(crate) fn set_groups(&mut self, groups: Vec<LayerGroup>) {
        self.next_group_id = groups.iter().map(|g| g.id + 1).max().unwrap_or(1);
        self.groups = groups;
    }
}
