use glam::Vec2;
use grana_glass::{Animated, Spring};
use slotmap::DefaultKey;

use crate::edit::Edit;
use crate::element::{Content, Glass, Interaction, Key, Layer, Layout, Popup, Scroll};
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::input::Cursor;
use crate::paint::LayerInstances;
use crate::style::Surface;
use grana_glass::GlassId;

pub type NodeId = DefaultKey;

pub const SPRING: Spring = Spring::new(0.18, 0.0);

pub(crate) struct Runtime {
    pub hover: Animated,
    pub press: Animated,
    pub value: Animated,
    pub extent: Animated,
    pub engaged: Animated,
    pub dragging: bool,
    pub open: bool,
    pub open_prop: bool,
    pub edit: Option<Edit>,
    pub thumb: u8,
    pub seeded: bool,
}

impl Runtime {
    pub fn new(content: &Content) -> Self {
        let open = match content {
            Content::Disclosure(disclosure) => disclosure.open,
            _ => false,
        };
        let (value, extent) = match content {
            Content::Toggle(toggle) => (if toggle.value { 1.0 } else { 0.0 }, 0.0),
            Content::Checkbox(checkbox) => (if checkbox.value { 1.0 } else { 0.0 }, 0.0),
            Content::Slider(slider) => {
                let (minimum, maximum) = (*slider.range.start(), *slider.range.end());
                let normalized = if slider.logarithmic && minimum > 0.0 {
                    (slider.value.max(minimum).ln() - minimum.ln()) / (maximum.ln() - minimum.ln())
                } else {
                    (slider.value - minimum) / (maximum - minimum)
                };
                (normalized.clamp(0.0, 1.0), 0.0)
            }
            Content::RangeSlider(range) => {
                let (minimum, maximum) = (*range.range.start(), *range.range.end());
                let span = (maximum - minimum).max(1.0e-6);
                (
                    ((range.start - minimum) / span).clamp(0.0, 1.0),
                    ((range.end - minimum) / span).clamp(0.0, 1.0),
                )
            }
            Content::Disclosure(disclosure) => (if disclosure.open { 1.0 } else { 0.0 }, 0.0),
            Content::ColorPicker(picker) => {
                let (hue, _, _) = crate::widget::color::rgb_to_hsv(picker.color);
                (hue, 0.0)
            }
            _ => (0.0, 0.0),
        };
        let seeded = matches!(
            content,
            Content::Toggle(_)
                | Content::Checkbox(_)
                | Content::Slider(_)
                | Content::RangeSlider(_)
                | Content::Disclosure(_)
                | Content::ColorPicker(_)
        );
        Self {
            hover: Animated::new(0.0, SPRING),
            press: Animated::new(0.0, SPRING),
            value: Animated::new(value, SPRING),
            extent: Animated::new(extent, SPRING),
            engaged: Animated::new(0.0, SPRING),
            dragging: false,
            open,
            open_prop: open,
            edit: None,
            thumb: 0,
            seeded,
        }
    }
}

pub(crate) struct Node<A> {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub key: Key,
    pub kind: u8,
    pub layout: Layout,
    pub content: Content,
    pub interaction: Interaction<A>,
    pub revision: u64,
    pub background: Option<Surface>,
    pub glass_spec: Option<Glass>,
    pub glass_id: Option<GlassId>,
    pub glass_style: Option<GlassStyle>,
    pub layer: Layer,
    pub layer_explicit: bool,
    pub position: Option<Rect>,
    pub scroll_spec: Option<Scroll>,
    pub popup_spec: Option<Popup>,
    pub tooltip: Option<String>,
    pub cursor: Option<Cursor>,
    pub occluder: bool,
    pub enabled: bool,
    pub focus_ring: Option<Corners>,
    pub runtime: Runtime,
    pub rect: Rect,
    pub measured: Vec2,
    pub provisional: Vec2,
    pub inner_provisional: Vec2,
    pub content_main: f32,
    pub content_cross: f32,
    pub popup_height: f32,
    pub auto_cross: bool,
    pub auto_main: bool,
    pub baseline: Option<f32>,
    pub scroll: Option<crate::scroll::ScrollState>,
    pub clip: Rect,
    pub clip_corners: Corners,
    pub paint: LayerInstances,
    pub paint_dirty: bool,
    pub layout_dirty: bool,
    pub visible: bool,
    pub has_canvas: bool,
}

impl<A> Node<A> {
    pub fn new(
        key: Key,
        kind: u8,
        layout: Layout,
        content: Content,
        interaction: Interaction<A>,
    ) -> Self {
        let runtime = Runtime::new(&content);
        Self {
            parent: None,
            children: Vec::new(),
            key,
            kind,
            layout,
            content,
            interaction,
            revision: 0,
            background: None,
            glass_spec: None,
            glass_id: None,
            glass_style: None,
            layer: Layer::Chrome,
            layer_explicit: false,
            position: None,
            scroll_spec: None,
            popup_spec: None,
            tooltip: None,
            cursor: None,
            occluder: false,
            enabled: true,
            focus_ring: None,
            runtime,
            rect: Rect::ZERO,
            measured: Vec2::ZERO,
            provisional: Vec2::ZERO,
            inner_provisional: Vec2::ZERO,
            content_main: 0.0,
            content_cross: 0.0,
            popup_height: 0.0,
            auto_cross: false,
            auto_main: false,
            baseline: None,
            scroll: None,
            clip: Rect::ZERO,
            clip_corners: Corners::ZERO,
            paint: LayerInstances::default(),
            paint_dirty: true,
            layout_dirty: true,
            visible: true,
            has_canvas: false,
        }
    }
}
