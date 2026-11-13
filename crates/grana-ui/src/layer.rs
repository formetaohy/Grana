use glam::Vec2;
use slotmap::SecondaryMap;

use crate::element::{CanvasKind, CanvasPaint, Layer};
use crate::geometry::{Corners, Rect};
use crate::node::NodeId;
use crate::paint::{Clip, Instance, Instances, Painter};
use crate::style::State;
use crate::theme::Role;
use crate::ui::Ui;
use crate::widget::PaintInfo;

const CULL_MARGIN: f32 = 64.0;

impl<A: Clone> Ui<A> {
    pub(crate) fn paint_layers<'a>(&mut self, canvases: &SecondaryMap<NodeId, CanvasPaint<'a>>) {
        if std::mem::take(&mut self.tree.structure) {
            for layer in &mut self.layers {
                layer.rebuild = true;
            }
        }
        let root = self.tree.root;
        self.paint_node(root, canvases);
        for index in 0..Layer::ALL.len() {
            if !self.layers[index].rebuild {
                continue;
            }
            let mut material = std::mem::take(&mut self.layers[index].material);
            material.clear();
            self.collect_layer(root, index, &mut material);
            let layer = &mut self.layers[index];
            layer.material = material;
            layer.rebuild = false;
            layer.dirty = Some((0, layer.material.len()));
            layer.touch();
        }
    }

    fn paint_node<'a>(&mut self, id: NodeId, canvases: &SecondaryMap<NodeId, CanvasPaint<'a>>) {
        if !self.tree.nodes[id].visible {
            let layer = self.tree.nodes[id].layer.index();
            if self.tree.nodes[id].painted {
                self.tree.nodes[id].painted = false;
                self.layers[layer].rebuild = true;
            }
            return;
        }
        let culled = self.tree.nodes[id].popup_spec.is_none()
            && !self.tree.nodes[id]
                .rect
                .expand(CULL_MARGIN)
                .intersects(self.tree.nodes[id].clip);
        let repaint = !culled
            && (self.tree.nodes[id].paint_dirty || self.tree.nodes[id].canvas == CanvasKind::Frame);
        if repaint {
            self.repaint(id, canvases);
        }
        #[cfg(debug_assertions)]
        if !repaint
            && !culled
            && !self.tree.nodes[id].paint_dirty
            && !matches!(
                self.tree.nodes[id].canvas,
                CanvasKind::Frame | CanvasKind::None
            )
        {
            self.verify_canvas(id, canvases);
        }
        let count = self.tree.nodes[id].paint.len();
        let included = !culled && count > 0;
        let layer = self.tree.nodes[id].layer.index();
        if included != self.tree.nodes[id].painted {
            self.tree.nodes[id].painted = included;
            self.layers[layer].rebuild = true;
        }
        if included
            && !self.layers[layer].rebuild
            && self.tree.nodes[id].paint_version != self.tree.nodes[id].layer_version
        {
            let (start, end) = self.tree.nodes[id].layer_range;
            if (end - start) as usize == count {
                let material = std::mem::take(&mut self.tree.nodes[id].paint.material);
                self.layers[layer].patch(start as usize, &material);
                self.tree.nodes[id].paint.material = material;
                self.tree.nodes[id].layer_version = self.tree.nodes[id].paint_version;
            } else {
                self.layers[layer].rebuild = true;
            }
        }
        for index in 0..self.tree.nodes[id].children.len() {
            let child = self.tree.nodes[id].children[index];
            self.paint_node(child, canvases);
        }
    }

    fn repaint<'a>(&mut self, id: NodeId, canvases: &SecondaryMap<NodeId, CanvasPaint<'a>>) {
        let previous = std::mem::take(&mut self.tree.nodes[id].paint);
        let mut fresh = std::mem::take(&mut self.scratch);
        self.paint_into(id, canvases, &mut fresh);
        if previous.material == fresh.material {
            self.tree.nodes[id].paint = previous;
            self.scratch = fresh;
        } else {
            self.tree.nodes[id].paint = fresh;
            self.tree.nodes[id].paint_version = self.tree.nodes[id].paint_version.wrapping_add(1);
            self.scratch = previous;
        }
        self.tree.nodes[id].paint_dirty = false;
    }

    #[cfg(debug_assertions)]
    fn verify_canvas<'a>(&mut self, id: NodeId, canvases: &SecondaryMap<NodeId, CanvasPaint<'a>>) {
        if !canvases.contains_key(id) {
            return;
        }
        let mut instances = std::mem::take(&mut self.scratch);
        self.paint_into(id, canvases, &mut instances);
        assert!(
            instances.material == self.tree.nodes[id].paint.material,
            "the cached canvas {:#x} of node {:#x} changed without new props",
            match self.tree.nodes[id].canvas {
                CanvasKind::Props(hash) => hash,
                _ => 0,
            },
            self.tree.nodes[id].key.id()
        );
        self.scratch = instances;
    }

    fn paint_into<'a>(
        &mut self,
        id: NodeId,
        canvases: &SecondaryMap<NodeId, CanvasPaint<'a>>,
        instances: &mut Instances,
    ) {
        let (rect, clip, clip_corners, background, popup, enabled, hover, press, engaged) = {
            let node = &self.tree.nodes[id];
            (
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
            background.or_else(|| popup.then(|| self.theme.surface(Role::Menu, State::Rest)));
        instances.clear();
        let time = self.time;
        let screen_clip = Clip::new(Rect::from_min_size(Vec2::ZERO, self.size), Corners::ZERO);
        let node_clip = Clip::new(clip, clip_corners);
        {
            let mut painter = Painter::new(
                instances,
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
            if canvases.contains_key(id)
                && let Some(paint) = canvases.get(id)
            {
                let info = crate::element::CanvasInfo {
                    rect,
                    theme: &self.theme,
                    hover,
                    press,
                    engaged,
                    time,
                };
                paint(&mut painter, info);
            }
        }
    }

    fn collect_layer(&mut self, id: NodeId, layer: usize, material: &mut Vec<Instance>) {
        if !self.tree.nodes[id].visible {
            return;
        }
        if self.tree.nodes[id].layer.index() == layer {
            let painted = self.tree.nodes[id].painted && !self.tree.nodes[id].paint.is_empty();
            if painted {
                let start = material.len() as u32;
                material.extend_from_slice(&self.tree.nodes[id].paint.material);
                let end = material.len() as u32;
                self.tree.nodes[id].layer_range = (start, end);
                self.tree.nodes[id].layer_version = self.tree.nodes[id].paint_version;
            } else {
                self.tree.nodes[id].layer_range = (0, 0);
            }
        }
        for index in 0..self.tree.nodes[id].children.len() {
            let child = self.tree.nodes[id].children[index];
            self.collect_layer(child, layer, material);
        }
    }
}
