use glam::Vec2;

use crate::geometry::{Corners, Rect};
use crate::paint::{Clip, Painter};
use crate::style::{Border, Fill, Shadow, State};
use crate::text::TextStyle;
use crate::theme::{Level, Role};
use crate::ui::{TOOLTIP_DELAY, Ui};

impl<A: Clone> Ui<A> {
    pub(crate) fn update_tooltip(&mut self) {
        let Some(node) = self.hover else {
            self.hovered_since = None;
            self.tooltip = None;
            return;
        };
        if self.tree.nodes[node].tooltip.is_none() {
            self.hovered_since = Some((node, self.time));
            self.tooltip = None;
            return;
        }
        let since = match self.hovered_since {
            Some((id, since)) if id == node => since,
            _ => {
                self.hovered_since = Some((node, self.time));
                self.tooltip = None;
                self.wake(TOOLTIP_DELAY);
                return;
            }
        };
        let remaining = TOOLTIP_DELAY - (self.time - since);
        if remaining > 0.0 {
            self.wake(remaining);
            return;
        }
        let text = self.tree.nodes[node]
            .tooltip
            .as_deref()
            .expect("the hovered node carries a tooltip");
        if self
            .tooltip
            .as_ref()
            .is_some_and(|(held, shown)| *held == node && shown == text)
        {
            return;
        }
        self.tooltip = Some((node, text.to_string()));
    }

    pub(crate) fn paint_overlays(&mut self) {
        let tooltip = self.tooltip.take();
        let focus = self.focus;
        let focus_visible = self.focus_visible;
        let pointer = self.pointer;
        let size = self.size;
        let mut instances = std::mem::take(&mut self.scratch);
        instances.clear();
        {
            let clip = Clip::new(Rect::from_min_size(Vec2::ZERO, size), Corners::ZERO);
            let mut painter =
                Painter::new(&mut instances, &mut self.text, &mut self.images, clip, None);
            if focus_visible
                && let Some(node) = focus
                && self.tree.nodes[node].visible
            {
                let (rect, corners, node_clip) = {
                    let node = &self.tree.nodes[node];
                    (
                        node.rect,
                        node.focus_ring.unwrap_or_else(|| {
                            crate::widget::focus_corners(&node.content, &self.theme)
                        }),
                        node.clip,
                    )
                };
                let previous = painter.clip();
                painter.push_clip(node_clip, Corners::ZERO);
                painter.outlined(
                    rect.expand(2.0),
                    corners,
                    Fill::None,
                    Border {
                        width: 2.0,
                        color: self.theme.palette.primary,
                    },
                    Shadow::NONE,
                );
                painter.pop_clip(previous);
            }
            let bars = self
                .tree
                .scrolls
                .iter()
                .filter_map(|id| {
                    let node = self.tree.nodes.get(*id)?;
                    let scroll = node.scroll?;
                    let spec = node.scroll_spec?;
                    if scroll.bar <= 0.01 || scroll.limit() <= 0.0 {
                        return None;
                    }
                    Some((node.rect, spec.axis, scroll))
                })
                .collect::<Vec<_>>();
            for (outer, axis, scroll) in bars {
                let bar = crate::scroll::bar(outer, axis, scroll);
                painter.rect(
                    bar.thumb,
                    Corners::capsule(),
                    self.theme.palette.outline.with_alpha(0.65 * scroll.bar),
                );
            }
            if let Some((_, text)) = &tooltip {
                let style = TextStyle::new(
                    self.theme.font(Level::Caption),
                    self.theme.palette.on_surface,
                );
                let layout = painter.layout(text, style, 0.0);
                let box_size = layout.size + Vec2::new(16.0, 10.0);
                let mut origin = pointer + Vec2::new(12.0, 16.0);
                if origin.x + box_size.x > self.size.x - 8.0 {
                    origin.x = self.size.x - 8.0 - box_size.x;
                }
                if origin.y + box_size.y > self.size.y - 8.0 {
                    origin.y = pointer.y - 12.0 - box_size.y;
                }
                let rect = Rect::from_min_size(origin, box_size);
                let surface = self
                    .theme
                    .surface(Role::Menu, State::Rest)
                    .with_corners(Corners::all(self.theme.metrics.corner_sm));
                painter.surface(rect, surface);
                painter.text(
                    &layout,
                    rect.min + Vec2::new(8.0, 5.0),
                    self.theme.palette.on_surface,
                );
            }
        }
        if self.overlay.material != instances.material {
            let held = crate::paint::extent(&self.overlay.material);
            let fresh = crate::paint::extent(&instances.material);
            let extent = match (held, fresh) {
                (Some(held), Some(fresh)) => Some(held.union(fresh)),
                (held, fresh) => held.or(fresh),
            };
            if let Some(extent) = extent {
                self.damage.add(extent);
            }
            self.overlay.assign(&instances.material);
        }
        self.scratch = instances;
        self.tooltip = tooltip;
    }
}
