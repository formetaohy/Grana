use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Edges, Rect};
use crate::input::{Button, Input};
use crate::motion::Motion;
use crate::paint::{Clip, Instance, Painter};
use crate::registry::{Cursor, Registry};
use crate::render::Renderer;
use crate::style::{Look, State};
use crate::text::{Align, Text, TextLayout, TextStyle};
use crate::theme::{Level, Role, Theme};

const PROPERTY_SALT: u64 = 0x9e37_79b9_7f4a_7c15;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Fill,
    Fixed(f32),
    Maximum(f32),
}

#[derive(Clone, Copy, Debug)]
pub struct Block {
    padding: Edges,
    gap: f32,
    width: Length,
    height: Length,
    align: Align,
    justify: Align,
    background: Option<Look>,
    clip: bool,
}

impl Block {
    pub fn column() -> Self {
        Self {
            padding: Edges::ZERO,
            gap: 0.0,
            width: Length::Fill,
            height: Length::Auto,
            align: Align::Start,
            justify: Align::Start,
            background: None,
            clip: false,
        }
    }

    pub fn row() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            ..Self::column()
        }
    }

    pub fn padding(mut self, padding: Edges) -> Self {
        self.padding = padding;
        self
    }

    pub fn gaps(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn justify(mut self, justify: Align) -> Self {
        self.justify = justify;
        self
    }

    pub fn background(mut self, look: Look) -> Self {
        self.background = Some(look);
        self
    }

    pub fn clipped(mut self) -> Self {
        self.clip = true;
        self
    }
}

#[derive(Clone, Copy, Debug)]
struct Area {
    rect: Rect,
    cursor: Vec2,
    axis: Axis,
    gap: f32,
    align: Align,
    clip: Clip,
    origin: Vec2,
    cross: f32,
    main: f32,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    instance: usize,
    overlay: usize,
    hit: usize,
    wheel: usize,
    offset: f32,
    size: f32,
    direct: bool,
}

#[derive(Default)]
struct SpanFrame {
    spans: Vec<Span>,
    open: Option<Span>,
}

impl Area {
    fn remaining(&self) -> Vec2 {
        match self.axis {
            Axis::Column => Vec2::new(
                self.rect.width(),
                (self.rect.max.y - self.cursor.y).max(0.0),
            ),
            Axis::Row => Vec2::new(
                (self.rect.max.x - self.cursor.x).max(0.0),
                self.rect.height(),
            ),
        }
    }

    fn content_size(&self) -> Vec2 {
        match self.axis {
            Axis::Column => Vec2::new(self.cross, self.main),
            Axis::Row => Vec2::new(self.main, self.cross),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollInfo {
    pub offset: f32,
    pub viewport: f32,
    pub content: f32,
    pub limit: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Response {
    pub rect: Rect,
    pub hovered: bool,
    pub pressed: bool,
    pub active: bool,
    pub clicked: bool,
    pub dragging: bool,
}

impl Response {
    pub fn state(&self) -> State {
        if self.pressed || self.active {
            State::Pressed
        } else if self.hovered {
            State::Hover
        } else {
            State::Rest
        }
    }
}

pub struct Frame {
    pub animating: bool,
    pub cursor: Cursor,
}

pub struct Ui {
    renderer: Renderer,
    text: Text,
    theme: Theme,
    input: Input,
    registry: Registry,
    instances: Vec<Instance>,
    overlay: Vec<Instance>,
    areas: Vec<Area>,
    frames: Vec<SpanFrame>,
    ids: Vec<u64>,
    scale: f32,
    size: Vec2,
    time: f32,
    elapsed: f32,
    occluders: Vec<Rect>,
    overlay_clip: Clip,
    overlay_mode: u32,
}

impl Ui {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let theme = Theme::dark();
        Self {
            renderer: Renderer::new(device, queue, format),
            text: Text::new(device),
            theme,
            input: Input::new(),
            registry: Registry::new(),
            instances: Vec::new(),
            overlay: Vec::new(),
            areas: Vec::new(),
            frames: Vec::new(),
            ids: Vec::new(),
            scale: 1.0,
            size: Vec2::ZERO,
            time: 0.0,
            elapsed: 0.0,
            occluders: Vec::new(),
            overlay_clip: Clip::new(Rect::ZERO, Corners::ZERO),
            overlay_mode: 0,
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn theme_mut(&mut self) -> &mut Theme {
        &mut self.theme
    }

    pub fn input_mut(&mut self) -> &mut Input {
        &mut self.input
    }

    pub fn input(&self) -> &Input {
        &self.input
    }

    pub fn size(&self) -> Vec2 {
        self.size
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn text(&mut self) -> &mut Text {
        &mut self.text
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.renderer.resize(size);
    }

    pub fn begin(&mut self, size: Vec2, scale: f32, time: f64) {
        self.size = size;
        self.scale = scale;
        self.time = time as f32;
        self.elapsed = self.input.begin(size, scale, time);
        self.text.begin(scale);
        let pointer = self.input.position();
        let pressed = self.input.button(Button::Primary).pressed;
        let released = self.input.button(Button::Primary).released;
        let down = self.input.button(Button::Primary).down;
        self.registry
            .begin(pointer, pressed, released, down, self.time);
        self.instances.clear();
        self.overlay.clear();
        self.occluders.clear();
        self.areas.clear();
        self.frames.clear();
        self.ids.clear();
        self.overlay_clip = Clip::new(Rect::from_min_size(Vec2::ZERO, size), Corners::ZERO);
    }

    pub fn end(&mut self) -> Frame {
        self.registry.end(self.elapsed);
        self.text.flush(self.renderer.queue());
        let pointer = self.input.position();
        self.renderer
            .write_uniforms(self.scale, self.time, [pointer.x, pointer.y]);
        self.areas.clear();
        self.frames.clear();
        self.ids.clear();
        Frame {
            animating: self.registry.animating(),
            cursor: self.registry.cursor(),
        }
    }

    pub fn paint(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        self.renderer.paint(
            encoder,
            target,
            self.text.atlas(),
            &self.instances,
            &self.overlay,
        );
    }

    pub fn scene_view(&self) -> &wgpu::TextureView {
        self.renderer.scene_view()
    }

    pub fn root(&mut self, content: impl FnOnce(&mut Self)) {
        let rect = Rect::from_min_size(Vec2::ZERO, self.size);
        self.areas.push(Area {
            rect,
            cursor: rect.min,
            axis: Axis::Column,
            gap: 0.0,
            align: Align::Start,
            clip: Clip::new(rect, Corners::ZERO),
            origin: rect.min,
            cross: 0.0,
            main: 0.0,
        });
        self.push_frame();
        content(self);
        self.pop_frame();
        self.areas.pop();
    }

    fn area(&self) -> &Area {
        self.areas.last().expect("a layout area is active")
    }

    fn area_mut(&mut self) -> &mut Area {
        self.areas.last_mut().expect("a layout area is active")
    }

    fn push_frame(&mut self) {
        self.frames.push(SpanFrame::default());
    }

    fn pop_frame(&mut self) {
        self.frames.pop();
    }

    fn begin_span(&mut self, offset: f32, size: f32, direct: bool) {
        let span = Span {
            instance: self.instances.len(),
            overlay: self.overlay.len(),
            hit: self.registry.hit_count(),
            wheel: self.registry.wheel_count(),
            offset,
            size,
            direct,
        };
        let frame = self.frames.last_mut().expect("a layout frame is active");
        if let Some(open) = frame.open.replace(span) {
            frame.spans.push(open);
        }
    }

    fn end_span(&mut self, size: Option<f32>) {
        let frame = self.frames.last_mut().expect("a layout frame is active");
        let Some(mut open) = frame.open.take() else {
            return;
        };
        if let Some(size) = size {
            open.size = size;
        }
        frame.spans.push(open);
    }

    fn settle_frame(
        &mut self,
        axis: Axis,
        align: Align,
        justify: Align,
        auto_cross: bool,
        content_cross: f32,
        main_space: f32,
    ) {
        let Some(frame) = self.frames.pop() else {
            return;
        };

        let mut spans = frame.spans;
        if let Some(open) = frame.open {
            spans.push(open);
        }
        let instance_end = self.instances.len();
        let overlay_end = self.overlay.len();
        let hit_end = self.registry.hit_count();
        let wheel_end = self.registry.wheel_count();
        for index in 0..spans.len() {
            let span = spans[index];
            let cross = if auto_cross && !span.direct {
                (content_cross - span.size) * cross_factor(align) - span.offset
            } else {
                0.0
            };
            let main = main_space * cross_factor(justify);
            let delta = match axis {
                Axis::Row => Vec2::new(main, cross),
                Axis::Column => Vec2::new(cross, main),
            };
            if delta == Vec2::ZERO {
                continue;
            }
            let next = spans.get(index + 1);
            let instance_stop = next.map_or(instance_end, |span| span.instance);
            let overlay_stop = next.map_or(overlay_end, |span| span.overlay);
            let hit_stop = next.map_or(hit_end, |span| span.hit);
            let wheel_stop = next.map_or(wheel_end, |span| span.wheel);
            for instance in &mut self.instances[span.instance..instance_stop] {
                instance.translate(delta);
            }
            for instance in &mut self.overlay[span.overlay..overlay_stop] {
                instance.translate(delta);
            }
            self.registry.shift_hits(span.hit..hit_stop, delta);
            self.registry.shift_wheels(span.wheel..wheel_stop, delta);
        }
    }

    pub fn available(&self) -> Vec2 {
        self.area().remaining()
    }

    pub fn allocate(&mut self, size: Vec2) -> Rect {
        self.allocate_with(size, false)
    }

    pub fn allocate_fill(&mut self) -> Rect {
        self.allocate_with(Vec2::ZERO, true)
    }

    pub fn allocate_full(&mut self, height: f32) -> Rect {
        let width = self.area().remaining().x;
        self.allocate(Vec2::new(width, height))
    }

    pub fn allocate_sized(&mut self, width: Length, height: f32) -> Rect {
        let available = self.area().remaining().x;
        let width = match width {
            Length::Auto | Length::Fill => available,
            Length::Fixed(value) => value.min(available),
            Length::Maximum(maximum) => available.min(maximum),
        };
        self.allocate(Vec2::new(width, height))
    }

    pub fn open_state(&self, key: u64, fallback: bool) -> bool {
        self.registry.open_state(key).unwrap_or(fallback)
    }

    pub fn set_open_state(&mut self, key: u64, open: bool) {
        self.registry.set_open_state(key, open);
    }

    pub fn disclosure_open(&mut self, id: &str, fallback: bool) -> bool {
        let key = self.id(id);
        self.open_state(key, fallback)
    }

    pub fn store_open(&mut self, id: &str, open: bool) {
        let key = self.id(id);
        self.set_open_state(key, open);
    }

    fn allocate_with(&mut self, size: Vec2, fill: bool) -> Rect {
        let area = self.area();
        let axis = area.axis;
        let remaining = area.remaining();
        let width = if fill && axis == Axis::Column {
            remaining.x
        } else {
            size.x.min(remaining.x).max(0.0)
        };
        let height = if fill && axis == Axis::Row {
            remaining.y
        } else {
            size.y
        };
        let size = Vec2::new(width, height);
        let align = area.align;
        let origin = match axis {
            Axis::Column => Vec2::new(
                area.rect.min.x + (area.rect.width() - size.x) * cross_factor(align),
                area.cursor.y,
            ),
            Axis::Row => Vec2::new(
                area.cursor.x,
                area.rect.min.y + (area.rect.height() - size.y) * cross_factor(align),
            ),
        };
        let offset = match axis {
            Axis::Column => origin.x - area.rect.min.x,
            Axis::Row => origin.y - area.rect.min.y,
        };
        let rect = Rect::from_min_size(origin, size);
        self.begin_span(
            offset,
            match axis {
                Axis::Column => size.x,
                Axis::Row => size.y,
            },
            false,
        );
        let area = self.area_mut();
        match axis {
            Axis::Column => {
                area.cursor.y += size.y + area.gap;
                area.cross = area.cross.max(size.x);
                area.main = (area.cursor.y - area.origin.y - area.gap).max(0.0);
            }
            Axis::Row => {
                area.cursor.x += size.x + area.gap;
                area.cross = area.cross.max(size.y);
                area.main = (area.cursor.x - area.origin.x - area.gap).max(0.0);
            }
        }
        rect
    }

    pub fn space(&mut self, amount: f32) {
        let area = self.area_mut();
        match area.axis {
            Axis::Column => {
                area.cursor.y += amount;
                area.main = area.cursor.y - area.origin.y;
            }
            Axis::Row => {
                area.cursor.x += amount;
                area.main = area.cursor.x - area.origin.x;
            }
        }
    }

    pub fn block(&mut self, axis: Axis, block: Block, content: impl FnOnce(&mut Self)) -> Rect {
        let parent = *self.area();
        let remaining = parent.remaining();
        let width = resolve(block.width, remaining.x);
        let height = resolve(block.height, remaining.y);
        let origin = match parent.axis {
            Axis::Column => Vec2::new(
                parent.rect.min.x + (parent.rect.width() - width) * cross_factor(parent.align),
                parent.cursor.y,
            ),
            Axis::Row => Vec2::new(
                parent.cursor.x,
                parent.rect.min.y + (parent.rect.height() - height) * cross_factor(parent.align),
            ),
        };
        let offset = match parent.axis {
            Axis::Column => origin.x - parent.rect.min.x,
            Axis::Row => origin.y - parent.rect.min.y,
        };
        let initial_cross = match axis {
            Axis::Row => height,
            Axis::Column => width,
        };
        self.begin_span(offset, initial_cross, false);
        let outer = Rect::from_min_size(origin, Vec2::new(width, height));
        let inner = block.padding.shrink(outer);
        let inner = match (axis, block.width, block.height) {
            (Axis::Row, _, Length::Auto) => {
                Rect::from_min_size(inner.min, Vec2::new(inner.width(), 0.0))
            }
            (Axis::Column, Length::Auto, _) => {
                Rect::from_min_size(inner.min, Vec2::new(0.0, inner.height()))
            }
            _ => inner,
        };
        let clip = if block.clip {
            parent.clip.intersect(
                outer,
                block.background.map_or(Corners::ZERO, |look| look.corners),
            )
        } else {
            parent.clip
        };
        let index = self.instances.len();
        self.areas.push(Area {
            rect: inner,
            cursor: inner.min,
            axis,
            gap: block.gap,
            align: block.align,
            clip,
            origin: inner.min,
            cross: 0.0,
            main: 0.0,
        });
        self.push_frame();
        self.begin_span(0.0, 0.0, true);
        content(self);
        let area = self.areas.pop().expect("a layout area is active");
        let auto_cross = match axis {
            Axis::Row => matches!(block.height, Length::Auto),
            Axis::Column => matches!(block.width, Length::Auto),
        };
        let auto_main = match axis {
            Axis::Row => matches!(block.width, Length::Auto),
            Axis::Column => matches!(block.height, Length::Auto),
        };
        let inner_main = match axis {
            Axis::Row => area.rect.width(),
            Axis::Column => area.rect.height(),
        };
        let main_space = if auto_main {
            0.0
        } else {
            (inner_main - area.main).max(0.0)
        };
        self.settle_frame(
            axis,
            block.align,
            block.justify,
            auto_cross,
            area.cross,
            main_space,
        );
        let measured = area.content_size() + block.padding.size();
        let width = match block.width {
            Length::Auto => measured.x.min(remaining.x),
            Length::Maximum(maximum) => measured.x.min(maximum).min(remaining.x),
            Length::Fill | Length::Fixed(_) => width,
        };
        let height = match block.height {
            Length::Auto | Length::Maximum(_) => measured.y,
            Length::Fill | Length::Fixed(_) => height,
        };
        let rect = Rect::from_min_size(origin, Vec2::new(width, height));
        {
            let parent = self.area_mut();
            match parent.axis {
                Axis::Column => {
                    parent.cursor.y += rect.height() + parent.gap;
                    parent.cross = parent.cross.max(rect.width());
                    parent.main = (parent.cursor.y - parent.origin.y - parent.gap).max(0.0);
                }
                Axis::Row => {
                    parent.cursor.x += rect.width() + parent.gap;
                    parent.cross = parent.cross.max(rect.height());
                    parent.main = (parent.cursor.x - parent.origin.x - parent.gap).max(0.0);
                }
            }
        }
        if let Some(look) = block.background {
            self.paint_background(index, rect, look);
        }
        let final_cross = match axis {
            Axis::Row => height,
            Axis::Column => width,
        };
        self.end_span(Some(final_cross));
        rect
    }

    pub fn column(&mut self, block: Block, content: impl FnOnce(&mut Self)) -> Rect {
        self.block(Axis::Column, block, content)
    }

    pub fn row(&mut self, block: Block, content: impl FnOnce(&mut Self)) -> Rect {
        self.block(Axis::Row, block, content)
    }

    pub fn place(&mut self, rect: Rect, content: impl FnOnce(&mut Self)) {
        let parent = *self.area();
        self.areas.push(Area {
            rect,
            cursor: rect.min,
            axis: Axis::Column,
            gap: 0.0,
            align: Align::Start,
            clip: parent.clip,
            origin: rect.min,
            cross: 0.0,
            main: 0.0,
        });
        self.push_frame();
        content(self);
        self.pop_frame();
        self.areas.pop();
    }

    pub fn stacked(&mut self, rect: Rect, clip: bool, content: impl FnOnce(&mut Self)) {
        let parent = *self.area();
        self.areas.push(Area {
            rect,
            cursor: rect.min,
            axis: Axis::Column,
            gap: 0.0,
            align: Align::Start,
            clip: if clip {
                parent.clip.intersect(rect, Corners::ZERO)
            } else {
                parent.clip
            },
            origin: rect.min,
            cross: 0.0,
            main: 0.0,
        });
        self.push_frame();
        content(self);
        self.pop_frame();
        self.areas.pop();
    }

    pub fn clip(&self) -> Clip {
        self.area().clip
    }

    pub fn painter(&mut self) -> Painter<'_> {
        let clip = self.area().clip;
        let dark = self.theme.dark;
        if self.overlay_mode > 0 {
            Painter::new(&mut self.overlay, &mut self.text, clip, dark)
        } else {
            Painter::new(&mut self.instances, &mut self.text, clip, dark)
        }
    }

    pub fn painter_in(&mut self, clip: Clip) -> Painter<'_> {
        let dark = self.theme.dark;
        Painter::new(&mut self.instances, &mut self.text, clip, dark)
    }

    pub fn overlay_painter(&mut self) -> Painter<'_> {
        let clip = self.overlay_clip;
        let dark = self.theme.dark;
        Painter::new(&mut self.overlay, &mut self.text, clip, dark)
    }

    fn paint_background(&mut self, index: usize, rect: Rect, look: Look) {
        if rect.is_empty() {
            return;
        }
        let clip = self.area().clip;
        let dark = self.theme.dark;
        if self.overlay_mode > 0 {
            let mut painter = Painter::new(&mut self.overlay, &mut self.text, clip, dark);
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
            return;
        }
        let start = self.instances.len();
        {
            let mut painter = Painter::new(&mut self.instances, &mut self.text, clip, dark);
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        let added = self.instances.len() - start;
        if added > 0 {
            self.instances[index..].rotate_right(added);
        }
    }

    pub fn label(&mut self, text: &str, level: Level) -> Rect {
        let style = TextStyle::new(self.theme.font(level), self.theme.text(level));
        self.text_style(text, style)
    }

    pub fn text_style(&mut self, text: &str, style: TextStyle) -> Rect {
        let available = self.area().remaining().x;
        let width = if style.wrap { available } else { 0.0 };
        let layout = self.layout_text(text, style, width);
        let size = Vec2::new(
            if style.align == Align::Start {
                layout.size.x
            } else {
                available
            },
            layout.size.y,
        );
        let rect = self.allocate(size);
        let mut painter = self.painter();
        painter.text(&layout, rect.min, style.color);
        rect
    }

    pub fn text_style_wrapped(&mut self, text: &str, style: TextStyle, width: f32) -> Rect {
        let layout = self.layout_text(text, style, width);
        let rect = self.allocate(layout.size);
        let mut painter = self.painter();
        painter.text(&layout, rect.min, style.color);
        rect
    }

    pub fn overlay_scope(&mut self, rect: Rect, content: impl FnOnce(&mut Self)) -> f32 {
        let clip = self.overlay_clip.intersect(rect, Corners::ZERO);
        self.overlay_mode += 1;
        self.areas.push(Area {
            rect,
            cursor: rect.min,
            axis: Axis::Column,
            gap: 0.0,
            align: Align::Start,
            clip,
            origin: rect.min,
            cross: 0.0,
            main: 0.0,
        });
        self.push_frame();
        content(self);
        self.pop_frame();
        let area = self.areas.pop().expect("a layout area is active");
        self.overlay_mode -= 1;
        area.main
    }

    pub fn text_in(&mut self, rect: Rect, text: &str, style: TextStyle, vertical: Align) -> Rect {
        let layout = self.layout_text(text, style, rect.width());
        let origin = Vec2::new(
            rect.min.x,
            rect.min.y + (rect.height() - layout.size.y) * cross_factor(vertical),
        );
        let mut painter = self.painter();
        painter.text(&layout, origin, style.color);
        rect
    }

    pub fn cursor_position(&self) -> Vec2 {
        self.area().cursor
    }

    pub fn pointer_in(&self, rect: Rect) -> Option<Vec2> {
        let pointer = self.input.position();
        (self.input.inside() && rect.contains(pointer)).then_some(pointer)
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.registry.set_cursor(cursor);
    }

    pub fn spring_to(&mut self, key: u64, target: f32, motion: Motion) -> f32 {
        self.registry
            .spring(mix(key, PROPERTY_SALT), target, motion, self.elapsed)
    }

    pub fn spring_at(&self, key: u64, fallback: f32) -> f32 {
        self.registry
            .spring_value(mix(key, PROPERTY_SALT), fallback)
    }

    pub fn overlay_len(&self) -> usize {
        self.overlay.len()
    }

    pub fn overlay_background_at(&mut self, index: usize, rect: Rect, look: Look) {
        if rect.is_empty() {
            return;
        }
        let clip = self.overlay_clip;
        let dark = self.theme.dark;
        let start = self.overlay.len();
        {
            let mut painter = Painter::new(&mut self.overlay, &mut self.text, clip, dark);
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
        }
        let added = self.overlay.len() - start;
        if added > 0 {
            self.overlay[index..].rotate_right(added);
        }
    }

    pub fn value_of(&self, key: u64) -> Option<f32> {
        self.registry.value(key)
    }

    pub fn store_value(&mut self, key: u64, value: f32) {
        self.registry.set_value(key, value);
    }
    pub fn height_of(&self, key: u64) -> Option<f32> {
        self.registry.height(key)
    }

    pub fn store_height(&mut self, key: u64, height: f32) {
        self.registry.set_height(key, height);
    }

    pub fn layout_text(
        &mut self,
        text: &str,
        style: TextStyle,
        width: f32,
    ) -> std::sync::Arc<TextLayout> {
        let wrap = style.wrap;
        let max = if wrap || style.align != Align::Start {
            width
        } else {
            0.0
        };
        self.text
            .layout(text, style.font, max, wrap, style.truncate, style.align)
    }

    pub fn measure_text(&mut self, text: &str, style: TextStyle) -> Vec2 {
        self.layout_text(text, style, 0.0).size
    }

    pub fn separator(&mut self) {
        let thickness = self.theme.metrics.hairline;
        let rect = self.allocate(Vec2::new(0.0, thickness + self.theme.metrics.gap));
        let line = Rect::from_min_size(
            Vec2::new(rect.min.x, rect.center().y),
            Vec2::new(rect.width(), thickness),
        );
        let color = self.theme.palette.separator;
        let mut painter = self.painter();
        painter.rect(line, Corners::ZERO, color);
    }

    pub fn id(&self, salt: &str) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for id in &self.ids {
            hash = mix(hash, *id);
        }
        mix(hash, hash_string(salt))
    }

    pub fn push_id(&mut self, salt: &str) {
        let id = self.id(salt);
        self.ids.push(id);
    }

    pub fn pop_id(&mut self) {
        self.ids.pop();
    }

    pub fn register_occluder(&mut self, rect: Rect) {
        self.registry.register_occluder(rect);
        self.occluders.push(rect);
    }

    pub fn pointer_over_chrome(&self) -> bool {
        let pointer = self.input.position();
        self.registry
            .occluders()
            .iter()
            .any(|rect| rect.contains(pointer))
    }

    pub fn pointer_over_ui(&self) -> bool {
        if self.input.button(Button::Primary).down {
            return true;
        }
        let pointer = self.input.position();
        self.registry
            .hits()
            .iter()
            .any(|hit| hit.clip.contains(pointer) && hit.rect.contains(pointer))
    }

    pub fn interact(&mut self, key: u64, rect: Rect) -> Response {
        let clip = self.area().clip.rect;
        if !rect.is_empty() && clip.intersects(rect) {
            self.registry.register(key, rect, clip);
        }
        let hovered = self.registry.hovered(key);
        let pressed = self.registry.pressed(key);
        let active = self.registry.active(key);
        let clicked = self.registry.clicked(key);
        let dragging = self.registry.dragging(key);
        if hovered {
            self.registry.set_cursor(Cursor::Pointing);
        }
        Response {
            rect,
            hovered,
            pressed,
            active,
            clicked,
            dragging,
        }
    }

    pub fn hovered(&self, key: u64) -> bool {
        self.registry.hovered(key)
    }

    pub fn animate(&mut self, key: u64, target: f32, motion: Motion) -> f32 {
        self.registry
            .spring(mix(key, PROPERTY_SALT), target, motion, self.elapsed)
    }

    pub fn look(&mut self, key: u64, role: Role, response: &Response) -> Look {
        let hover = self.animate(
            key,
            if response.hovered { 1.0 } else { 0.0 },
            Motion::quick(),
        );
        let press = self.animate(
            key,
            if response.active { 1.0 } else { 0.0 },
            Motion::quick(),
        );
        let rest = self.theme.look(role, State::Rest);
        let hovered = self.theme.look(role, State::Hover);
        let pressed = self.theme.look(role, State::Pressed);
        rest.lerp(hovered, hover).lerp(pressed, press)
    }

    pub fn look_disabled(&mut self, role: Role) -> Look {
        self.theme.look(role, State::Disabled)
    }

    pub fn radius(&self) -> Corners {
        Corners::all(self.theme.metrics.radius)
    }

    pub fn scroll(&mut self, key: u64, height: Length, content: impl FnOnce(&mut Self)) -> Rect {
        self.scroll_axis(key, Axis::Column, height, None, content)
    }

    pub fn scroll_axis(
        &mut self,
        key: u64,
        axis: Axis,
        length: Length,
        content_length: Option<f32>,
        content: impl FnOnce(&mut Self),
    ) -> Rect {
        let available = self.area().remaining();
        let outer = match axis {
            Axis::Column => self.allocate(Vec2::new(available.x, resolve(length, available.y))),
            Axis::Row => self.allocate(Vec2::new(resolve(length, available.x), available.y)),
        };
        self.registry.register_wheel(key, outer);
        if self.registry.wheel_owner() == Some(key) {
            let wheel = match axis {
                Axis::Column => -self.input.wheel().y,
                Axis::Row => -self.input.wheel().x - self.input.wheel().y,
            };
            if wheel != 0.0 {
                self.registry.scroll(key).scroll_by(wheel * 42.0);
            }
        }
        if let Some(length) = content_length {
            let scroll = self.registry.scroll(key);
            scroll.content = length;
        }
        let offset = self.registry.scroll(key).offset;
        let clip = self.area().clip.intersect(outer, Corners::ZERO);
        let inner = match axis {
            Axis::Column => Rect::from_min_size(
                Vec2::new(outer.min.x, outer.min.y - offset),
                Vec2::new(outer.width(), outer.height() + offset),
            ),
            Axis::Row => Rect::from_min_size(
                Vec2::new(outer.min.x - offset, outer.min.y),
                Vec2::new(outer.width() + offset, outer.height()),
            ),
        };
        self.areas.push(Area {
            rect: inner,
            cursor: inner.min,
            axis,
            gap: 0.0,
            align: Align::Start,
            clip,
            origin: inner.min,
            cross: 0.0,
            main: 0.0,
        });
        self.push_frame();
        content(self);
        self.pop_frame();
        let area = self.areas.pop().expect("a layout area is active");
        let measured = area.main;
        let viewport = match axis {
            Axis::Column => outer.height(),
            Axis::Row => outer.width(),
        };
        let scroll = self.registry.scroll(key);
        scroll.viewport = viewport;
        scroll.content = content_length.unwrap_or(measured).max(viewport);
        scroll.target = scroll.target.clamp(0.0, scroll.limit());
        let bar = (scroll.bar, scroll.limit());
        if bar.0 > 0.01 && bar.1 > 0.0 {
            self.scrollbar(key, outer, axis, bar.0);
        }
        self.registry.register(key, outer, self.area().clip.rect);
        outer
    }

    pub fn scroll_info(&self, key: u64) -> ScrollInfo {
        let scroll = self.registry.scroll_state(key);
        ScrollInfo {
            offset: scroll.offset,
            viewport: scroll.viewport,
            content: scroll.content,
            limit: scroll.limit(),
        }
    }

    fn scrollbar(&mut self, key: u64, outer: Rect, axis: Axis, bar: f32) {
        let scroll = self.registry.scroll_state(key);
        let (track, thumb) = match axis {
            Axis::Column => {
                let track = Rect::from_min_size(
                    Vec2::new(outer.max.x - 7.0, outer.min.y + 4.0),
                    Vec2::new(3.0, (outer.height() - 8.0).max(8.0)),
                );
                let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
                let thumb_height = (track.height() * ratio).max(24.0).min(track.height());
                let travel = track.height() - thumb_height;
                let position = if scroll.limit() > 0.0 {
                    (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (
                    track,
                    Rect::from_min_size(
                        Vec2::new(track.min.x, track.min.y + travel * position),
                        Vec2::new(track.width(), thumb_height),
                    ),
                )
            }
            Axis::Row => {
                let track = Rect::from_min_size(
                    Vec2::new(outer.min.x + 4.0, outer.max.y - 7.0),
                    Vec2::new((outer.width() - 8.0).max(8.0), 3.0),
                );
                let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
                let thumb_width = (track.width() * ratio).max(24.0).min(track.width());
                let travel = track.width() - thumb_width;
                let position = if scroll.limit() > 0.0 {
                    (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (
                    track,
                    Rect::from_min_size(
                        Vec2::new(track.min.x + travel * position, track.min.y),
                        Vec2::new(thumb_width, track.height()),
                    ),
                )
            }
        };
        let _ = track;
        let color = Color::white_alpha(0.22 * bar);
        let mut painter = self.overlay_painter();
        painter.rect(thumb, Corners::capsule(), color);
    }

    pub fn pointer_captured(&self) -> bool {
        self.registry.captured()
    }

    pub fn wake(&mut self) {
        self.registry.wake();
    }

    pub fn appeared(&mut self, key: u64, seconds: f32) -> f32 {
        let since = self.registry.value(key);
        let now = self.time;
        let start = match since {
            Some(start) => start,
            None => {
                self.registry.set_value(key, now);
                now
            }
        };
        let t = if seconds > 0.0 {
            ((now - start) / seconds).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if t < 1.0 {
            self.registry.wake();
        }
        1.0 - (1.0 - t).powi(3)
    }

    pub fn popup_open(&self, key: u64) -> bool {
        self.registry.popup() == Some(key)
    }

    pub fn toggle_popup(&mut self, key: u64) {
        if self.popup_open(key) {
            self.registry.close_popup();
        } else {
            self.registry.open_popup(key);
        }
    }

    pub fn close_popup(&mut self) {
        self.registry.close_popup();
    }

    pub fn popup_interacted(&self) -> bool {
        self.registry.popup_interacted()
    }

    pub fn tooltip(&mut self, text: &str) {
        self.registry.request_tooltip(text);
    }

    pub fn paint_tooltip(&mut self) {
        let Some(text) = self.registry.tooltip().map(str::to_string) else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let style = TextStyle::new(self.theme.font(Level::Caption), Color::WHITE);
        let layout = self.layout_text(&text, style, 0.0);
        let pointer = self.input.position();
        let size = layout.size + Vec2::new(18.0, 12.0);
        let mut origin = pointer + Vec2::new(14.0, 18.0);
        if origin.x + size.x > self.size.x - 8.0 {
            origin.x = self.size.x - 8.0 - size.x;
        }
        if origin.y + size.y > self.size.y - 8.0 {
            origin.y = pointer.y - 14.0 - size.y;
        }
        let rect = Rect::from_min_size(origin, size);
        let look = self.theme.look(Role::Popover, State::Rest);
        let clip = self.overlay_clip;
        let dark = self.theme.dark;
        {
            let mut painter = Painter::new(&mut self.overlay, &mut self.text, clip, dark);
            painter.outlined(rect, look.corners, look.fill, look.border, look.shadow);
            painter.text(
                &layout,
                rect.min + Vec2::new(9.0, 6.0),
                Color::white_alpha(0.94),
            );
        }
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn overlay_instances(&self) -> &[Instance] {
        &self.overlay
    }

    pub fn atlas(&self) -> &crate::atlas::Atlas {
        self.text.atlas()
    }

    pub fn cursor(&self) -> Cursor {
        self.registry.cursor()
    }

    pub fn render_figure(
        &mut self,
        size: Vec2,
        pixels_per_point: f32,
        content: impl FnOnce(&mut Painter),
    ) -> Vec<Instance> {
        self.text.begin(pixels_per_point);
        let clip = Clip::new(Rect::from_min_size(Vec2::ZERO, size), Corners::ZERO);
        let dark = self.theme.dark;
        let mut instances = Vec::new();
        {
            let mut painter = Painter::new(&mut instances, &mut self.text, clip, dark);
            content(&mut painter);
        }
        self.text.begin(self.scale);
        instances
    }

    pub fn description(&self) -> String {
        format!(
            "{} · {} instances",
            self.text.description(),
            self.instances.len()
        )
    }
}

pub fn resolve(length: Length, available: f32) -> f32 {
    match length {
        Length::Auto => available,
        Length::Fill => available,
        Length::Fixed(value) => value,
        Length::Maximum(maximum) => available.min(maximum),
    }
}

fn cross_factor(align: Align) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center => 0.5,
        Align::End => 1.0,
    }
}

fn hash_string(value: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn mix(left: u64, right: u64) -> u64 {
    let mut hash = left ^ right.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash
}
