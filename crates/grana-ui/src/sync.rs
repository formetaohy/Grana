use std::collections::HashSet;

use crate::dispatch::Finish;
use crate::element::{Content, Layer};
use crate::node::NodeId;
use crate::ui::Ui;
use crate::widget::FrameAnimation;

impl<A: Clone> Ui<A> {
    pub(crate) fn sync_hover(&mut self) {
        let mut chain = HashSet::new();
        let mut cursor = self.hover;
        while let Some(id) = cursor {
            let Some(node) = self.tree.nodes.get(id) else {
                break;
            };
            chain.insert(id);
            cursor = node.parent;
        }
        let mut fired = Vec::new();
        for (id, node) in self.tree.nodes.iter_mut() {
            let within = chain.contains(&id);
            if within == node.runtime.within_hover {
                continue;
            }
            node.runtime.within_hover = within;
            if let Some(handler) = &node.hover {
                fired.push(handler(within));
            }
        }
        self.actions.extend(fired);
    }

    pub(crate) fn animate_nodes(&mut self) {
        let elapsed = self.elapsed;
        let mut hovered_chain = Vec::new();
        let mut ancestor = self.hover;
        while let Some(id) = ancestor {
            if !self.tree.nodes.contains_key(id) {
                break;
            }
            hovered_chain.push(id);
            ancestor = self.tree.nodes[id].parent;
        }
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
                within_hover: hovered_chain.contains(&id),
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
                    assert!(
                        self.edit == Some(id),
                        "an editing field owns the edit session"
                    );
                    self.finish_edit(Finish::Write);
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
                assert!(
                    self.edit == Some(id),
                    "an editing field owns the edit session"
                );
                self.finish_edit(Finish::Write);
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
            if node.layer == Layer::Hud && !node.blocks_pointer {
                continue;
            }
            let surface_visible = node
                .background
                .is_some_and(|surface| surface.fill.is_visible());
            let popup_open = node.popup_spec.is_some() && node.runtime.open;
            if surface_visible || popup_open || node.blocks_pointer {
                self.occluders.push(node.rect);
            }
        }
    }

    pub(crate) fn rebuild_modal(&mut self) {
        let mut stack = vec![self.tree.root];
        let mut held: Option<(usize, NodeId)> = None;
        while let Some(id) = stack.pop() {
            let node = &self.tree.nodes[id];
            if node.modal && node.visible {
                let layer = node.layer.index();
                if held.is_none_or(|(best, _)| layer >= best) {
                    held = Some((layer, id));
                }
            }
            for child in node.children.iter().rev() {
                stack.push(*child);
            }
        }
        let next = held.map(|(_, id)| id);
        if next != self.modal {
            self.modal = next;
            self.tree.focus = true;
        }
        if self.focus.is_some_and(|focus| !self.inside_modal(focus)) {
            self.focus = None;
            self.focus_visible = false;
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
        cleared(&mut self.modal);
        if self.scrollbar.is_some_and(|(id, _, _)| gone.contains(&id)) {
            self.scrollbar = None;
        }
        if self.hovered_since.is_some_and(|(id, _)| gone.contains(&id)) {
            self.hovered_since = None;
            self.tooltip = None;
        }
        self.tree.scrolls.retain(|id| !gone.contains(id));
        self.scrolling.retain(|id| !gone.contains(id));
    }
}
