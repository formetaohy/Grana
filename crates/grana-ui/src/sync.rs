use std::collections::HashSet;

use crate::element::{Content, Layer};
use crate::node::NodeId;
use crate::ui::Ui;
use crate::widget::FrameAnimation;

impl<A: Clone> Ui<A> {
    pub(crate) fn animate_nodes(&mut self) {
        let elapsed = self.elapsed;
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            if !self.tree.nodes[id].visible {
                self.tree.nodes[id].runtime.commit = None;
                continue;
            }
            let frame = FrameAnimation {
                hovered: self.hover == Some(id),
                pressed: self.press == Some(id) || self.drag == Some(id),
                dragging: self.drag == Some(id),
                pointer: self.pointer,
                elapsed,
            };
            let animating =
                crate::widget::animate(&mut self.tree.nodes[id], &frame, &mut self.text);
            if self.edit == Some(id) {
                self.tree.nodes[id].paint_dirty = true;
                self.animating = true;
            }
            self.animating |= animating;
        }
    }

    pub(crate) fn sync_fields(&mut self) {
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            let visible = self.tree.nodes[id].visible;
            let active = self.tree.nodes[id].runtime.edit.is_some();
            if !visible {
                if active {
                    self.tree.nodes[id].runtime.edit = None;
                    if self.edit == Some(id) {
                        self.edit = None;
                    }
                }
                continue;
            }
            let editing = match &self.tree.nodes[id].content {
                Content::Field(field) => field.editing,
                _ => false,
            };
            let previous = self.tree.nodes[id].runtime.edit_prop;
            self.tree.nodes[id].runtime.edit_prop = editing;
            if editing && !active {
                self.begin_edit(id, false);
            } else if !editing && previous && active {
                self.tree.nodes[id].runtime.edit = None;
                if self.edit == Some(id) {
                    self.edit = None;
                }
            }
        }
    }

    pub(crate) fn rebuild_hits(&mut self) {
        let mut hits = std::mem::take(&mut self.hits);
        hits.clear();
        crate::hit::collect(
            &self.tree,
            self.tree.root,
            &mut hits,
            &mut self.text,
            &self.theme,
        );
        self.hits = hits;
        self.occluders.clear();
        for &id in &self.tree.ids {
            let node = &self.tree.nodes[id];
            if !node.visible || node.rect.is_empty() || node.layer == Layer::Content {
                continue;
            }
            let surface_visible = node
                .background
                .is_some_and(|surface| surface.fill.is_visible() || surface.backdrop > 0.0);
            let popup_open = node.popup_spec.is_some() && node.runtime.open;
            if surface_visible || popup_open {
                self.occluders.push(node.rect);
            }
        }
    }

    pub(crate) fn rebuild_popup_anchors(&mut self) {
        self.popup_anchors.clear();
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            if let Some(spec) = self.tree.nodes[id].popup_spec
                && let Some(previous) = self.popup_anchors.insert(spec.anchor, id)
                && previous != id
            {
                panic!("duplicate popup anchor {:#x}", spec.anchor.id());
            }
        }
    }

    pub(crate) fn prune(&mut self, removed: &[NodeId]) {
        if removed.is_empty() {
            return;
        }
        let gone = removed.iter().copied().collect::<HashSet<_>>();
        let cleared = |value: &mut Option<NodeId>| {
            if value.is_some_and(|id| gone.contains(&id)) {
                *value = None;
            }
        };
        cleared(&mut self.hover);
        cleared(&mut self.press);
        cleared(&mut self.drag);
        cleared(&mut self.focus);
        cleared(&mut self.edit);
        if self.hovered_since.is_some_and(|(id, _)| gone.contains(&id)) {
            self.hovered_since = None;
            self.tooltip = None;
        }
    }
}
