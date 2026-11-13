use glam::Vec2;
use slotmap::DefaultKey;

use crate::edit::Edit;
use crate::element::{
    Axis, CanvasKey, Content, ContentKind, Interaction, Key, Layer, Layout, Popup, Scroll,
};
use crate::geometry::{Corners, Rect};
use crate::input::Cursor;
use crate::motion::{Animated, Spring};
use crate::paint::Clip;
use crate::paint::Instances;
use crate::style::Surface;

pub type NodeId = DefaultKey;

pub const SPRING: Spring = Spring::new(0.16, 0.0);

pub(crate) struct Runtime {
    pub hover: Animated,
    pub press: Animated,
    pub value: Animated,
    pub extent: Animated,
    pub engaged: Animated,
    pub drag_value: f32,
    pub drag_extent: f32,
    pub commit: Option<f32>,
    pub open: bool,
    pub edit: Option<Edit>,
    pub edit_prop: bool,
    pub thumb: u8,
    pub seeded: bool,
    pub active: bool,
    pub resnap: bool,
}

impl Runtime {
    pub fn new(content: &Content) -> Self {
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
                | Content::ColorPicker(_)
        );
        Self {
            hover: Animated::new(0.0, SPRING),
            press: Animated::new(0.0, SPRING),
            value: Animated::new(value, SPRING),
            extent: Animated::new(extent, SPRING),
            engaged: Animated::new(0.0, SPRING),
            drag_value: 0.0,
            drag_extent: 0.0,
            commit: None,
            open: false,
            edit: None,
            edit_prop: false,
            thumb: 0,
            seeded,
            active: false,
            resnap: false,
        }
    }
}

pub(crate) struct Node<A> {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub key: Key,
    pub kind: ContentKind,
    pub layout: Layout,
    pub content: Content,
    pub interaction: Interaction<A>,
    pub background: Option<Surface>,
    pub layer: Layer,
    pub layer_explicit: bool,
    pub position: Option<Rect>,
    pub scroll_spec: Option<Scroll>,
    pub popup_spec: Option<Popup>,
    pub tooltip: Option<String>,
    pub cursor: Option<Cursor>,
    pub enabled: bool,
    pub focus_ring: Option<Corners>,
    pub blocks_pointer: bool,
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
    pub paint: Instances,
    pub paint_dirty: bool,
    pub canvas: Option<CanvasKey>,
    pub visible: bool,
    pub painted: bool,
    pub layer_range: (u32, u32),
    pub measure_dirty: bool,
    pub measure_valid: bool,
    pub measure_available: Vec2,
    pub measure_loose: (bool, bool),
    pub arrange_dirty: bool,
    pub arranged: bool,
    pub arranged_clip: Clip,
}

impl<A> Node<A> {
    pub fn new(
        key: Key,
        kind: ContentKind,
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
            background: None,
            layer: Layer::Chrome,
            layer_explicit: false,
            position: None,
            scroll_spec: None,
            popup_spec: None,
            tooltip: None,
            cursor: None,
            enabled: true,
            focus_ring: None,
            blocks_pointer: false,
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
            paint: Instances::default(),
            paint_dirty: true,
            canvas: None,
            visible: true,
            painted: false,
            layer_range: (0, 0),
            measure_dirty: true,
            measure_valid: false,
            measure_available: Vec2::ZERO,
            measure_loose: (false, false),
            arrange_dirty: true,
            arranged: false,
            arranged_clip: Clip::new(Rect::ZERO, Corners::ZERO),
        }
    }

    pub fn interactive(&self) -> bool {
        self.interaction.is_interactive()
    }

    pub fn scroll_axis(&self) -> Axis {
        self.scroll_spec.map_or(Axis::Column, |scroll| scroll.axis)
    }
}
