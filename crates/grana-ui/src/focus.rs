use crate::node::NodeId;
use crate::ui::Ui;

impl<A: Clone> Ui<A> {
    pub(crate) fn collect_focus(&mut self) {
        let root = self
            .modal
            .filter(|modal| self.tree.nodes.contains_key(*modal))
            .unwrap_or(self.tree.root);
        let mut focus = std::mem::take(&mut self.focus_order);
        focus.clear();
        self.collect_focus_in(root, &mut focus);
        self.focus_order = focus;
        if self
            .focus
            .is_some_and(|focus| !self.focus_order.contains(&focus))
        {
            self.focus = None;
            self.focus_visible = false;
        }
    }

    fn collect_focus_in(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let node = &self.tree.nodes[id];
        if !node.visible || !node.enabled {
            return;
        }
        if node.interaction.is_focusable() {
            out.push(id);
        }
        for child in &node.children {
            self.collect_focus_in(*child, out);
        }
    }

    pub(crate) fn advance_focus(&mut self, forward: bool) -> bool {
        let count = self.focus_order.len();
        if count == 0 {
            return false;
        }
        let current = self
            .focus
            .and_then(|focus| self.focus_order.iter().position(|key| *key == focus));
        let next = match (current, forward) {
            (Some(index), true) => (index + 1) % count,
            (Some(index), false) => (index + count - 1) % count,
            (None, true) => 0,
            (None, false) => count - 1,
        };
        self.focus = Some(self.focus_order[next]);
        true
    }
}
