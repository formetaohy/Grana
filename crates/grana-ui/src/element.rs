use std::ops::RangeInclusive;

use glam::Vec2;

use crate::color::Color;
use crate::geometry::{Corners, Edges, Rect};
use crate::glass::GlassStyle;
use crate::icon::Icon;
use crate::input::{Button, Cursor, Modifiers};
use crate::paint::Painter;
use crate::style::Surface;
use crate::text::{Align, TextStyle};
use crate::theme::{Role, Theme, TypeStyle};

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

    pub fn id(self) -> u64 {
        self.0
    }

    pub(crate) fn structural(index: usize) -> Self {
        Self(mix(0x9e37_79b9_7f4a_7c15, index as u64))
    }
}

pub fn hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub fn mix(left: u64, right: u64) -> u64 {
    let mut hash = left ^ right.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash
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
pub struct Glass {
    pub style: GlassStyle,
    pub group: Option<Key>,
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
}

pub struct CanvasInfo<'a> {
    pub rect: Rect,
    pub glass: Option<Rect>,
    pub theme: &'a Theme,
    pub hover: f32,
    pub press: f32,
    pub engaged: f32,
    pub time: f32,
    pub scale: f32,
}

pub type CanvasPaint<'a> = Box<dyn Fn(&mut Painter, CanvasInfo<'_>) + 'a>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Content,
    Chrome,
    Labels,
    Popup,
    Hud,
}

impl Layer {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::Content => 0,
            Self::Chrome => 1,
            Self::Labels => 2,
            Self::Popup => 3,
            Self::Hud => 4,
        }
    }
}

pub struct Element<'a, A> {
    pub(crate) id: Key,
    pub(crate) layout: Layout,
    pub(crate) content: Content,
    pub(crate) interaction: Interaction<A>,
    pub(crate) canvas: Option<CanvasPaint<'a>>,
    pub(crate) revision: u64,
    pub(crate) children: Vec<Element<'a, A>>,
    pub(crate) background: Option<Surface>,
    pub(crate) glass: Option<Glass>,
    pub(crate) layer: Option<Layer>,
    pub(crate) position: Option<Rect>,
    pub(crate) scroll: Option<Scroll>,
    pub(crate) popup: Option<Popup>,
    pub(crate) tooltip: Option<String>,
    pub(crate) cursor: Option<Cursor>,
    pub(crate) occluder: bool,
    pub(crate) enabled: bool,
    pub(crate) focus_ring: Option<Corners>,
}

impl<'a, A> Element<'a, A> {
    pub fn new(layout: Layout) -> Self {
        Self {
            id: Key::NONE,
            layout,
            content: Content::Empty,
            interaction: Interaction::None,
            canvas: None,
            revision: 0,
            children: Vec::new(),
            background: None,
            glass: None,
            layer: None,
            position: None,
            scroll: None,
            popup: None,
            tooltip: None,
            cursor: None,
            occluder: false,
            enabled: true,
            focus_ring: None,
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
        self
    }

    pub fn height(mut self, height: Length) -> Self {
        self.layout.size.height = height;
        self
    }

    pub fn size(mut self, width: Length, height: Length) -> Self {
        self.layout.size = Size::new(width, height);
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

    pub fn glass(mut self, style: GlassStyle) -> Self {
        self.glass = Some(Glass { style, group: None });
        self
    }

    pub fn glass_group(mut self, group: Key) -> Self {
        if let Some(glass) = self.glass.as_mut() {
            glass.group = Some(group);
        }
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
        self.popup = Some(Popup { anchor, width });
        self
    }

    pub fn tooltip(mut self, tooltip: &str) -> Self {
        self.tooltip = Some(tooltip.to_string());
        self
    }

    pub fn cursor(mut self, cursor: Cursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    pub fn occluder(mut self) -> Self {
        self.occluder = true;
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

    pub fn children(mut self, children: impl IntoIterator<Item = Element<'a, A>>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    pub fn child(mut self, child: Element<'a, A>) -> Self {
        self.children.push(child);
        self
    }

    pub fn paint(
        mut self,
        revision: u64,
        paint: impl Fn(&mut Painter, CanvasInfo<'_>) + 'a,
    ) -> Self {
        self.canvas = Some(Box::new(paint));
        self.revision = revision;
        self.content = Content::Canvas;
        self
    }

    pub fn press(mut self, action: A) -> Self {
        self.interaction = Interaction::Press {
            action: Some(action),
            focus: true,
        };
        self
    }

    pub fn press_disabled(mut self) -> Self {
        self.interaction = Interaction::Press {
            action: None,
            focus: false,
        };
        self
    }

    pub fn on_toggle(mut self, value: bool, action: impl Fn(bool) -> A + 'static) -> Self {
        self.interaction = Interaction::Toggle {
            value,
            action: Box::new(action),
        };
        self
    }

    pub fn on_slide(
        mut self,
        range: RangeInclusive<f32>,
        logarithmic: bool,
        action: impl Fn(f32) -> A + 'static,
    ) -> Self {
        self.interaction = Interaction::Slider {
            range,
            logarithmic,
            action: Box::new(action),
        };
        self
    }

    pub fn on_range(
        mut self,
        range: RangeInclusive<f32>,
        action: impl Fn(f32, f32) -> A + 'static,
    ) -> Self {
        self.interaction = Interaction::RangeSlider {
            range,
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

    pub fn on_drag_value(
        mut self,
        value: f32,
        speed: f32,
        action: impl Fn(f32) -> A + 'static,
    ) -> Self {
        self.interaction = Interaction::DragValue {
            value,
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

    pub fn on_color(mut self, color: [f32; 3], action: impl Fn([f32; 3]) -> A + 'static) -> Self {
        self.interaction = Interaction::Color {
            color,
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
        let placed = self.position;
        let mut node = crate::node::Node::new(
            key,
            self.content.kind(),
            self.layout,
            self.content,
            self.interaction,
        );
        node.background = self.background;
        node.glass_spec = self.glass;
        node.layer = self.layer.unwrap_or(layer);
        node.layer_explicit = self.layer.is_some();
        node.position = placed;
        node.scroll_spec = self.scroll;
        node.popup_spec = self.popup;
        node.tooltip = self.tooltip;
        node.cursor = self.cursor;
        node.occluder = self.occluder;
        node.enabled = self.enabled;
        node.focus_ring = self.focus_ring;
        node.revision = self.revision;
        node
    }

    pub(crate) fn patch(&mut self, node: &mut crate::node::Node<A>) -> bool {
        let background_changed = node.background != self.background;
        let mut layout_changed = node.layout != self.layout
            || !node.content.same(&self.content)
            || node.position != self.position
            || node.scroll_spec != self.scroll
            || node.popup_spec != self.popup
            || node.enabled != self.enabled;
        if let (Content::Disclosure(old), Content::Disclosure(new)) = (&node.content, &self.content)
            && old.open != new.open
            && new.open != node.runtime.open_prop
        {
            node.runtime.open = new.open;
            node.runtime.open_prop = new.open;
            layout_changed = true;
        }
        if layout_changed {
            node.content = std::mem::replace(&mut self.content, Content::Empty);
            node.layout = self.layout;
            node.position = self.position;
            node.scroll_spec = self.scroll;
            node.popup_spec = self.popup;
            node.enabled = self.enabled;
            node.layout_dirty = true;
        }
        if let Some(layer) = self.layer
            && node.layer != layer
        {
            node.layer = layer;
            node.layer_explicit = true;
        }
        node.interaction = std::mem::replace(&mut self.interaction, Interaction::None);
        node.background = self.background;
        node.glass_spec = self.glass;
        node.tooltip = self.tooltip.take();
        node.cursor = self.cursor;
        node.occluder = self.occluder;
        node.focus_ring = self.focus_ring;
        if node.revision != self.revision || self.canvas.is_some() {
            node.revision = self.revision;
            node.paint_dirty = true;
        }
        if background_changed {
            node.paint_dirty = true;
        }
        layout_changed
    }
}

pub enum Interaction<A> {
    None,
    Press {
        action: Option<A>,
        focus: bool,
    },
    Toggle {
        value: bool,
        action: Box<dyn Fn(bool) -> A>,
    },
    Slider {
        range: RangeInclusive<f32>,
        logarithmic: bool,
        action: Box<dyn Fn(f32) -> A>,
    },
    RangeSlider {
        range: RangeInclusive<f32>,
        action: Box<dyn Fn(f32, f32) -> A>,
    },
    Segment {
        action: Box<dyn Fn(usize) -> A>,
    },
    DragValue {
        value: f32,
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
        color: [f32; 3],
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
            Self::Press { focus: true, .. }
                | Self::Toggle { .. }
                | Self::Slider { .. }
                | Self::RangeSlider { .. }
                | Self::Segment { .. }
                | Self::DragValue { .. }
                | Self::Field { .. }
        )
    }
}

pub enum Content {
    Empty,
    Text(TextContent),
    Canvas,
    Button(ButtonContent),
    IconButton(IconButtonContent),
    Toggle(ToggleContent),
    Checkbox(CheckboxContent),
    Slider(SliderContent),
    RangeSlider(RangeSliderContent),
    Segment(SegmentContent),
    Tabs(TabsContent),
    DragValue(DragValueContent),
    Field(FieldContent),
    Disclosure(DisclosureContent),
    MenuRow(MenuRowContent),
    ActionRow(ActionRowContent),
    Badge(BadgeContent),
    Spinner(SpinnerContent),
    Progress(ProgressContent),
    GradientBar(GradientBarContent),
    Labelled(LabelledContent),
    SectionTitle(String),
    ColorSwatch(ColorSwatchContent),
    ColorPicker(ColorPickerContent),
    Separator,
}

impl Content {
    pub(crate) fn kind(&self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Text(_) => 1,
            Self::Canvas => 2,
            Self::Button(_) => 3,
            Self::IconButton(_) => 4,
            Self::Toggle(_) => 5,
            Self::Checkbox(_) => 6,
            Self::Slider(_) => 7,
            Self::RangeSlider(_) => 8,
            Self::Segment(_) => 9,
            Self::Tabs(_) => 10,
            Self::DragValue(_) => 11,
            Self::Field(_) => 12,
            Self::Disclosure(_) => 13,
            Self::MenuRow(_) => 14,
            Self::ActionRow(_) => 15,
            Self::Badge(_) => 16,
            Self::Spinner(_) => 17,
            Self::Progress(_) => 18,
            Self::GradientBar(_) => 19,
            Self::Labelled(_) => 20,
            Self::SectionTitle(_) => 21,
            Self::ColorSwatch(_) => 22,
            Self::ColorPicker(_) => 24,
            Self::Separator => 23,
        }
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) | (Self::Canvas, Self::Canvas) => true,
            (Self::Spinner(a), Self::Spinner(b)) => a == b,
            (Self::Separator, Self::Separator) => true,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Button(a), Self::Button(b)) => a == b,
            (Self::IconButton(a), Self::IconButton(b)) => a == b,
            (Self::Toggle(a), Self::Toggle(b)) => a == b,
            (Self::Checkbox(a), Self::Checkbox(b)) => a == b,
            (Self::Slider(a), Self::Slider(b)) => a == b,
            (Self::RangeSlider(a), Self::RangeSlider(b)) => a == b,
            (Self::Segment(a), Self::Segment(b)) => a == b,
            (Self::Tabs(a), Self::Tabs(b)) => a == b,
            (Self::DragValue(a), Self::DragValue(b)) => a == b,
            (Self::Field(a), Self::Field(b)) => a == b,
            (Self::Disclosure(a), Self::Disclosure(b)) => a == b,
            (Self::MenuRow(a), Self::MenuRow(b)) => a == b,
            (Self::ActionRow(a), Self::ActionRow(b)) => a == b,
            (Self::Badge(a), Self::Badge(b)) => a == b,
            (Self::Progress(a), Self::Progress(b)) => a == b,
            (Self::GradientBar(a), Self::GradientBar(b)) => a == b,
            (Self::Labelled(a), Self::Labelled(b)) => a == b,
            (Self::SectionTitle(a), Self::SectionTitle(b)) => a == b,
            (Self::ColorSwatch(a), Self::ColorSwatch(b)) => a == b,
            (Self::ColorPicker(a), Self::ColorPicker(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextContent {
    pub text: String,
    pub style: TextStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ButtonContent {
    pub label: String,
    pub role: Role,
    pub font: TypeStyle,
    pub symbol: Option<Icon>,
    pub style: Option<GlassStyle>,
    pub inset: f32,
    pub width: Length,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IconButtonContent {
    pub symbol: Icon,
    pub size: f32,
    pub role: Role,
    pub style: Option<GlassStyle>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToggleContent {
    pub value: bool,
    pub label: Option<String>,
    pub font: TypeStyle,
    pub row: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckboxContent {
    pub value: bool,
    pub label: String,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SliderContent {
    pub value: f32,
    pub range: RangeInclusive<f32>,
    pub label: Option<String>,
    pub readout: Option<String>,
    pub logarithmic: bool,
    pub width: Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RangeSliderContent {
    pub start: f32,
    pub end: f32,
    pub range: RangeInclusive<f32>,
    pub gradient: Vec<Color>,
    pub width: Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentContent {
    pub options: Vec<String>,
    pub selected: usize,
    pub fill: bool,
    pub tabs: bool,
    pub bordered: bool,
    pub font: TypeStyle,
    pub width: Length,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabsContent {
    pub options: Vec<String>,
    pub selected: usize,
    pub bordered: bool,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragValueContent {
    pub value: f32,
    pub decimals: usize,
    pub font: TypeStyle,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldContent {
    pub value: String,
    pub style: TextStyle,
    pub multiline: bool,
    pub radius: f32,
    pub editing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisclosureContent {
    pub title: String,
    pub open: bool,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuRowContent {
    pub label: String,
    pub checked: bool,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ActionRowContent {
    pub symbol: Icon,
    pub label: String,
    pub shortcut: Option<String>,
    pub font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BadgeContent {
    pub label: String,
    pub tint: Color,
    pub font: TypeStyle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinnerContent {
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgressContent {
    pub fraction: f32,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradientBarContent {
    pub colors: Vec<Color>,
    pub height: f32,
    pub radius: Corners,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelledContent {
    pub label: String,
    pub value: String,
    pub label_font: TypeStyle,
    pub value_font: TypeStyle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorSwatchContent {
    pub color: [f32; 3],
    pub size: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorPickerContent {
    pub color: [f32; 3],
}

pub fn text<'a, A>(value: impl Into<String>, style: TextStyle) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::AUTO,
        ..Layout::row()
    })
    .with_content(Content::Text(TextContent {
        text: value.into(),
        style,
    }))
}

pub fn canvas<'a, A>(
    width: Length,
    height: f32,
    revision: u64,
    paint: impl Fn(&mut Painter, CanvasInfo<'_>) + 'a,
) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(width, Length::Fixed(height)),
        ..Layout::column()
    })
    .paint(revision, paint)
}

pub fn spacer<'a, A>(size: f32) -> Element<'a, A> {
    Element::new(Layout {
        size: Size::new(Length::Fixed(0.0), Length::Fixed(size)),
        ..Layout::column()
    })
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
