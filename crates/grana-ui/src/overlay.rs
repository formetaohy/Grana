use glam::Vec2;

use crate::element::{Axis, Layer};
use crate::geometry::{Corners, Rect};
use crate::paint::{Clip, LayerInstances, Painter};
use crate::style::{Border, Fill, Shadow};
use crate::text::TextStyle;
use crate::theme::Level;
use crate::ui::{TOOLTIP_DELAY, Ui};

impl<A: Clone> Ui<A> {
    pub(crate) fn update_tooltip(&mut self) {
        let Some(node) = self.hover else {
            self.hovered_since = None;
            self.tooltip = None;
            return;
        };
        let since = match self.hovered_since {
            Some((id, since)) if id == node => since,
            _ => {
                self.hovered_since = Some((node, self.time));
                self.tooltip = None;
                return;
            }
        };
        if self.time - since > TOOLTIP_DELAY
            && let Some(text) = self.tree.nodes[node].tooltip.clone()
        {
            self.tooltip = Some((node, text));
            self.animating = true;
        }
        if self.time - since <= TOOLTIP_DELAY {
            self.animating = true;
        }
    }

    pub(crate) fn paint_overlays(&mut self) {
        let tooltip = self.tooltip.clone();
        let focus = self.focus;
        let focus_visible = self.focus_visible;
        let pointer = self.pointer;
        let size = self.size;
        let dark = self.theme.dark;
        let mut instances = LayerInstances::default();
        let mut tooltip_rect = None;
        {
            let clip = Clip::new(Rect::from_min_size(Vec2::ZERO, size), Corners::ZERO);
            let mut painter = Painter::new(&mut instances, &mut self.text, clip, dark, true);
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
                        width: 1.5,
                        color: self.theme.palette.focus,
                    },
                    Shadow::NONE,
                );
                painter.pop_clip(previous);
            }
            let bars = self
                .tree
                .ids
                .iter()
                .filter_map(|id| {
                    let node = &self.tree.nodes[*id];
                    let scroll = node.scroll?;
                    let spec = node.scroll_spec?;
                    if scroll.bar <= 0.01 || scroll.limit() <= 0.0 {
                        return None;
                    }
                    Some((node.rect, spec.axis, scroll))
                })
                .collect::<Vec<_>>();
            for (outer, axis, scroll) in bars {
                let thumb = scrollbar_thumb(outer, axis, scroll);
                painter.rect(
                    thumb,
                    Corners::capsule(),
                    self.theme
                        .palette
                        .text_quaternary
                        .with_alpha(0.7 * scroll.bar),
                );
            }
            if let Some((_, text)) = &tooltip {
                let style = TextStyle::new(
                    self.theme.font(Level::Caption),
                    self.theme.palette.text_primary,
                );
                let layout = painter.layout(text, style, 0.0);
                let size = layout.size + Vec2::new(14.0, 10.0);
                let mut origin = pointer + Vec2::new(14.0, 18.0);
                if origin.x + size.x > self.size.x - 8.0 {
                    origin.x = self.size.x - 8.0 - size.x;
                }
                if origin.y + size.y > self.size.y - 8.0 {
                    origin.y = pointer.y - 14.0 - size.y;
                }
                let rect = Rect::from_min_size(origin, size);
                painter.text(
                    &layout,
                    rect.min + Vec2::new(7.0, 5.0),
                    self.theme.palette.text_primary,
                );
                tooltip_rect = Some(rect.expand(2.0));
            }
        }
        let hud = Layer::Hud.index();
        self.layers[hud].material.extend(instances.material);
        self.layers[hud].ink.extend(instances.ink);
        match tooltip_rect {
            Some(rect) => self.place_tooltip_glass(rect),
            None => {
                if let Some(glass) = self.tooltip_glass.take() {
                    self.scene.remove(glass);
                }
            }
        }
    }

    fn place_tooltip_glass(&mut self, rect: Rect) {
        let style = crate::glass::GlassStyle::tooltip();
        let frame = crate::ui::glass_rect(rect);
        match self.tooltip_glass {
            Some(glass) => {
                self.scene.place(glass, frame);
                self.scene.set_clip(glass, None);
            }
            None => {
                let glass = grana_glass::Glass::new(frame)
                    .shape(style.shape)
                    .material(style.material)
                    .z(style.layer);
                self.tooltip_glass = Some(self.scene.add(glass));
            }
        }
    }
}

fn scrollbar_thumb(outer: Rect, axis: Axis, scroll: crate::scroll::ScrollState) -> Rect {
    let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
    let position = if scroll.limit() > 0.0 {
        (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    match axis {
        Axis::Column => {
            let track = Rect::from_min_size(
                Vec2::new(outer.max.x - 8.0, outer.min.y + 4.0),
                Vec2::new(4.0, (outer.height() - 8.0).max(8.0)),
            );
            let height = (track.height() * ratio).max(24.0).min(track.height());
            let travel = track.height() - height;
            Rect::from_min_size(
                Vec2::new(track.min.x, track.min.y + travel * position),
                Vec2::new(track.width(), height),
            )
        }
        Axis::Row => {
            let track = Rect::from_min_size(
                Vec2::new(outer.min.x + 4.0, outer.max.y - 8.0),
                Vec2::new((outer.width() - 8.0).max(8.0), 4.0),
            );
            let width = (track.width() * ratio).max(24.0).min(track.width());
            let travel = track.width() - width;
            Rect::from_min_size(
                Vec2::new(track.min.x + travel * position, track.min.y),
                Vec2::new(width, track.height()),
            )
        }
    }
}
