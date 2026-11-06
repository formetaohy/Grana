use glam::Vec2;
use grana_glass::{Glass as LayerGlass, Material, Shape, Spring};

use crate::element::Popup;
use crate::geometry::{Corners, Rect};
use crate::motion::Motion;
use crate::node::NodeId;
use crate::ui::{PopupTransition, Ui, glass_rect};

impl<A: Clone> Ui<A> {
    pub(crate) fn collect_popup_windows(&mut self) {
        self.popup_windows.clear();
        for (&node, transition) in &self.popup_glass {
            let Some(frame) = self.scene.current_frame(transition.glass) else {
                continue;
            };
            let window = Rect::from_min_size(
                Vec2::new(frame.x, frame.y),
                Vec2::new(frame.width, frame.height),
            );
            let corners = self
                .scene
                .current_corner(transition.glass)
                .map_or(Corners::ZERO, |(radius, _)| Corners::all(radius.max(0.0)));
            self.popup_windows.insert(node, (window, corners));
        }
        let morphing = self.popup_windows.iter().any(|(node, (window, _))| {
            let body = self.tree.nodes[*node].rect;
            let offset = (body.min - window.min).abs().max_element();
            let size = (body.size() - window.size()).abs().max_element();
            offset.max(size) > 0.25
        });
        if morphing {
            self.tree.layout_dirty = true;
        }
    }

    pub(crate) fn retire_popup_anchors(&mut self) {
        let scene = &mut self.scene;
        self.popup_retire.retain(|(anchor, popover)| {
            if scene.glass(*popover).is_some() {
                true
            } else {
                scene.remove(*anchor);
                false
            }
        });
    }

    pub(crate) fn ensure_popups(&mut self) -> bool {
        let mut opened = false;
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            let Some(spec) = self.tree.nodes[id].popup_spec else {
                continue;
            };
            let open = self.tree.nodes[id].runtime.open;
            let exists = self.glass_nodes.contains_key(&id);
            if open && !exists {
                if self.open_popup(id, spec) {
                    opened = true;
                }
            } else if !open && exists {
                self.close_popup(id);
            }
        }
        opened
    }

    fn open_popup(&mut self, id: NodeId, spec: Popup) -> bool {
        let node = &self.tree.nodes[id];
        if !node.visible || node.rect.is_empty() {
            return false;
        }
        let Some(glass_spec) = node.glass_spec else {
            return false;
        };
        let style = glass_spec.style;
        let body = node.rect;
        let anchor = self
            .tree
            .node_at(spec.anchor)
            .map_or(body, |anchor| self.tree.nodes[anchor].rect);
        let floating = *self
            .floating
            .get_or_insert_with(|| self.scene.add_container(16.0, style.layer));
        let ghost = self.scene.add(
            LayerGlass::new(glass_rect(anchor))
                .shape(Shape::Capsule)
                .material(Material::regular())
                .z(style.layer)
                .container(floating),
        );
        self.scene.snap_presence(ghost, 1.0);
        let mut glass = LayerGlass::new(glass_rect(body))
            .shape(style.shape)
            .material(style.material)
            .z(style.layer)
            .container(floating);
        if style.interactive {
            glass = glass.interactive();
        }
        if style.lens {
            glass = glass.lens();
        }
        let glass = self
            .scene
            .expand_from(ghost, glass, Spring::new(0.45, 0.22));
        self.glass_nodes.insert(id, glass);
        self.tree.nodes[id].glass_style = Some(style);
        self.popup_glass.insert(
            id,
            PopupTransition {
                glass,
                anchor: ghost,
                body,
            },
        );
        true
    }

    fn close_popup(&mut self, id: NodeId) {
        self.tree.nodes[id].glass_style = None;
        let Some(transition) = self.popup_glass.remove(&id) else {
            if let Some(glass) = self.glass_nodes.remove(&id) {
                self.scene.remove(glass);
            }
            return;
        };
        self.scene
            .collapse_into(transition.glass, transition.anchor, Spring::new(0.35, 0.0));
        self.scene
            .set_material(transition.anchor, Material::regular());
        self.scene.snap_presence(transition.anchor, 1.0);
        self.glass_nodes.remove(&id);
        self.popup_retire
            .push((transition.anchor, transition.glass));
    }

    pub(crate) fn sync_glass(&mut self) {
        let mut seen: Vec<NodeId> = Vec::new();
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            if self.tree.nodes[id].popup_spec.is_some() {
                continue;
            }
            let (spec, rect, clip, visible, active, previous) = {
                let node = &self.tree.nodes[id];
                (
                    node.glass_spec,
                    node.rect,
                    node.clip,
                    node.visible,
                    self.press == Some(id) || self.drag == Some(id),
                    node.glass_style,
                )
            };
            let Some(spec) = spec else {
                continue;
            };
            if !visible || rect.is_empty() {
                continue;
            }
            seen.push(id);
            let container = spec.group.map(|group| {
                let layer = spec.style.layer;
                *self
                    .glass_groups
                    .entry(group)
                    .or_insert_with(|| self.scene.add_container(0.0, layer))
            });
            let frame = glass_rect(rect);
            let bounds = glass_rect(clip);
            match self.glass_nodes.get(&id).copied() {
                Some(glass) => {
                    if previous != Some(spec.style) {
                        let fade = self.motion(Motion::response(0.4, 0.0)).spring();
                        self.scene.set_shape(glass, spec.style.shape);
                        self.scene
                            .set_material_with(glass, spec.style.material, fade);
                        self.scene
                            .set_behavior(glass, spec.style.interactive, spec.style.lens);
                        self.tree.nodes[id].glass_style = Some(spec.style);
                    }
                    self.scene.place(glass, frame);
                    self.scene.set_clip(glass, Some(bounds));
                    let engaged = active && self.inside;
                    self.scene
                        .set_engaged(glass, engaged, engaged.then_some(self.pointer));
                }
                None => {
                    let mut glass = LayerGlass::new(frame)
                        .shape(spec.style.shape)
                        .material(spec.style.material)
                        .z(spec.style.layer)
                        .clip(bounds);
                    if let Some(container) = container {
                        glass = glass.container(container);
                    }
                    if spec.style.interactive {
                        glass = glass.interactive();
                    }
                    if spec.style.lens {
                        glass = glass.lens();
                    }
                    let glass = self.scene.add(glass);
                    self.glass_nodes.insert(id, glass);
                    self.tree.nodes[id].glass_style = Some(spec.style);
                }
            }
        }
        self.sync_popup_glass(&mut seen);
        for id in self.glass_nodes.keys().copied().collect::<Vec<_>>() {
            if !seen.contains(&id)
                && let Some(glass) = self.glass_nodes.remove(&id)
            {
                self.scene.remove(glass);
            }
        }
        self.sync_sub_glass();
        self.occluders = self
            .glass_nodes
            .keys()
            .filter_map(|id| {
                let node = &self.tree.nodes[*id];
                (node.visible && !node.rect.is_empty()).then_some(node.rect)
            })
            .collect();
    }

    fn sync_popup_glass(&mut self, seen: &mut Vec<NodeId>) {
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            let Some(&glass) = self.glass_nodes.get(&id) else {
                continue;
            };
            let (visible, open, rect, previous, spec, anchor) = {
                let node = &self.tree.nodes[id];
                (
                    node.visible,
                    node.runtime.open,
                    node.rect,
                    node.glass_style,
                    node.glass_spec,
                    node.popup_spec
                        .and_then(|spec| self.tree.node_at(spec.anchor))
                        .map(|anchor| self.tree.nodes[anchor].rect),
                )
            };
            let Some(spec) = spec else {
                continue;
            };
            if !visible || !open {
                continue;
            }
            seen.push(id);
            if previous != Some(spec.style) {
                let fade = self.motion(Motion::response(0.4, 0.0)).spring();
                self.scene.set_shape(glass, spec.style.shape);
                self.scene
                    .set_material_with(glass, spec.style.material, fade);
                self.scene
                    .set_behavior(glass, spec.style.interactive, spec.style.lens);
                self.tree.nodes[id].glass_style = Some(spec.style);
            }
            if let Some(transition) = self.popup_glass.get_mut(&id) {
                if transition.body != rect {
                    transition.body = rect;
                    self.scene
                        .set_frame_with(glass, glass_rect(rect), Spring::new(0.3, 0.0));
                }
                if let Some(anchor) = anchor {
                    self.scene.place(transition.anchor, glass_rect(anchor));
                }
                let material = if self
                    .scene
                    .current_frame(glass)
                    .is_some_and(|frame| expansion(frame, rect) > 0.92)
                {
                    Material::identity()
                } else {
                    Material::regular()
                };
                self.scene.set_material(transition.anchor, material);
            }
        }
    }

    fn sync_sub_glass(&mut self) {
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
            let visible = self.tree.nodes[id].visible;
            let clip = self.tree.nodes[id].clip;
            let items = std::mem::take(&mut self.tree.nodes[id].sub_glass);
            if !visible || items.is_empty() {
                if let Some(ids) = self.sub_glass.remove(&id) {
                    for glass in ids {
                        self.scene.remove(glass);
                    }
                }
                self.tree.nodes[id].sub_glass = items;
                continue;
            }
            let ids = self.sub_glass.entry(id).or_default();
            while ids.len() > items.len() {
                if let Some(glass) = ids.pop() {
                    self.scene.remove(glass);
                }
            }
            while ids.len() < items.len() {
                let item = items[ids.len()];
                let mut glass = LayerGlass::new(glass_rect(item.rect))
                    .shape(item.style.shape)
                    .material(item.style.material)
                    .z(item.style.layer)
                    .clip(glass_rect(clip));
                if item.style.interactive {
                    glass = glass.interactive();
                }
                if item.style.lens {
                    glass = glass.lens();
                }
                let glass = self.scene.add(glass);
                ids.push(glass);
            }
            let engaged_pointer = self.inside.then_some(self.pointer);
            for (glass, item) in ids.iter().zip(items.iter()) {
                self.scene.place(*glass, glass_rect(item.rect));
                self.scene.set_clip(*glass, Some(glass_rect(clip)));
                let engaged = item.engaged && self.inside;
                self.scene.set_engaged(
                    *glass,
                    engaged,
                    if engaged { engaged_pointer } else { None },
                );
            }
            self.tree.nodes[id].sub_glass = items;
        }
        for id in self.sub_glass.keys().copied().collect::<Vec<_>>() {
            if !self.tree.ids.contains(&id)
                && let Some(ids) = self.sub_glass.remove(&id)
            {
                for glass in ids {
                    self.scene.remove(glass);
                }
            }
        }
    }
}

fn expansion(frame: grana_glass::Rect, body: Rect) -> f32 {
    let width = frame.width / body.width().max(1.0);
    let height = frame.height / body.height().max(1.0);
    width.min(height).clamp(0.0, 1.0)
}
