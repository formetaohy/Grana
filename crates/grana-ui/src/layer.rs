use std::collections::HashMap;

use glam::Vec2;

use crate::element::{CanvasInfo, CanvasPaint, Layer};
use crate::geometry::{Corners, Rect};
use crate::node::NodeId;
use crate::paint::{Clip, Instance, LayerInstances, Painter};
use crate::style::State;
use crate::theme::Role;
use crate::ui::Ui;
use crate::widget::PaintInfo;

const ORDER_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const ORDER_PRIME: u64 = 0x0000_0100_0000_01b3;

impl<A: Clone> Ui<A> {
    pub(crate) fn paint_layers<'a>(&mut self, canvases: &HashMap<NodeId, CanvasPaint<'a>>) {
        self.change.next_orders = [ORDER_OFFSET; Layer::ALL.len()];
        self.change.touched = [false; Layer::ALL.len()];
        for layer in &mut self.layers {
            layer.clear();
        }
        let root = self.tree.root;
        self.paint_node(root, canvases);
    }

    fn paint_node<'a>(&mut self, id: NodeId, canvases: &HashMap<NodeId, CanvasPaint<'a>>) {
        if !self.tree.nodes[id].visible {
            return;
        }
        let (layer, rect, clip, clip_corners, background, popup, enabled, hover, press, engaged) = {
            let node = &self.tree.nodes[id];
            (
                node.layer,
                node.rect,
                node.clip,
                node.clip_corners,
                node.background,
                node.popup_spec.is_some(),
                node.enabled,
                node.runtime.hover.value,
                node.runtime.press.value,
                node.runtime.engaged.value,
            )
        };
        let background =
            background.or_else(|| popup.then(|| self.theme.surface(Role::Popover, State::Rest)));
        let layer_index = layer.index();
        let order = self.change.next_orders[layer_index];
        self.change.next_orders[layer_index] =
            order.wrapping_mul(ORDER_PRIME) ^ self.tree.nodes[id].key.id();
        let mut instances = std::mem::take(&mut self.tree.nodes[id].paint);
        let dirty = self.tree.nodes[id].paint_dirty;
        if dirty {
            self.change.touched[layer_index] = true;
            instances.clear();
            let time = self.time;
            let screen_clip = Clip::new(Rect::from_min_size(Vec2::ZERO, self.size), Corners::ZERO);
            let node_clip = Clip::new(clip, clip_corners);
            let mut painter = Painter::new(
                &mut instances,
                &mut self.text,
                &mut self.images,
                if popup { screen_clip } else { node_clip },
                None,
            );
            if let Some(surface) = background
                && !rect.is_empty()
            {
                painter.surface(rect, surface);
            }
            if popup {
                painter.pop_clip(node_clip);
            }
            let preedit = if self.edit == Some(id) {
                self.preedit.as_deref()
            } else {
                None
            };
            let info = PaintInfo {
                theme: &self.theme,
                rect,
                hover,
                press,
                engaged,
                time,
                enabled,
                preedit,
            };
            let node = &mut self.tree.nodes[id];
            let caret = crate::widget::paint(&node.content, &node.runtime, &mut painter, &info);
            if self.edit == Some(id) {
                self.edit_caret = caret;
            }
            if let Some(paint) = canvases.get(&id) {
                let info = CanvasInfo {
                    rect,
                    theme: &self.theme,
                    hover,
                    press,
                    engaged,
                    time,
                };
                paint(&mut painter, info);
            }
            self.tree.nodes[id].paint_dirty = false;
        }
        self.layers[layer_index]
            .material
            .extend(instances.material.iter().copied());
        self.tree.nodes[id].paint = instances;
        for index in 0..self.tree.nodes[id].children.len() {
            let child = self.tree.nodes[id].children[index];
            self.paint_node(child, canvases);
        }
    }

    pub(crate) fn measure_layers(&mut self) {
        let mut changed = [false; Layer::ALL.len()];
        for (index, layer) in self.layers.iter().enumerate() {
            let reordered = self.change.orders[index] != self.change.next_orders[index];
            self.change.orders[index] = self.change.next_orders[index];
            let touched = self.change.restructured || self.change.touched[index] || reordered;
            if !touched && self.change.hashes[index].is_some() {
                continue;
            }
            let hash = hash_layer(layer, 0x9e37_79b9_7f4a_7c15 ^ index as u64);
            changed[index] = self.change.hashes[index] != Some(hash);
            self.change.hashes[index] = Some(hash);
        }
        self.change.restructured = false;
        let base = changed[Layer::Content.index()] || self.change.reset;
        let interface = changed[Layer::Chrome.index()]
            || changed[Layer::Popup.index()]
            || changed[Layer::Hud.index()]
            || base;
        self.change.base |= base;
        self.change.interface |= interface;
    }
}

fn hash_layer(layer: &LayerInstances, mut hash: u64) -> u64 {
    hash ^= layer.material.len() as u64;
    hash = hash.wrapping_mul(0x100_0000_01b3);
    for instance in &layer.material {
        for word in bytemuck::cast_slice::<Instance, u32>(std::slice::from_ref(instance)) {
            hash ^= u64::from(*word);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    hash
}
