use glam::Vec2;

use crate::geometry::Rect;
use crate::node::NodeId;
use crate::text::Text;
use crate::theme::Theme;
use crate::tree::Tree;

#[derive(Clone, Copy)]
pub(crate) struct Hit {
    pub node: NodeId,
    pub rect: Rect,
    pub clip: Rect,
    pub scroll: bool,
    pub layer: usize,
    pub blocking: bool,
}

pub(crate) fn collect<A>(
    tree: &Tree<A>,
    id: NodeId,
    hits: &mut Vec<Hit>,
    text: &mut Text,
    theme: &Theme,
) {
    let node = &tree.nodes[id];
    if !node.visible {
        return;
    }
    let visible = node.clip.width() > 0.0 && node.clip.height() > 0.0;
    let interactive = node.interactive();
    let scroll = node.scroll_spec.is_some();
    let popup = node.popup_spec.is_some() && node.runtime.open;
    let blocking = !interactive && !scroll && !popup && opaque(node);
    if visible && (interactive || scroll || popup || blocking) && (node.enabled || blocking) {
        let rect = if interactive {
            crate::widget::hit_rect(node, text, theme)
        } else {
            node.rect
        };
        hits.push(Hit {
            node: id,
            rect,
            clip: node.clip,
            scroll,
            layer: node.layer.index(),
            blocking,
        });
    }
    for child in &node.children {
        collect(tree, *child, hits, text, theme);
    }
}

fn opaque<A>(node: &crate::node::Node<A>) -> bool {
    node.glass_spec.is_some()
        || node
            .background
            .is_some_and(|surface| surface.fill.is_visible())
}

pub(crate) fn top(hits: &[Hit], pointer: Vec2) -> Option<&Hit> {
    hits.iter()
        .filter(|hit| hit.clip.contains(pointer) && hit.rect.contains(pointer))
        .max_by_key(|hit| hit.layer)
}

pub(crate) fn resolve(hits: &[Hit], pointer: Vec2) -> Option<&Hit> {
    top(hits, pointer).filter(|hit| !hit.blocking)
}

pub(crate) fn resolve_scroll(hits: &[Hit], pointer: Vec2) -> Option<&Hit> {
    hits.iter()
        .filter(|hit| hit.scroll && hit.clip.contains(pointer) && hit.rect.contains(pointer))
        .max_by_key(|hit| hit.layer)
}
