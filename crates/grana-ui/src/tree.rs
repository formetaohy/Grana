use std::collections::{HashMap, HashSet};

use slotmap::SlotMap;

use crate::element::{
    CanvasKind, CanvasPaint, Content, ContentKind, Dirty, Element, Interaction, Key, Layer,
};
use crate::node::{Node, NodeId};

pub struct Reconciled<A> {
    pub removed: Vec<NodeId>,
    pub committed: Vec<A>,
}

pub struct Tree<A> {
    pub nodes: SlotMap<NodeId, Node<A>>,
    pub root: NodeId,
    pub anchors: HashMap<Key, NodeId>,
    pub structure: bool,
    pub layout: bool,
    pub focus: bool,
    pub fields: bool,
    pub popups: bool,
    pub hits: bool,
    pub activated: Vec<NodeId>,
    pub removed_damage: Option<crate::geometry::Rect>,
    pub scrolls: Vec<NodeId>,
    created: Vec<(Key, NodeId)>,
    #[cfg(debug_assertions)]
    seen: HashSet<Key>,
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
            root,
            anchors: HashMap::new(),
            structure: false,
            layout: false,
            focus: false,
            fields: false,
            popups: false,
            hits: false,
            activated: Vec::new(),
            removed_damage: None,
            scrolls: Vec::new(),
            created: Vec::new(),
            #[cfg(debug_assertions)]
            seen: HashSet::new(),
        }
    }

    pub fn apply<'a>(
        &mut self,
        element: Element<'a, A>,
        canvases: &mut slotmap::SecondaryMap<NodeId, CanvasPaint<'a>>,
    ) -> Reconciled<A> {
        let mut reconciled = Reconciled {
            removed: Vec::new(),
            committed: Vec::new(),
        };
        #[cfg(debug_assertions)]
        self.seen.clear();
        self.reconcile_list(self.root, vec![element], canvases, &mut reconciled);
        for (key, id) in self.created.drain(..) {
            if let Some(existing) = self.anchors.insert(key, id)
                && existing != id
            {
                panic!("duplicate retained key {:#x}", key.id());
            }
        }
        #[cfg(debug_assertions)]
        for node in self.nodes.values() {
            for child in &node.children {
                assert!(
                    self.nodes.contains_key(*child),
                    "a stale child {child:?} of {:#x}",
                    node.key.id()
                );
            }
        }
        reconciled
    }

    fn reconcile_list<'a>(
        &mut self,
        parent: NodeId,
        elements: Vec<Element<'a, A>>,
        canvases: &mut slotmap::SecondaryMap<NodeId, CanvasPaint<'a>>,
        reconciled: &mut Reconciled<A>,
    ) {
        let old = std::mem::take(&mut self.nodes[parent].children);
        let positional = old.len() == elements.len()
            && elements.iter().enumerate().all(|(index, element)| {
                let key = if element.id != Key::NONE {
                    element.id
                } else {
                    Key::structural(index)
                };
                let node = &self.nodes[old[index]];
                node.key == key && node.kind == element.content.kind()
            });
        let mut next = Vec::with_capacity(elements.len());
        if positional {
            for (index, element) in elements.into_iter().enumerate() {
                #[cfg(debug_assertions)]
                if element.id != Key::NONE {
                    self.check_key(element.id);
                }
                let id = old[index];
                self.patch_element(id, element, parent, canvases, reconciled);
                next.push(id);
            }
        } else {
            let mut keyed: HashMap<Key, NodeId> = HashMap::with_capacity(old.len());
            for id in &old {
                keyed.entry(self.nodes[*id].key).or_insert(*id);
            }
            let mut matched: HashSet<NodeId> = HashSet::with_capacity(old.len());
            for (index, element) in elements.into_iter().enumerate() {
                let named = element.id != Key::NONE;
                let key = if named {
                    element.id
                } else {
                    Key::structural(index)
                };
                #[cfg(debug_assertions)]
                if named {
                    self.check_key(key);
                }
                let kind = element.content.kind();
                let candidate = keyed
                    .get(&key)
                    .copied()
                    .filter(|id| !matched.contains(id) && self.nodes[*id].kind == kind);
                match candidate {
                    Some(id) => {
                        matched.insert(id);
                        if old.get(index) != Some(&id) {
                            self.structure = true;
                        }
                        self.patch_element(id, element, parent, canvases, reconciled);
                        next.push(id);
                    }
                    None => {
                        self.structure = true;
                        self.focus = true;
                        self.hits = true;
                        let mut element = element;
                        let fields = matches!(element.content, Content::Field(_));
                        let popups = element.popup.is_some();
                        let scroll = element.scroll.is_some();
                        let children = std::mem::take(&mut element.children);
                        let canvas = element.canvas.take();
                        let parent_layer = self.nodes[parent].layer;
                        let mut node = element.into_node(key, parent_layer);
                        node.parent = Some(parent);
                        let id = self.nodes.insert(node);
                        if fields {
                            self.fields = true;
                        }
                        if popups {
                            self.popups = true;
                        }
                        if scroll {
                            self.scrolls.push(id);
                        }
                        if let Some(canvas) = canvas {
                            canvases.insert(id, canvas.paint);
                        }
                        if named {
                            self.created.push((key, id));
                        }
                        if crate::element::animates(&self.nodes[id].content) {
                            self.activate(id);
                        }
                        self.dirty_layout(parent);
                        self.reconcile_list(id, children, canvases, reconciled);
                        next.push(id);
                    }
                }
            }
            if matched.len() != old.len() {
                self.dirty_layout(parent);
            }
            for id in old {
                if !matched.contains(&id) {
                    self.remove(id, reconciled);
                }
            }
        }
        self.nodes[parent].children = next;
    }

    fn patch_element<'a>(
        &mut self,
        id: NodeId,
        mut element: Element<'a, A>,
        _parent: NodeId,
        canvases: &mut slotmap::SecondaryMap<NodeId, CanvasPaint<'a>>,
        reconciled: &mut Reconciled<A>,
    ) {
        let children = std::mem::take(&mut element.children);
        let canvas = element
            .canvas
            .as_ref()
            .map_or(CanvasKind::None, |canvas| canvas.kind);
        if let Some(canvas) = element.canvas.take() {
            canvases.insert(id, canvas.paint);
        }
        let layer = element.layer;
        let mut patch = element.patch(&mut self.nodes[id]);
        if self.nodes[id].canvas != canvas {
            self.nodes[id].canvas = canvas;
            patch = patch.union(Dirty::PAINT);
        }
        if patch.geometry {
            self.dirty_layout(id);
            if self.nodes[id].scroll_spec.is_some() && !self.scrolls.contains(&id) {
                self.scrolls.push(id);
            }
        }
        if patch.paint {
            self.nodes[id].paint_dirty = true;
        }
        if patch.focus {
            self.focus = true;
        }
        if patch.fields {
            self.fields = true;
        }
        if patch.popup {
            self.popups = true;
        }
        if patch.hits {
            self.hits = true;
        }
        if patch.animate {
            self.activate(id);
        }
        let parent_layer = self.nodes[id]
            .parent
            .map_or(self.nodes[id].layer, |parent| self.nodes[parent].layer);
        self.assign_layer(id, layer, parent_layer);
        self.reconcile_list(id, children, canvases, reconciled);
    }

    fn assign_layer(&mut self, id: NodeId, requested: Option<Layer>, inherited: Layer) {
        let target = requested.unwrap_or(inherited);
        let previous = self.nodes[id].layer;
        if self.nodes[id].layer != target || self.nodes[id].layer_explicit != requested.is_some() {
            self.nodes[id].layer = target;
            self.nodes[id].layer_explicit = requested.is_some();
            self.nodes[id].paint_dirty = true;
            self.structure = true;
            if previous != target {
                self.rebase(id, target);
            }
        }
    }

    fn rebase(&mut self, id: NodeId, layer: Layer) {
        for index in 0..self.nodes[id].children.len() {
            let child = self.nodes[id].children[index];
            if !self.nodes[child].layer_explicit && self.nodes[child].layer != layer {
                self.nodes[child].layer = layer;
                self.nodes[child].paint_dirty = true;
                self.rebase(child, layer);
            }
        }
    }

    fn activate(&mut self, id: NodeId) {
        if !self.nodes[id].runtime.active {
            self.nodes[id].runtime.active = true;
            self.activated.push(id);
        }
    }

    pub(crate) fn wake(&mut self, id: NodeId) {
        self.activate(id);
    }

    #[cfg(debug_assertions)]
    fn check_key(&mut self, key: Key) {
        if !self.seen.insert(key) {
            panic!("duplicate retained key {:#x}", key.id());
        }
    }

    fn remove(&mut self, id: NodeId, reconciled: &mut Reconciled<A>) {
        let children = std::mem::take(&mut self.nodes[id].children);
        for child in children {
            self.remove(child, reconciled);
        }
        if let Some(edit) = self.nodes[id].runtime.edit.take() {
            let interaction = std::mem::replace(&mut self.nodes[id].interaction, Interaction::None);
            if let Interaction::Field {
                commit: Some(commit),
                ..
            } = interaction
            {
                reconciled.committed.push(commit(edit.text()));
            }
        }
        let node = &self.nodes[id];
        if node.painted {
            let erased = crate::paint::extent(&node.paint.material)
                .map_or(node.rect, |held| held.union(node.rect));
            self.removed_damage = Some(
                self.removed_damage
                    .map_or(erased, |held| held.union(erased)),
            );
        }
        let key = self.nodes[id].key;
        if self.anchors.get(&key) == Some(&id) {
            self.anchors.remove(&key);
        }
        self.structure = true;
        self.layout = true;
        self.focus = true;
        self.fields = true;
        self.popups = true;
        self.hits = true;
        reconciled.removed.push(id);
        self.nodes.remove(id);
    }

    fn dirty_layout(&mut self, id: NodeId) {
        let mut current = Some(id);
        while let Some(node) = current {
            if self.nodes[node].measure_dirty && self.nodes[node].arrange_dirty {
                break;
            }
            self.nodes[node].measure_dirty = true;
            self.nodes[node].arrange_dirty = true;
            current = self.nodes[node].parent;
        }
        self.layout = true;
    }

    pub fn invalidate_layout(&mut self, id: NodeId) {
        self.dirty_layout(id);
    }

    pub fn invalidate_arrange(&mut self, id: NodeId) {
        let mut current = Some(id);
        while let Some(node) = current {
            if self.nodes[node].arrange_dirty {
                break;
            }
            self.nodes[node].arrange_dirty = true;
            current = self.nodes[node].parent;
        }
        self.layout = true;
    }

    pub fn node_at(&self, key: Key) -> Option<NodeId> {
        self.anchors.get(&key).copied()
    }

    pub fn mark_dirty(&mut self) {
        for node in self.nodes.values_mut() {
            node.measure_dirty = true;
            node.measure_valid = false;
            node.arrange_dirty = true;
            node.paint_dirty = true;
        }
        self.layout = true;
        self.hits = true;
        self.focus = true;
    }
}
