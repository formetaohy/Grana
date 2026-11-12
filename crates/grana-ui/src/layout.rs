use glam::Vec2;

use crate::element::{Axis, Content, Flow, Length, align_factor};
use crate::geometry::{Corners, Rect};
use crate::node::NodeId;
use crate::paint::Clip;
use crate::text::{Align, Text};
use crate::tree::Tree;

#[derive(Clone, Copy, Debug)]
pub struct Loose {
    pub x: bool,
    pub y: bool,
}

impl Loose {
    pub const TIGHT: Self = Self { x: false, y: false };
    pub const LOOSE: Self = Self { x: true, y: true };
}

pub struct LayoutEngine<'a> {
    pub text: &'a mut Text,
    pub theme: &'a crate::theme::Theme,
    pub screen: Vec2,
}

impl<'a> LayoutEngine<'a> {
    pub fn run<A>(&mut self, tree: &mut Tree<A>) {
        let root = tree.root;
        for index in 0..tree.nodes[root].children.len() {
            let child = tree.nodes[root].children[index];
            self.measure(tree, child, self.screen, Loose::TIGHT);
        }
        let screen = Rect::from_min_size(Vec2::ZERO, self.screen);
        let clip = Clip::new(screen, Corners::ZERO);
        tree.nodes[root].rect = screen;
        tree.nodes[root].measured = self.screen;
        for index in 0..tree.nodes[root].children.len() {
            let child = tree.nodes[root].children[index];
            let size = tree.nodes[child].measured;
            self.arrange(tree, child, Rect::from_min_size(Vec2::ZERO, size), clip);
        }
        tree.layout_dirty = false;
    }

    fn measure<A>(&mut self, tree: &mut Tree<A>, id: NodeId, available: Vec2, loose: Loose) {
        let forced = tree.nodes[id].position.map(|rect| rect.size());
        let available = forced.unwrap_or(available);
        if tree.nodes[id].popup_spec.is_some() {
            let width = tree.nodes[id]
                .popup_spec
                .expect("the popup is present")
                .width;
            let padding = tree.nodes[id].layout.padding;
            let gap = tree.nodes[id].layout.gap;
            let inner_width = (width - padding.left - padding.right).max(0.0);
            tree.nodes[id].measured = Vec2::ZERO;
            tree.nodes[id].provisional = Vec2::ZERO;
            let mut height = 0.0f32;
            let count = tree.nodes[id].children.len();
            for index in 0..count {
                let child = tree.nodes[id].children[index];
                let remaining = Vec2::new(inner_width, self.screen.y);
                self.measure(tree, child, remaining, Loose::LOOSE);
                height += tree.nodes[child].measured.y;
                if index + 1 < count {
                    height += gap;
                }
            }
            tree.nodes[id].popup_height = height + padding.top + padding.bottom;
            tree.nodes[id].content_main = height;
            tree.nodes[id].content_cross = inner_width;
            return;
        }
        if tree.nodes[id].scroll_spec.is_some() {
            self.measure_scroll(tree, id, available, loose);
            return;
        }
        if matches!(tree.nodes[id].content, Content::Empty) {
            self.measure_group(tree, id, available, loose, forced);
            return;
        }
        let layout = tree.nodes[id].layout;
        let content = &tree.nodes[id].content;
        let (mut size, baseline) = if let Some(size) = forced {
            (size, None)
        } else {
            measure_leaf(content, layout, available, self.text, self.theme)
        };
        if !loose.x && forced.is_none() {
            size.x = size.x.min(available.x.max(0.0));
        }
        let size = Vec2::new(size.x.max(0.0), size.y.max(0.0));
        let node = &mut tree.nodes[id];
        node.measured = size;
        node.provisional = size;
        node.inner_provisional = size;
        node.baseline = baseline;
        node.auto_cross = false;
        node.auto_main = false;
    }

    fn measure_scroll<A>(&mut self, tree: &mut Tree<A>, id: NodeId, available: Vec2, loose: Loose) {
        let spec = tree.nodes[id].scroll_spec.expect("the scroll is present");
        let inner = Vec2::new(
            match spec.axis {
                Axis::Column => available.x,
                Axis::Row => match spec.length {
                    Length::Auto | Length::Fill => available.x,
                    length => length.resolve(available.x),
                },
            },
            match spec.axis {
                Axis::Column => match spec.length {
                    Length::Auto | Length::Fill => available.y,
                    length => length.resolve(available.y),
                },
                Axis::Row => available.y,
            },
        );
        let mut size = inner;
        if !loose.x {
            size.x = size.x.min(available.x.max(0.0));
        }
        let mut cross = 0.0f32;
        let mut main = 0.0f32;
        for index in 0..tree.nodes[id].children.len() {
            let child = tree.nodes[id].children[index];
            let remaining = Vec2::new(size.x, self.screen.y.max(size.y));
            self.measure(tree, child, remaining, Loose::TIGHT);
            match spec.axis {
                Axis::Column => {
                    main += tree.nodes[child].measured.y;
                    cross = cross.max(tree.nodes[child].measured.x);
                }
                Axis::Row => {
                    main += tree.nodes[child].measured.x;
                    cross = cross.max(tree.nodes[child].measured.y);
                }
            }
        }
        let node = &mut tree.nodes[id];
        node.measured = size;
        node.provisional = size;
        node.inner_provisional = size;
        node.content_main = main;
        node.content_cross = cross;
        node.auto_cross = false;
        node.auto_main = false;
    }

    fn measure_group<A>(
        &mut self,
        tree: &mut Tree<A>,
        id: NodeId,
        available: Vec2,
        loose: Loose,
        forced: Option<Vec2>,
    ) {
        let layout = tree.nodes[id].layout;
        let provisional = forced.unwrap_or_else(|| {
            Vec2::new(
                layout.size.width.resolve(available.x),
                layout.size.height.resolve(available.y),
            )
        });
        let outer = Rect::from_min_size(Vec2::ZERO, provisional);
        let mut inner = layout.padding.shrink(outer);
        if layout.flow == Flow::Row && layout.size.height == Length::Auto && forced.is_none() {
            inner = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 0.0));
        }
        if layout.flow == Flow::Column && layout.size.width == Length::Auto && forced.is_none() {
            inner = Rect::from_min_size(inner.min, Vec2::new(0.0, inner.height()));
        }
        let (loose_width, loose_height) = if forced.is_some() {
            (false, false)
        } else {
            (
                layout.flow == Flow::Row
                    || layout.size.width == Length::Auto
                    || (loose.x && matches!(layout.size.width, Length::Fill | Length::Maximum(_))),
                layout.size.height == Length::Auto
                    || (loose.y && matches!(layout.size.height, Length::Fill | Length::Maximum(_))),
            )
        };
        let child_loose = Loose {
            x: loose_width,
            y: loose_height,
        };
        let mut cursor = inner.min;
        let mut cross = 0.0f32;
        let mut main = 0.0f32;
        let mut line = 0.0f32;
        let mut line_height = 0.0f32;
        for index in 0..tree.nodes[id].children.len() {
            let child = tree.nodes[id].children[index];
            if tree.nodes[child].position.is_some() || tree.nodes[child].popup_spec.is_some() {
                self.measure(tree, child, available, Loose::TIGHT);
                continue;
            }
            let remaining = match layout.flow {
                Flow::Column | Flow::Stack => {
                    Vec2::new(inner.width(), (inner.max.y - cursor.y).max(0.0))
                }
                Flow::Row if layout.wrap => Vec2::new(inner.width(), inner.height()),
                Flow::Row => Vec2::new((inner.max.x - cursor.x).max(0.0), inner.height()),
            };
            let mut size = measure_and_layout(self, tree, child, remaining, child_loose);
            if layout.flow == Flow::Row
                && layout.wrap
                && cursor.x > inner.min.x + 0.01
                && cursor.x + size.x > inner.max.x + 0.01
            {
                line += line_height + layout.gap;
                cursor.x = inner.min.x;
                line_height = 0.0;
                let remaining = Vec2::new(inner.width(), inner.height());
                size = measure_and_layout(self, tree, child, remaining, child_loose);
            }
            match layout.flow {
                Flow::Column | Flow::Stack => {
                    cursor.y += size.y + layout.gap;
                    cross = cross.max(size.x);
                    main = (cursor.y - inner.min.y - layout.gap).max(0.0);
                }
                Flow::Row => {
                    cursor.x += size.x + layout.gap;
                    if layout.wrap {
                        line_height = line_height.max(size.y);
                        cross = cross.max(line + line_height);
                        main = main.max((cursor.x - inner.min.x - layout.gap).max(0.0));
                    } else {
                        cross = cross.max(size.y);
                        main = (cursor.x - inner.min.x - layout.gap).max(0.0);
                    }
                }
            }
        }
        let content = match layout.flow {
            Flow::Column | Flow::Stack => Vec2::new(cross, main),
            Flow::Row => Vec2::new(main, cross),
        };
        let measured = content + layout.padding.size();
        let size = if forced.is_some() {
            provisional
        } else {
            let width = match layout.size.width {
                Length::Auto => {
                    if loose.x {
                        measured.x
                    } else {
                        measured.x.min(available.x)
                    }
                }
                Length::Maximum(maximum) => {
                    if loose.x {
                        measured.x.min(maximum)
                    } else {
                        measured.x.min(maximum).min(available.x)
                    }
                }
                Length::Fill if loose.x => measured.x,
                _ => provisional.x,
            };
            let height = match layout.size.height {
                Length::Auto => measured.y,
                Length::Maximum(maximum) => measured.y.min(maximum),
                Length::Fill if loose.y => measured.y,
                _ => provisional.y,
            };
            Vec2::new(width.max(0.0), height.max(0.0))
        };
        let (auto_cross, auto_main) = if forced.is_some() {
            (false, true)
        } else {
            (
                match layout.flow {
                    Flow::Row => {
                        layout.size.height == Length::Auto
                            || (loose.y && layout.size.height == Length::Fill)
                    }
                    Flow::Column | Flow::Stack => {
                        layout.size.width == Length::Auto
                            || (loose.x && layout.size.width == Length::Fill)
                    }
                },
                match layout.flow {
                    Flow::Row => {
                        layout.size.width == Length::Auto
                            || (loose.x && layout.size.width == Length::Fill)
                    }
                    Flow::Column | Flow::Stack => {
                        layout.size.height == Length::Auto
                            || (loose.y && layout.size.height == Length::Fill)
                    }
                },
            )
        };
        let node = &mut tree.nodes[id];
        node.measured = size;
        node.provisional = provisional;
        node.inner_provisional = inner.size();
        node.content_main = main;
        node.content_cross = cross;
        node.auto_cross = auto_cross;
        node.auto_main = auto_main;
    }

    fn arrange<A>(&mut self, tree: &mut Tree<A>, id: NodeId, rect: Rect, parent_clip: Clip) {
        let (layout, position, scroll_spec, popup_spec) = {
            let node = &tree.nodes[id];
            (
                node.layout,
                node.position,
                node.scroll_spec,
                node.popup_spec,
            )
        };
        let rect = position.unwrap_or(rect);
        let corners = if layout.clip {
            tree.nodes[id]
                .background
                .map_or(Corners::ZERO, |surface| surface.corners)
        } else {
            Corners::ZERO
        };
        let clip = if layout.clip {
            parent_clip.intersect(rect, corners)
        } else {
            parent_clip
        };
        {
            let node = &mut tree.nodes[id];
            if node.rect != rect || node.clip != clip.rect || node.clip_corners != clip.corners {
                node.paint_dirty = true;
            }
            node.rect = rect;
            node.clip = clip.rect;
            node.clip_corners = clip.corners;
            node.visible = true;
        }
        if popup_spec.is_some() {
            self.arrange_popup(tree, id);
            return;
        }
        if scroll_spec.is_some() {
            self.arrange_scroll(tree, id, rect, clip);
            return;
        }
        if matches!(tree.nodes[id].content, Content::Empty) {
            self.arrange_group(tree, id, rect, clip);
        }
    }

    fn arrange_group<A>(&mut self, tree: &mut Tree<A>, id: NodeId, rect: Rect, clip: Clip) {
        let (layout, inner_provisional, content_main, content_cross, auto_cross, auto_main) = {
            let node = &tree.nodes[id];
            (
                node.layout,
                node.inner_provisional,
                node.content_main,
                node.content_cross,
                node.auto_cross,
                node.auto_main,
            )
        };
        let inner_min = rect.min + Vec2::new(layout.padding.left, layout.padding.top);
        let cross_base = if auto_cross {
            content_cross
        } else {
            match layout.flow {
                Flow::Row => inner_provisional.y,
                Flow::Column | Flow::Stack => inner_provisional.x,
            }
        };
        let inner_main = match layout.flow {
            Flow::Row => inner_provisional.x,
            Flow::Column | Flow::Stack => inner_provisional.y,
        };
        let main_space = if auto_main || layout.wrap {
            0.0
        } else {
            (inner_main - content_main).max(0.0)
        };
        let justify = align_factor(layout.justify);
        let align = align_factor(layout.align);
        let baseline = if layout.flow == Flow::Row && layout.align == Align::Baseline {
            (0..tree.nodes[id].children.len())
                .map(|index| tree.nodes[id].children[index])
                .filter(|child| tree.nodes[*child].position.is_none())
                .filter_map(|child| tree.nodes[child].baseline)
                .fold(f32::MIN, f32::max)
        } else {
            f32::MIN
        };
        let mut cursor = Vec2::ZERO;
        let mut line = 0.0f32;
        let mut line_height = 0.0f32;
        for index in 0..tree.nodes[id].children.len() {
            let child = tree.nodes[id].children[index];
            if tree.nodes[child].position.is_some() || tree.nodes[child].popup_spec.is_some() {
                let size = tree.nodes[child].measured;
                let overlay = tree.nodes[child]
                    .position
                    .unwrap_or_else(|| Rect::from_min_size(Vec2::ZERO, size));
                self.arrange(tree, child, overlay, clip);
                continue;
            }
            let size = tree.nodes[child].measured;
            match layout.flow {
                Flow::Column | Flow::Stack => {
                    let x = inner_min.x + (cross_base - size.x) * align;
                    let y = inner_min.y + cursor.y + main_space * justify;
                    self.arrange(
                        tree,
                        child,
                        Rect::from_min_size(Vec2::new(x, y), size),
                        clip,
                    );
                    cursor.y += size.y + layout.gap;
                }
                Flow::Row => {
                    if layout.wrap
                        && cursor.x > 0.01
                        && cursor.x + size.x > inner_provisional.x + 0.01
                    {
                        line += line_height + layout.gap;
                        cursor.x = 0.0;
                        line_height = 0.0;
                    }
                    let y = if baseline > f32::MIN {
                        inner_min.y
                            + line
                            + tree.nodes[child].baseline.map_or(0.0, |own| baseline - own)
                    } else if layout.wrap {
                        inner_min.y + line + (line_height.max(size.y) - size.y) * align
                    } else {
                        inner_min.y + line + (cross_base - line_height.max(size.y)) * align
                    };
                    let x = inner_min.x + cursor.x + main_space * justify;
                    self.arrange(
                        tree,
                        child,
                        Rect::from_min_size(Vec2::new(x, y), size),
                        clip,
                    );
                    cursor.x += size.x + layout.gap;
                    line_height = line_height.max(size.y);
                }
            }
        }
    }

    fn arrange_scroll<A>(&mut self, tree: &mut Tree<A>, id: NodeId, rect: Rect, clip: Clip) {
        let spec = tree.nodes[id].scroll_spec.expect("the scroll is present");
        let offset = tree.nodes[id].scroll.map_or(0.0, |scroll| scroll.offset);
        let origin = match spec.axis {
            Axis::Column => Vec2::new(rect.min.x, rect.min.y - offset),
            Axis::Row => Vec2::new(rect.min.x - offset, rect.min.y),
        };
        let mut cursor = origin;
        for index in 0..tree.nodes[id].children.len() {
            let child = tree.nodes[id].children[index];
            let size = tree.nodes[child].measured;
            self.arrange(tree, child, Rect::from_min_size(cursor, size), clip);
            match spec.axis {
                Axis::Column => cursor.y += size.y,
                Axis::Row => cursor.x += size.x,
            }
        }
    }

    fn arrange_popup<A>(&mut self, tree: &mut Tree<A>, id: NodeId) {
        let spec = tree.nodes[id].popup_spec.expect("the popup is present");
        let open = tree.nodes[id].runtime.open;
        if !open {
            hide(tree, id);
            return;
        }
        let Some(anchor) = tree.node_at(spec.anchor) else {
            hide(tree, id);
            return;
        };
        let anchor_rect = tree.nodes[anchor].rect;
        let padding = tree.nodes[id].layout.padding;
        let gap = tree.nodes[id].layout.gap;
        let margin = 10.0;
        let offset = 6.0;
        let width = spec.width;
        let x = anchor_rect
            .min
            .x
            .min(self.screen.x - width - margin)
            .max(margin);
        let content = tree.nodes[id].popup_height.max(16.0);
        let below_room = self.screen.y - anchor_rect.max.y - offset - margin;
        let above_room = anchor_rect.min.y - offset - margin;
        let (y, height) = if content <= below_room || below_room >= above_room {
            let h = content.min(below_room.max(120.0));
            (anchor_rect.max.y + offset, h)
        } else {
            let h = content.min(above_room.max(120.0));
            ((anchor_rect.min.y - offset - h).max(margin), h)
        };
        let rect = Rect::from_min_size(Vec2::new(x, y), Vec2::new(width, height));
        let screen = Rect::from_min_size(Vec2::ZERO, self.screen);
        let corners = Corners::all(self.theme.metrics.panel_radius);
        let clip = Clip::new(screen, Corners::ZERO).intersect(rect, corners);
        {
            let node = &mut tree.nodes[id];
            if node.rect != rect {
                node.paint_dirty = true;
            }
            node.rect = rect;
            node.clip = clip.rect;
            node.clip_corners = clip.corners;
            node.visible = true;
            node.measured = rect.size();
        }
        let mut cursor = rect.min + Vec2::new(padding.left, padding.top);
        for index in 0..tree.nodes[id].children.len() {
            let child = tree.nodes[id].children[index];
            let size = tree.nodes[child].measured;
            self.arrange(tree, child, Rect::from_min_size(cursor, size), clip);
            cursor.y += size.y + gap;
        }
    }
}

fn measure_and_layout<A>(
    engine: &mut LayoutEngine<'_>,
    tree: &mut Tree<A>,
    id: NodeId,
    available: Vec2,
    loose: Loose,
) -> Vec2 {
    engine.measure(tree, id, available, loose);
    tree.nodes[id].measured
}

fn hide<A>(tree: &mut Tree<A>, id: NodeId) {
    tree.nodes[id].visible = false;
    for index in 0..tree.nodes[id].children.len() {
        let child = tree.nodes[id].children[index];
        hide(tree, child);
    }
}

pub fn measure_leaf(
    content: &Content,
    layout: crate::element::Layout,
    available: Vec2,
    text: &mut Text,
    theme: &crate::theme::Theme,
) -> (Vec2, Option<f32>) {
    match content {
        Content::Text(text_content) => {
            let style = text_content.style;
            let wrap = style.wrap;
            let width = if wrap { available.x } else { 0.0 };
            let text_layout = text.layout(
                &text_content.text,
                style.font,
                width,
                wrap,
                style.truncate,
                style.align,
            );
            let width =
                if matches!(style.align, Align::Start | Align::Baseline) || available.x <= 0.0 {
                    text_layout.size.x
                } else {
                    available.x
                };
            let size = Vec2::new(width, text_layout.size.y);
            (size, Some(text_layout.baseline))
        }
        Content::Empty | Content::Canvas => (
            Vec2::new(
                layout.size.width.resolve(available.x),
                layout.size.height.resolve(available.y),
            ),
            None,
        ),
        other => crate::widget::measure(other, layout, available, text, theme),
    }
}
