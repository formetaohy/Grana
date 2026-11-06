use grana_glass::Glass as LayerGlass;

use crate::motion::Motion;
use crate::node::NodeId;
use crate::ui::{Ui, glass_rect};

impl<A: Clone> Ui<A> {
    pub(crate) fn sync_glass(&mut self) {
        let mut seen: Vec<NodeId> = Vec::new();
        for index in 0..self.tree.ids.len() {
            let id = self.tree.ids[index];
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
