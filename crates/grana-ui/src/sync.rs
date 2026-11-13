use std::collections::HashSet;

use crate::element::{Content, Layer};
use crate::node::NodeId;
use crate::ui::Ui;
use crate::widget::FrameAnimation;

impl<A: Clone> Ui<A> {
    pub(crate) fn animate_nodes(&mut self) {
        let elapsed = self.elapsed;
        let mut animation = std::mem::take(&mut self.animation);
        animation.append(&mut self.tree.activated);
        let mut next = Vec::with_capacity(animation.len());
        for id in animation {
            if !self.tree.nodes.contains_key(id) {
                continue;
            }
            if !self.tree.nodes[id].visible {
                self.tree.nodes[id].runtime.active = false;
                continue;
            }
            let frame = FrameAnimation {
                hovered: self.hover == Some(id),
                pressed: self.press == Some(id) || self.drag == Some(id),
                dragging: self.drag == Some(id),
                pointer: self.pointer,
                elapsed,
            };
            let mut animating =
                crate::widget::animate(&mut self.tree.nodes[id], &frame, &mut self.text);
            if self.edit == Some(id) {
                self.tree.nodes[id].paint_dirty = true;
                animating = true;
            }
            self.tree.nodes[id].runtime.active = animating;
            if animating {
                next.push(id);
            }
            self.animating |= animating;
        }
        self.animation = next;
    }

    pub(crate) fn sync_fields(&mut self) {
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        for id in ids {
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
        let root = self.tree.root;
        crate::hit::collect(&self.tree, root, &mut hits, &mut self.text, &self.theme);
        self.hits = hits;
        self.occluders.clear();
        for node in self.tree.nodes.values() {
            if !node.visible || node.rect.is_empty() || node.layer == Layer::Content {
                continue;
            }
            let surface_visible = node
                .background
                .is_some_and(|surface| surface.fill.is_visible() || surface.backdrop > 0.0);
            let popup_open = node.popup_spec.is_some() && node.runtime.open;
            if surface_visible || popup_open || node.blocks_pointer {
                self.occluders.push(node.rect);
            }
        }
    }

    pub(crate) fn rebuild_popup_anchors(&mut self) {
        self.popup_anchors.clear();
        for (id, node) in self.tree.nodes.iter() {
            if let Some(spec) = node.popup_spec
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
        if self.scrollbar.is_some_and(|(id, _, _)| gone.contains(&id)) {
            self.scrollbar = None;
        }
        if self.hovered_since.is_some_and(|(id, _)| gone.contains(&id)) {
            self.hovered_since = None;
            self.tooltip = None;
        }
    }
}
