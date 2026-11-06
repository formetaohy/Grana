use std::collections::HashMap;

use slotmap::SlotMap;

use crate::element::{CanvasPaint, Content, Element, Interaction, Key, Layer};
use crate::node::{Node, NodeId};
use grana_glass::GlassId;

pub struct Tree<A> {
    pub nodes: SlotMap<NodeId, Node<A>>,
    pub root: NodeId,
    pub layout_dirty: bool,
    pub anchors: HashMap<Key, NodeId>,
    pub changed: bool,
}

impl<A> Tree<A> {
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node::new(
            Key::new("root"),
            0,
            crate::element::Layout::column(),
            Content::Empty,
            Interaction::None,
        ));
        Self {
            nodes,
            root,
            layout_dirty: true,
            anchors: HashMap::new(),
            changed: true,
        }
    }

    pub fn apply<'a>(
        &mut self,
        element: Element<'a, A>,
        canvases: &mut Vec<(NodeId, CanvasPaint<'a>)>,
    ) -> Vec<(NodeId, Option<GlassId>)> {
        let mut removed = Vec::new();
        self.reconcile_list(self.root, vec![element], canvases, &mut removed);
        self.anchors.clear();
        self.collect_anchors(self.root);
        removed
    }

    fn reconcile_list<'a>(
        &mut self,
        parent: NodeId,
        elements: Vec<Element<'a, A>>,
        canvases: &mut Vec<(NodeId, CanvasPaint<'a>)>,
        removed: &mut Vec<(NodeId, Option<GlassId>)>,
    ) {
        let old = self.nodes[parent].children.clone();
        let mut used: Vec<NodeId> = Vec::new();
        let mut next = Vec::with_capacity(elements.len());
        let parent_layer = self.nodes[parent].layer;
        for (index, element) in elements.into_iter().enumerate() {
            let key = if element.id == Key::NONE {
                Key::structural(index)
            } else {
                element.id
            };
            let kind = element.content.kind();
            let mut candidate = old.iter().copied().find(|id| {
                !used.contains(id) && self.nodes[*id].key == key && self.nodes[*id].kind == kind
            });
            if candidate.is_none() && element.id == Key::NONE {
                candidate = old.iter().copied().find(|id| {
                    !used.contains(id) && self.nodes[*id].kind == kind && !next.contains(id)
                });
            }
            let mut element = element;
            let children = std::mem::take(&mut element.children);
            let canvas = element.canvas.take();
            let node_id = match candidate {
                Some(id) => {
                    used.push(id);
                    let previous_layer = self.nodes[id].layer;
                    let layout_changed = element.patch(&mut self.nodes[id]);
                    if layout_changed {
                        self.layout_dirty = true;
                    }
                    if let Some(paint) = canvas {
                        canvases.push((id, paint));
                        self.nodes[id].has_canvas = true;
                        self.nodes[id].paint_dirty = true;
                    } else {
                        self.nodes[id].has_canvas = false;
                    }
                    let layer = self.nodes[id].layer;
                    if layer != previous_layer {
                        self.rebase(id, layer);
                    }
                    id
                }
                None => {
                    let mut node = element.into_node(key, parent_layer);
                    node.parent = Some(parent);
                    node.has_canvas = canvas.is_some();
                    let id = self.nodes.insert(node);
                    if let Some(paint) = canvas {
                        canvases.push((id, paint));
                    }
                    self.layout_dirty = true;
                    self.changed = true;
                    self.nodes[parent].layout_dirty = true;
                    id
                }
            };
            next.push(node_id);
            self.reconcile_list(node_id, children, canvases, removed);
        }
        for old in old {
            if !next.contains(&old) {
                self.remove(old, removed);
                self.layout_dirty = true;
                self.changed = true;
            }
        }
        self.nodes[parent].children = next;
    }

    fn remove(&mut self, id: NodeId, removed: &mut Vec<(NodeId, Option<GlassId>)>) {
        let children = self.nodes[id].children.clone();
        for child in children {
            self.remove(child, removed);
        }
        let glass = self.nodes[id].glass_id;
        removed.push((id, glass));
        self.nodes.remove(id);
    }

    fn rebase(&mut self, id: NodeId, layer: Layer) {
        if !self.nodes[id].layer_explicit {
            self.nodes[id].layer = layer;
            self.nodes[id].paint_dirty = true;
        }
        let children = self.nodes[id].children.clone();
        for child in children {
            self.rebase(child, layer);
        }
    }

    fn collect_anchors(&mut self, id: NodeId) {
        let (key, children) = {
            let node = &self.nodes[id];
            (node.key, node.children.clone())
        };
        if key != Key::NONE {
            self.anchors.insert(key, id);
        }
        for child in children {
            self.collect_anchors(child);
        }
    }

    pub fn node_at(&self, key: Key) -> Option<NodeId> {
        self.anchors.get(&key).copied()
    }

    pub fn mark_dirty(&mut self) {
        self.layout_dirty = true;
    }
}
