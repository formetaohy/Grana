use std::hash::{Hash, Hasher};

use glam::Vec2;

pub(crate) use crate::content::{Content, ContentKind};
use crate::geometry::{Corners, Edges, Rect};
use crate::input::{Button, Cursor, Modifiers};
use crate::paint::Painter;
use crate::style::Surface;
use crate::text::{Align, TextStyle};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(pub u64);

impl Key {
    pub const NONE: Self = Self(0);

    pub fn new(name: impl AsRef<str>) -> Self {
        Self(hash(name.as_ref().as_bytes()))
    }

    pub fn index(name: impl AsRef<str>, index: usize) -> Self {
        Self(mix(hash(name.as_ref().as_bytes()), index as u64))
    }

    pub fn then(self, name: &str) -> Self {
        Self(mix(self.0, hash(name.as_bytes())))
    }

    pub fn structural(index: usize) -> Self {
        Self(mix(0x9e37_79b9_7f4a_7c15, index as u64))
    }

    pub fn id(self) -> u64 {
        self.0
    }
}

fn hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
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

#[derive(Default)]
struct CanvasHasher(u64);

impl Hasher for CanvasHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = mix(self.0 ^ 0x9e37_79b9_7f4a_7c15, value);
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.write_u64(u64::from(value));
    }

    fn write_u8(&mut self, value: u8) {
        self.write_u64(u64::from(value));
    }

    fn write_usize(&mut self, value: usize) {
        self.write_u64(value as u64);
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_i32(&mut self, value: i32) {
        self.write_u64(value as u64);
    }

    fn write_i16(&mut self, value: i16) {
        self.write_u64(value as u64);
    }

    fn write_i8(&mut self, value: i8) {
        self.write_u64(value as u64);
    }

    fn write_isize(&mut self, value: isize) {
        self.write_u64(value as u64);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasKey(u64);

impl CanvasKey {
    pub const FRAME: Self = Self(0);

    pub const LAYOUT: Self = Self(1);

    pub fn new(parts: impl Hash) -> Self {
        let mut hasher = CanvasHasher(0xcbf2_9ce4_8422_2325);
        parts.hash(&mut hasher);
        Self(hasher.finish().max(2))
    }

    pub fn then(self, parts: impl Hash) -> Self {
        let mut hasher = CanvasHasher(self.0);
        parts.hash(&mut hasher);
        Self(hasher.finish().max(2))
    }

    pub fn id(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Auto,
    Fill,
    Fixed(f32),
    Minimum(f32),
    Maximum(f32),
}

impl Length {
    pub fn resolve(self, available: f32) -> f32 {
        match self {
            Self::Auto | Self::Fill => available,
            Self::Fixed(value) => value,
            Self::Minimum(minimum) => available.max(minimum),
            Self::Maximum(maximum) => available.min(maximum),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: Length,
    pub height: Length,
}

impl Size {
    pub const AUTO: Self = Self::new(Length::Auto, Length::Auto);

    pub const fn new(width: Length, height: Length) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Row,
    Column,
    Stack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Row,
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub flow: Flow,
    pub size: Size,
    pub padding: Edges,
    pub gap: f32,
    pub align: Align,
    pub justify: Align,
    pub wrap: bool,
    pub clip: bool,
}

impl Layout {
    pub fn column() -> Self {
        Self {
            flow: Flow::Column,
            size: Size::new(Length::Fill, Length::Auto),
            padding: Edges::ZERO,
            gap: 0.0,
            align: Align::Start,
            justify: Align::Start,
            wrap: false,
            clip: false,
        }
    }

    pub fn row() -> Self {
        Self {
            flow: Flow::Row,
            size: Size::AUTO,
            ..Self::column()
        }
    }

    pub fn stack() -> Self {
        Self {
            flow: Flow::Stack,
            ..Self::row()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scroll {
    pub axis: Axis,
    pub length: Length,
    pub content: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Popup {
    pub anchor: Key,
    pub width: f32,
    pub persistent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

#[derive(Clone, Copy, Debug)]
pub struct Pointer {
    pub position: Vec2,
    pub delta: Vec2,
    pub phase: Phase,
    pub button: Button,
    pub modifiers: Modifiers,
    pub rect: Rect,
}

pub struct CanvasInfo<'a> {
    pub rect: Rect,
    pub theme: &'a Theme,
    pub hover: f32,
    pub press: f32,
    pub engaged: f32,
    pub time: f32,
}

pub type CanvasPaint<'a> = Box<dyn Fn(&mut Painter, CanvasInfo<'_>) + 'a>;

pub(crate) struct Canvas<'a> {
    pub key: CanvasKey,
    pub paint: CanvasPaint<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Content,
    Chrome,
    Popup,
    Hud,
}

impl Layer {
    pub const ALL: [Layer; 4] = [Self::Content, Self::Chrome, Self::Popup, Self::Hud];

    pub(crate) fn index(self) -> usize {
        match self {
            Self::Content => 0,
            Self::Chrome => 1,
            Self::Popup => 2,
            Self::Hud => 3,
        }
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Patch {
    pub layout: bool,
    pub paint: bool,
    pub focus: bool,
    pub fields: bool,
    pub popup: bool,
    pub hits: bool,
    pub animate: bool,
}

pub struct Element<'a, A> {
    pub(crate) id: Key,
    pub(crate) layout: Layout,
    pub(crate) content: Content,
    pub(crate) interaction: Interaction<A>,
    pub(crate) canvas: Option<Canvas<'a>>,
    pub(crate) children: Vec<Element<'a, A>>,
    pub(crate) background: Option<Surface>,
    pub(crate) layer: Option<Layer>,
    pub(crate) position: Option<Rect>,
    pub(crate) scroll: Option<Scroll>,
    pub(crate) popup: Option<Popup>,
    pub(crate) tooltip: Option<String>,
    pub(crate) cursor: Option<Cursor>,
    pub(crate) enabled: bool,
    pub(crate) focus_ring: Option<Corners>,
    pub(crate) blocks_pointer: bool,
}

impl<'a, A> Element<'a, A> {
    pub fn new(layout: Layout) -> Self {
        Self {
            id: Key::NONE,
            layout,
            content: Content::Empty,
            interaction: Interaction::None,
            canvas: None,
            children: Vec::new(),
            background: None,
            layer: None,
            position: None,
            scroll: None,
            popup: None,
            tooltip: None,
            cursor: None,
            enabled: true,
            focus_ring: None,
            blocks_pointer: false,
        }
    }

    pub fn column() -> Self {
        Self::new(Layout::column())
    }

    pub fn row() -> Self {
        Self::new(Layout::row())
    }

    pub fn key(mut self, key: Key) -> Self {
        self.id = key;
        self
    }

    pub fn named(self, name: &str) -> Self {
        self.key(Key::new(name))
    }

    pub fn width(mut self, width: Length) -> Self {
        self.layout.size.width = width;
        if let Content::Button(button) = &mut self.content {
            button.width = width;
        }
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.layout.size.height = height;
        if let (Content::Button(button), Length::Fixed(value)) = (&mut self.content, height) {
            button.height = value;
        }
        self
    }

    pub fn padding(mut self, padding: Edges) -> Self {
        self.layout.padding = padding;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.layout.gap = gap;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.layout.align = align;
        self
    }

    pub fn justify(mut self, justify: Align) -> Self {
        self.layout.justify = justify;
        self
    }

    pub fn wrap(mut self) -> Self {
        self.layout.wrap = true;
        self
    }

    pub fn clipped(mut self) -> Self {
        self.layout.clip = true;
        self
    }

    pub fn background(mut self, surface: Surface) -> Self {
        self.background = Some(surface);
        self
    }

    pub fn layer(mut self, layer: Layer) -> Self {
        self.layer = Some(layer);
        self
    }

    pub fn at(mut self, rect: Rect) -> Self {
        self.position = Some(rect);
        self
    }

    pub fn scroll(mut self, axis: Axis, length: Length, content: Option<f32>) -> Self {
        self.scroll = Some(Scroll {
            axis,
            length,
            content,
        });
        self
    }

    pub fn popup(mut self, anchor: Key, width: f32) -> Self {
        self.popup = Some(Popup {
            anchor,
            width,
            persistent: false,
        });
        self
    }

    pub fn persistent(mut self) -> Self {
        if let Some(popup) = self.popup.as_mut() {
            popup.persistent = true;
        }
        self
    }

    pub fn tooltip(mut self, tooltip: &str) -> Self {
        self.tooltip = (!tooltip.is_empty()).then(|| tooltip.to_string());
        self
    }

    pub fn cursor(mut self, cursor: Cursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn focus_ring(mut self, corners: Corners) -> Self {
        self.focus_ring = Some(corners);
        self
    }

    pub fn blocks_pointer(mut self) -> Self {
        self.blocks_pointer = true;
        self
    }

    pub fn shaped(mut self, corners: Corners) -> Self {
        match &mut self.content {
            Content::Button(button) => button.corners = Some(corners),
            Content::IconButton(icon) => icon.corners = Some(corners),
            _ => {}
        }
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = Element<'a, A>>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    pub fn paint(mut self, paint: impl Fn(&mut Painter, CanvasInfo<'_>) + 'a) -> Self {
        self.canvas = Some(Canvas {
            key: CanvasKey::FRAME,
            paint: Box::new(paint),
        });
        self.content = Content::Canvas;
        self
    }

    pub fn cached(mut self, key: CanvasKey) -> Self {
        let canvas = self
            .canvas
            .as_mut()
            .expect("a cached element paints a canvas");
        canvas.key = key;
        self
    }

    pub fn press(mut self, action: A) -> Self {
        self.interaction = Interaction::Press(action);
        self
    }

    pub fn on_toggle(mut self, action: impl Fn(bool) -> A + 'static) -> Self {
        self.interaction = Interaction::Toggle {
            action: Box::new(action),
        };
        self
    }

    pub fn on_slide(mut self, action: impl Fn(f32) -> A + 'static) -> Self {
        self.interaction = Interaction::Slider {
            action: Box::new(action),
        };
        self
    }

    pub fn on_range(mut self, action: impl Fn(f32, f32) -> A + 'static) -> Self {
        self.interaction = Interaction::RangeSlider {
            action: Box::new(action),
        };
        self
    }

    pub fn on_segment(mut self, action: impl Fn(usize) -> A + 'static) -> Self {
        self.interaction = Interaction::Segment {
            action: Box::new(action),
        };
        self
    }

    pub fn on_drag_value(mut self, speed: f32, action: impl Fn(f32) -> A + 'static) -> Self {
        self.interaction = Interaction::DragValue {
            speed,
            action: Box::new(action),
        };
        self
    }

    pub fn on_commit(mut self, action: impl Fn(&str) -> A + 'static) -> Self {
        self.interaction = Interaction::Field {
            commit: Box::new(action),
            cancel: None,
        };
        self
    }

    pub fn on_cancel(mut self, action: impl Fn() -> A + 'static) -> Self {
        if let Interaction::Field { cancel, .. } = &mut self.interaction {
            *cancel = Some(Box::new(action));
        }
        self
    }

    pub fn editing(mut self, editing: bool) -> Self {
        if let Content::Field(field) = &mut self.content {
            field.editing = editing;
        }
        self
    }

    pub fn on_color(mut self, action: impl Fn([f32; 3]) -> A + 'static) -> Self {
        self.interaction = Interaction::Color {
            action: Box::new(action),
        };
        self
    }

    pub fn on_pointer(mut self, handler: impl Fn(Pointer) -> Option<A> + 'static) -> Self {
        self.interaction = Interaction::Pointer {
            handler: Box::new(handler),
        };
        self
    }

    pub(crate) fn with_content(mut self, content: Content) -> Self {
        self.content = content;
        self
    }

    pub(crate) fn into_node(self, key: Key, layer: Layer) -> crate::node::Node<A> {
        let mut node = crate::node::Node::new(
            key,
            self.content.kind(),
            self.layout,
            self.content,
            self.interaction,
        );
        node.background = self.background;
        node.layer = self.layer.unwrap_or(layer);
        node.layer_explicit = self.layer.is_some();
        node.position = self.position;
        node.scroll_spec = self.scroll;
        node.popup_spec = self.popup;
        node.tooltip = self.tooltip;
        node.cursor = self.cursor;
        node.enabled = self.enabled;
        node.focus_ring = self.focus_ring;
        node.blocks_pointer = self.blocks_pointer;
        node.canvas = self.canvas.as_ref().map(|canvas| canvas.key);
        node
    }

    pub(crate) fn patch(self, node: &mut crate::node::Node<A>) -> Patch {
        let Self {
            id: _,
            layout,
            content,
            interaction,
            canvas: _,
            children: _,
            background,
            layer: _,
            position,
            scroll,
            popup,
            tooltip,
            cursor,
            enabled,
            focus_ring,
            blocks_pointer,
        } = self;
        let mut patch = Patch::default();
        let kind = content.kind();
        let content_changed = node.kind != kind || !node.content.same(&content);
        let layout_changed = node.layout != layout
            || node.position != position
            || node.scroll_spec != scroll
            || node.popup_spec != popup;
        if node.popup_spec != popup {
            patch.popup = true;
        }
        if node.layout != layout
            || node.position != position
            || node.scroll_spec != scroll
            || node.popup_spec != popup
        {
            node.layout = layout;
            node.position = position;
            node.scroll_spec = scroll;
            node.popup_spec = popup;
            patch.layout = true;
            patch.hits = true;
        }
        if content_changed {
            patch.hits = hit_affected(&node.content, &content);
            patch.fields = matches!(content, Content::Field(_));
            patch.animate = animates(&content);
            patch.layout = true;
            patch.paint = true;
            patch.focus = node.kind != kind;
            node.kind = kind;
            node.content = content;
        }
        let focusable = interaction.is_focusable();
        if focusable != node.interaction.is_focusable() {
            patch.focus = true;
            patch.hits = true;
        }
        node.interaction = interaction;
        if node.background != background {
            node.background = background;
            patch.paint = true;
            patch.hits = true;
        }
        if node.tooltip != tooltip {
            node.tooltip = tooltip;
        }
        node.cursor = cursor;
        if node.enabled != enabled {
            node.enabled = enabled;
            patch.paint = true;
            patch.focus = true;
            patch.hits = true;
        }
        if node.focus_ring != focus_ring {
            node.focus_ring = focus_ring;
            patch.paint = true;
        }
        if node.blocks_pointer != blocks_pointer {
            node.blocks_pointer = blocks_pointer;
            patch.hits = true;
        }
        if layout_changed {
            patch.layout = true;
            patch.hits = true;
        }
        if patch.layout {
            patch.animate |= animates(&node.content);
        }
        patch
    }
}

pub(crate) fn animates(content: &Content) -> bool {
    matches!(
        content,
        Content::Toggle(_)
            | Content::Checkbox(_)
            | Content::Slider(_)
            | Content::RangeSlider(_)
            | Content::Segment(_)
            | Content::Tabs(_)
            | Content::ColorPicker(_)
            | Content::Spinner(_)
            | Content::Progress(_)
    )
}

fn hit_affected(before: &Content, after: &Content) -> bool {
    match (before, after) {
        (Content::Slider(a), Content::Slider(b)) => a.label != b.label,
        _ => true,
    }
}

pub enum Interaction<A> {
    None,
    Press(A),
    Toggle {
        action: Box<dyn Fn(bool) -> A>,
    },
    Slider {
        action: Box<dyn Fn(f32) -> A>,
    },
    RangeSlider {
        action: Box<dyn Fn(f32, f32) -> A>,
    },
    Segment {
        action: Box<dyn Fn(usize) -> A>,
    },
    DragValue {
        speed: f32,
        action: Box<dyn Fn(f32) -> A>,
    },
    Field {
        commit: Box<dyn Fn(&str) -> A>,
        cancel: Option<Box<dyn Fn() -> A>>,
    },
    Pointer {
        handler: Box<dyn Fn(Pointer) -> Option<A>>,
    },
    Color {
        action: Box<dyn Fn([f32; 3]) -> A>,
    },
}

impl<A> Interaction<A> {
    pub(crate) fn is_interactive(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub(crate) fn is_focusable(&self) -> bool {
        matches!(
            self,
            Self::Press(_)
                | Self::Toggle { .. }
                | Self::Slider { .. }
                | Self::RangeSlider { .. }
                | Self::Segment { .. }
                | Self::DragValue { .. }
                | Self::Field { .. }
                | Self::Color { .. }
        )
    }
}

pub fn text<'a, A>(value: impl Into<String>, style: TextStyle) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::AUTO,
        ..Layout::row()
    })
    .with_content(Content::Text(crate::content::TextContent {
        text: value.into(),
        style,
    }))
}

pub fn canvas<'a, A>(
    width: Length,
    height: f32,
    paint: impl Fn(&mut Painter, CanvasInfo<'_>) + 'a,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(width, Length::Fixed(height)),
        ..Layout::column()
    })
    .paint(paint)
}

pub fn spacer<'a, A>(size: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(0.0), Length::Fixed(size)),
        ..Layout::column()
    })
    .with_content(Content::Empty)
}

pub fn place<'a, A>(
    rect: Rect,
    children: impl IntoIterator<Item = Element<'a, A>>,
) -> Element<'a, A> {
    Element::new(Layout {
        flow: Flow::Column,
        size: Size::new(Length::Fixed(rect.width()), Length::Fixed(rect.height())),
        ..Layout::column()
    })
    .at(rect)
    .children(children)
}

pub fn stack<'a, A>(
    rect: Rect,
    clip: bool,
    children: impl IntoIterator<Item = Element<'a, A>>,
) -> Element<'a, A> {
    let mut element = place(rect, children);
    element.layout.clip = clip;
    element
}

pub fn align_factor(align: Align) -> f32 {
    match align {
        Align::Start | Align::Baseline => 0.0,
        Align::Center => 0.5,
        Align::End => 1.0,
    }
}

pub fn column<'a, A>() -> Element<'a, A> {
    Element::column()
}

pub fn row<'a, A>() -> Element<'a, A> {
    Element::row()
}
