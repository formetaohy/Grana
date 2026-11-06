use std::collections::{HashMap, HashSet};

use slotmap::SlotMap;

use crate::element::{CanvasPaint, ContentKind, Element, Key, Layer};
use crate::node::{Node, NodeId};
use grana_glass::GlassId;

pub struct Tree<A> {
    pub nodes: SlotMap<NodeId, Node<A>>,
    pub ids: Vec<NodeId>,
    pub root: NodeId,
    pub layout_dirty: bool,
    pub anchors: HashMap<Key, NodeId>,
}

impl<A> Tree<A> {
    pub fn new() -> Self {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node::new(
            Key::new("root"),
            ContentKind::Empty,
            crate::element::Layout::column(),
            crate::element::Content::Empty,
            crate::element::Interaction::None,
        ));
        Self {
            nodes,
            ids: vec![root],
            root,
            layout_dirty: true,
            anchors: HashMap::new(),
        }
    }

    pub fn apply<'a>(
        &mut self,
        element: Element<'a, A>,
        canvases: &mut HashMap<NodeId, CanvasPaint<'a>>,
    ) -> Vec<(NodeId, Option<GlassId>)> {
        let mut removed = Vec::new();
        self.anchors.clear();
        self.reconcile_list(self.root, vec![element], canvases, &mut removed);
        self.ids.retain(|id| self.nodes.contains_key(*id));
        removed
    }

    fn reconcile_list<'a>(
        &mut self,
        parent: NodeId,
        elements: Vec<Element<'a, A>>,
        canvases: &mut HashMap<NodeId, CanvasPaint<'a>>,
        removed: &mut Vec<(NodeId, Option<GlassId>)>,
    ) {
        let old = std::mem::take(&mut self.nodes[parent].children);
        let parent_layer = self.nodes[parent].layer;
        let mut keyed: HashMap<Key, NodeId> = HashMap::with_capacity(old.len());
        for id in &old {
            keyed.entry(self.nodes[*id].key).or_insert(*id);
        }
        let mut used: HashSet<NodeId> = HashSet::with_capacity(old.len());
        let mut next = Vec::with_capacity(elements.len());
        for (index, mut element) in elements.into_iter().enumerate() {
            let named = element.id != Key::NONE;
            let key = if named {
                element.id
            } else {
                Key::structural(index)
            };
            let kind = element.content.kind();
            let children = std::mem::take(&mut element.children);
            let canvas = element.canvas.take();
            let reuse = keyed
                .get(&key)
                .copied()
                .filter(|id| !used.contains(id) && self.nodes[*id].kind == kind);
            let id = match reuse {
                Some(id) => {
                    used.insert(id);
                    let previous_layer = self.nodes[id].layer;
                    if element.patch(&mut self.nodes[id]) {
                        self.layout_dirty = true;
                    }
                    if canvas.is_some() {
                        self.nodes[id].paint_dirty = true;
                    }
                    if let Some(paint) = canvas {
                        canvases.insert(id, paint);
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
                    let id = self.nodes.insert(node);
                    self.ids.push(id);
                    if let Some(paint) = canvas {
                        canvases.insert(id, paint);
                    }
                    self.layout_dirty = true;
                    id
                }
            };
            if named {
                self.bind(key, id);
            }
            next.push(id);
            self.reconcile_list(id, children, canvases, removed);
        }
        for id in old {
            if !next.contains(&id) {
                self.remove(id, removed);
                self.layout_dirty = true;
            }
        }
        self.nodes[parent].children = next;
    }

    fn bind(&mut self, key: Key, id: NodeId) {
        if let Some(existing) = self.anchors.insert(key, id)
            && existing != id
        {
            panic!("duplicate retained key {:#x}", key.id());
        }
    }

    fn remove(&mut self, id: NodeId, removed: &mut Vec<(NodeId, Option<GlassId>)>) {
        for index in 0..self.nodes[id].children.len() {
            let child = self.nodes[id].children[index];
            self.remove(child, removed);
        }
        removed.push((id, self.nodes[id].glass_id));
        self.nodes.remove(id);
    }

    fn rebase(&mut self, id: NodeId, layer: Layer) {
        if !self.nodes[id].layer_explicit {
            self.nodes[id].layer = layer;
            self.nodes[id].paint_dirty = true;
        }
        for index in 0..self.nodes[id].children.len() {
            let child = self.nodes[id].children[index];
            self.rebase(child, layer);
        }
    }

    pub fn node_at(&self, key: Key) -> Option<NodeId> {
        self.anchors.get(&key).copied()
    }

    pub fn mark_dirty(&mut self) {
        self.layout_dirty = true;
    }
}
