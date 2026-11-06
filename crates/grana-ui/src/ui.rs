use std::collections::{HashMap, HashSet};

use glam::Vec2;
use grana_glass::{Animated, ContainerId, Glass as LayerGlass, GlassId, Scene as GlassScene};

use crate::clipboard::{Clipboard, NullClipboard};
use crate::element::{CanvasInfo, CanvasPaint, Element, Interaction, Key, Layer, Phase, Pointer};
use crate::geometry::{Corners, Rect};
use crate::glass::GlassStyle;
use crate::input::{Button, Cursor, Input};
use crate::layout::LayoutEngine;
use crate::motion::Motion;
use crate::node::NodeId;
use crate::paint::{Clip, Instance, LayerInstances, Painter};
use crate::render::{Ink, PaintFrame, Renderer};
use crate::scroll::ScrollState;
use crate::style::State;
use crate::text::{Text, TextLayout, TextStyle};
use crate::theme::{Level, Role, Theme};
use crate::tree::Tree;

const CLICK_SLOP: f32 = 3.0;
const TOOLTIP_DELAY: f32 = 0.55;

#[derive(Clone, Copy, Debug)]
struct Hit {
    node: NodeId,
    rect: Rect,
    clip: Rect,
    scroll: bool,
}

pub struct Ui<A = ()> {
    renderer: Renderer,
    text: Text,
    theme: Theme,
    input: Input,
    tree: Tree<A>,
    animations: HashMap<u64, Animated>,
    glass: GlassScene,
    glass_nodes: HashMap<NodeId, GlassId>,
    glass_groups: HashMap<Key, ContainerId>,
    tooltip_glass: Option<GlassId>,
    sub_glass: HashMap<NodeId, Vec<crate::widget::SubGlass>>,
    sub_glass_ids: HashMap<NodeId, Vec<GlassId>>,
    layers: [LayerInstances; 5],
    scale: f32,
    size: Vec2,
    time: f32,
    elapsed: f32,
    actions: Vec<A>,
    hits: Vec<Hit>,
    hover: Option<NodeId>,
    press: Option<NodeId>,
    drag: Option<NodeId>,
    press_at: Vec2,
    focus: Option<NodeId>,
    focus_visible: bool,
    focus_order: Vec<NodeId>,
    edit: Option<NodeId>,
    edit_caret: Option<Rect>,
    popup_anchors: HashMap<Key, NodeId>,
    cursor: Cursor,
    animating: bool,
    inside: bool,
    pointer: Vec2,
    occluders: Vec<Rect>,
    hovered_since: Option<(NodeId, f32)>,
    tooltip: Option<(NodeId, String)>,
    clipboard: Box<dyn Clipboard>,
    physics: bool,
    reduce_motion: bool,
    base_hash: Option<u64>,
    interface_hash: Option<u64>,
    base_changed: bool,
    interface_changed: bool,
    scene_changed: bool,
    content_reset: bool,
    painted_policy: bool,
    scratch: LayerInstances,
}

pub struct Frame {
    pub animating: bool,
    pub cursor: Cursor,
}

impl<A: Clone> Ui<A> {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format),
            text: Text::new(device),
            theme: Theme::dark(),
            input: Input::new(),
            tree: Tree::new(),
            animations: HashMap::new(),
            glass: GlassScene::new(),
            glass_nodes: HashMap::new(),
            glass_groups: HashMap::new(),
            tooltip_glass: None,
            sub_glass: HashMap::new(),
            sub_glass_ids: HashMap::new(),
            layers: std::array::from_fn(|_| LayerInstances::default()),
            scale: 1.0,
            size: Vec2::ZERO,
            time: 0.0,
            elapsed: 0.0,
            actions: Vec::new(),
            hits: Vec::new(),
            hover: None,
            press: None,
            drag: None,
            press_at: Vec2::ZERO,
            focus: None,
            focus_visible: false,
            focus_order: Vec::new(),
            edit: None,
            edit_caret: None,
            popup_anchors: HashMap::new(),
            cursor: Cursor::Arrow,
            animating: false,
            inside: false,
            pointer: Vec2::ZERO,
            occluders: Vec::new(),
            hovered_since: None,
            tooltip: None,
            clipboard: Box::new(NullClipboard),
            physics: true,
            reduce_motion: false,
            base_hash: None,
            interface_hash: None,
            base_changed: true,
            interface_changed: true,
            scene_changed: true,
            content_reset: true,
            painted_policy: true,
            scratch: LayerInstances::default(),
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

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn physics(&self) -> bool {
        self.physics
    }

    pub fn set_physics(&mut self, on: bool) {
        self.physics = on;
    }

    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    pub fn set_reduce_motion(&mut self, on: bool) {
        self.reduce_motion = on;
    }

    pub fn text(&mut self) -> &mut Text {
        &mut self.text
    }

    pub fn painter_in(&mut self, clip: Clip) -> Painter<'_> {
        let dark = self.theme.dark;
        Painter::new(&mut self.scratch, &mut self.text, clip, dark, false)
    }

    pub fn measure_text(&mut self, text: &str, style: TextStyle) -> Vec2 {
        self.text
            .layout(text, style.font, 0.0, false, style.truncate, style.align)
            .size
    }

    pub fn layout_text(
        &mut self,
        text: &str,
        style: TextStyle,
        width: f32,
    ) -> std::sync::Arc<TextLayout> {
        self.text.layout(
            text,
            style.font,
            width,
            style.wrap,
            style.truncate,
            style.align,
        )
    }

    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    pub fn copy(&mut self, text: &str) {
        self.clipboard.write(text);
    }

    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }

    pub fn edit_caret(&self) -> Option<Rect> {
        self.edit_caret
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    pub fn renderer_and_atlas(&mut self) -> (&mut Renderer, &crate::atlas::Atlas) {
        (&mut self.renderer, self.text.atlas())
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        self.renderer.resize(size);
        self.content_reset = true;
        self.tree.mark_dirty();
    }

    pub fn begin(&mut self, size: Vec2, scale: f32, time: f64) -> Vec<A> {
        self.size = size;
        self.scale = scale;
        self.time = time as f32;
        self.elapsed = self.input.begin(size, scale, time);
        self.text.begin(scale);
        self.actions.clear();
        self.focus_order.clear();
        self.occluders.clear();
        self.cursor = Cursor::Arrow;
        self.animating = false;
        self.edit_caret = None;
        self.inside = self.input.inside();
        self.pointer = self.input.position();
        self.glass.appearance.dark = self.theme.dark;
        self.glass.appearance.transparency = 0.5;
        self.glass.appearance.reduce_motion = self.reduce_motion || !self.physics;
        self.glass.physics = self.physics;
        self.glass.glass_on_glass = self.theme.glass_on_glass;
        self.glass.set_light_angle(self.theme.light_angle);
        self.advance_scroll();
        self.collect_regions();
        self.dispatch();
        self.animate_nodes();
        std::mem::take(&mut self.actions)
    }

    pub fn show<'a>(&mut self, element: Element<'a, A>) {
        let mut canvases = Vec::new();
        let mut removed = self.tree.apply(element, &mut canvases);
        for (node, glass) in removed.drain(..) {
            if let Some(glass) = glass {
                self.glass.remove(glass);
            }
            self.sub_glass.remove(&node);
            if let Some(ids) = self.sub_glass_ids.remove(&node) {
                for glass in ids {
                    self.glass.remove(glass);
                }
            }
        }
        self.popup_anchors.clear();
        self.sync_fields();
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        for id in ids {
            if let Some(spec) = self.tree.nodes[id].popup_spec {
                self.popup_anchors.insert(spec.anchor, id);
            }
        }
        if self.tree.layout_dirty {
            let mut engine = LayoutEngine {
                text: &mut self.text,
                theme: &self.theme,
                screen: self.size,
            };
            engine.run(&mut self.tree);
            self.update_scroll_metrics();
            self.rebuild_hits();
        }
        self.paint_layers(&canvases);
        self.sync_glass();
        self.update_tooltip();
        self.paint_overlays();
    }

    pub fn end(&mut self) -> Frame {
        let animating_glass = self.glass.update(self.elapsed);
        let glass_changed = self.glass.take_dirty();
        let base_hash = hash_layer(&self.layers[Layer::Content.index()], 0xcbf2_9ce4_8422_2325);
        let mut interface_hash = 0x9e37_79b9_7f4a_7c15;
        for layer in [Layer::Chrome, Layer::Labels, Layer::Popup, Layer::Hud] {
            interface_hash = hash_layer(&self.layers[layer.index()], interface_hash);
        }
        self.base_changed = self.base_hash != Some(base_hash) || self.content_reset;
        self.interface_changed =
            self.interface_hash != Some(interface_hash) || self.base_changed || glass_changed;
        self.base_hash = Some(base_hash);
        self.interface_hash = Some(interface_hash);
        self.content_reset = false;
        self.text.flush(self.renderer.queue());
        self.renderer.write_uniforms(self.scale);
        Frame {
            animating: self.animating || animating_glass,
            cursor: self.cursor,
        }
    }

    pub fn paint(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let policy = self.theme.glass_on_glass;
        let policy_changed = self.painted_policy != policy;
        self.painted_policy = policy;
        let base_changed = self.base_changed || self.scene_changed || policy_changed;
        let interface_changed = self.interface_changed || base_changed;
        {
            let Self {
                renderer,
                text,
                layers,
                glass,
                theme,
                ..
            } = self;
            renderer.paint(
                encoder,
                PaintFrame {
                    target,
                    atlas: text.atlas(),
                    content: &layers[0].material,
                    chrome: &layers[1],
                    labels: &layers[2],
                    popup: &layers[3],
                    hud: &layers[4],
                    glass,
                    glass_on_glass: policy,
                    base_changed,
                    interface_changed,
                    ink: if theme.dark { Ink::Light } else { Ink::Dark },
                },
            );
        }
        self.scene_changed = false;
    }

    pub fn set_scene_changed(&mut self, changed: bool) {
        self.scene_changed = changed;
    }

    pub fn scene_view(&self) -> &wgpu::TextureView {
        self.renderer.scene_view()
    }

    pub fn instances(&self) -> &[Instance] {
        &self.layers[Layer::Chrome.index()].material
    }

    pub fn content_instances(&self) -> &[Instance] {
        &self.layers[Layer::Content.index()].material
    }

    pub fn chrome_instances(&self) -> &[Instance] {
        &self.layers[Layer::Chrome.index()].material
    }

    pub fn atlas(&self) -> &crate::atlas::Atlas {
        self.text.atlas()
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        self.text.flush(queue);
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }

    pub fn pointer_in(&self, rect: Rect) -> Option<Vec2> {
        (self.inside && rect.contains(self.pointer)).then_some(self.pointer)
    }

    pub fn pointer_over_widget(&self) -> bool {
        let pointer = self.pointer;
        self.hits
            .iter()
            .any(|hit| hit.clip.contains(pointer) && hit.rect.contains(pointer))
    }

    pub fn pointer_over_chrome(&self) -> bool {
        let pointer = self.pointer;
        self.occluders.iter().any(|rect| rect.contains(pointer))
    }

    pub fn pointer_over_ui(&self) -> bool {
        self.input.button(Button::Primary).down || self.pointer_over_widget()
    }

    pub fn pointer_captured(&self) -> bool {
        self.press.is_some()
    }

    pub fn node_at(&self, key: Key) -> Option<NodeId> {
        self.tree.node_at(key)
    }

    pub fn rect_of(&self, key: Key) -> Option<Rect> {
        self.tree.node_at(key).map(|id| self.tree.nodes[id].rect)
    }

    pub fn scroll_offset(&self, key: Key) -> f32 {
        self.tree
            .node_at(key)
            .and_then(|id| self.tree.nodes[id].scroll)
            .map_or(0.0, |scroll| scroll.offset)
    }

    pub fn focus_visible(&self) -> bool {
        self.focus_visible
    }

    pub fn popup_open_any(&self) -> bool {
        self.popup_anchors
            .values()
            .any(|id| self.tree.nodes[*id].runtime.open)
    }

    pub fn popup_open(&self, key: Key) -> bool {
        self.popup_anchors
            .get(&key)
            .is_some_and(|id| self.tree.nodes[*id].runtime.open)
    }

    pub fn disclosure_open(&self, key: Key) -> bool {
        self.tree
            .node_at(key)
            .is_some_and(|id| self.tree.nodes[id].runtime.open)
    }

    pub fn spring(&mut self, key: u64, target: f32, motion: Motion) -> f32 {
        let motion = self.motion(motion);
        let elapsed = self.elapsed;
        let spring = self
            .animations
            .entry(key)
            .or_insert_with(|| Animated::new(target, motion.spring()));
        if motion.is_instant() {
            spring.snap(target);
            return target;
        }
        spring.animate_to(target, motion.spring());
        if !spring.is_at_rest() {
            spring.step(elapsed);
            self.animating = true;
        }
        spring.value
    }

    pub fn spring_value(&self, key: u64, fallback: f32) -> f32 {
        self.animations
            .get(&key)
            .map_or(fallback, |spring| spring.value)
    }

    pub fn appeared(&mut self, key: u64, seconds: f32) -> f32 {
        let now = self.time;
        let start = match self.animations.get(&key) {
            Some(animation) => animation.target,
            None => {
                self.animations
                    .insert(key, Animated::new(now, Motion::INSTANT.spring()));
                now
            }
        };
        let t = if seconds > 0.0 {
            ((now - start) / seconds).clamp(0.0, 1.0)
        } else {
            1.0
        };
        if t < 1.0 {
            self.animating = true;
        }
        1.0 - (1.0 - t).powi(3)
    }

    pub fn store_open(&mut self, key: Key, open: bool) {
        if let Some(id) = self.tree.node_at(key) {
            self.tree.nodes[id].runtime.open = open;
            self.tree.nodes[id].runtime.open_prop = open;
            self.tree.mark_dirty();
        }
    }

    pub fn toggle_popup(&mut self, key: Key) {
        if let Some(&id) = self.popup_anchors.get(&key) {
            let open = !self.tree.nodes[id].runtime.open;
            self.set_popup(id, open);
        }
    }

    pub fn close_popup(&mut self) {
        let open = self
            .popup_anchors
            .values()
            .copied()
            .find(|id| self.tree.nodes[*id].runtime.open);
        if let Some(id) = open {
            self.set_popup(id, false);
        }
    }

    pub fn description(&self) -> String {
        format!(
            "{} - {} nodes",
            self.text.description(),
            self.tree.nodes.len()
        )
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
        let mut instances = LayerInstances::default();
        {
            let mut painter = Painter::new(&mut instances, &mut self.text, clip, dark, false);
            content(&mut painter);
        }
        self.text.begin(self.scale);
        instances.material
    }

    pub fn build(&mut self) -> BuildContext<'_, A> {
        BuildContext {
            text: &mut self.text,
            theme: &self.theme,
            tree: &self.tree,
            animations: &mut self.animations,
            animating: &mut self.animating,
            size: self.size,
            scale: self.scale,
            time: self.time,
            elapsed: self.elapsed,
            physics: self.physics,
            reduce_motion: self.reduce_motion,
            stepped: HashSet::new(),
            pointer: self.pointer,
        }
    }

    fn motion(&self, motion: Motion) -> Motion {
        if !self.physics {
            Motion::INSTANT
        } else if self.reduce_motion {
            motion.reduced()
        } else {
            motion
        }
    }

    fn advance_scroll(&mut self) {
        let elapsed = self.elapsed;
        let motion = if self.physics {
            Motion::quick()
        } else {
            Motion::INSTANT
        };
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        for id in ids {
            let Some(scroll) = self.tree.nodes[id].scroll.as_mut() else {
                continue;
            };
            if scroll.settled() {
                continue;
            }
            scroll.update(elapsed, motion);
            self.tree.nodes[id].paint_dirty = true;
            self.tree.nodes[id].layout_dirty = true;
            self.tree.layout_dirty = true;
            self.animating = true;
        }
    }

    fn collect_regions(&mut self) {
        let mut focus = Vec::new();
        self.collect_focus(self.tree.root, &mut focus);
        self.focus_order = focus;
        if self
            .focus
            .is_some_and(|focus| !self.focus_order.contains(&focus))
        {
            self.focus = None;
            self.focus_visible = false;
        }
    }

    fn collect_focus(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let node = &self.tree.nodes[id];
        if !node.visible {
            return;
        }
        if node.interaction.is_focusable() && self.edit != Some(id) {
            out.push(id);
        }
        for child in &node.children {
            self.collect_focus(*child, out);
        }
    }

    fn dispatch(&mut self) {
        let pointer = self.pointer;
        self.hover = self
            .hits
            .iter()
            .rev()
            .find(|hit| hit.clip.contains(pointer) && hit.rect.contains(pointer))
            .map(|hit| hit.node);
        if let Some(node) = self.hover
            && let Some(cursor) = self.tree.nodes[node].cursor
        {
            self.cursor = cursor;
        }
        if let Some(node) = self.hover
            && matches!(
                self.tree.nodes[node].content,
                crate::element::Content::Segment(_) | crate::element::Content::Tabs(_)
            )
        {
            let index = {
                let rect = self.tree.nodes[node].rect;
                crate::widget::hover_cell(
                    &self.tree.nodes[node].content,
                    rect,
                    self.pointer,
                    &mut self.text,
                )
            };
            self.tree.nodes[node].runtime.thumb = index.min(u8::MAX as usize) as u8;
        }
        let primary = self.input.button(Button::Primary);
        if primary.pressed {
            self.press = self.hover;
            self.press_at = pointer;
            self.drag = None;
            self.dismiss_popups();
        }
        if primary.down
            && let Some(press) = self.press
            && self.drag.is_none()
            && (pointer - self.press_at).length() > CLICK_SLOP
        {
            self.drag = Some(press);
        }
        self.drag_values();
        if let Some(press) = self.press {
            self.pointer_events(
                press,
                if primary.pressed {
                    Phase::Down
                } else {
                    Phase::Move
                },
            );
        }
        if primary.released {
            if let Some(press) = self.press
                && self.drag.is_none()
                && self.hover.is_none_or(|hover| hover == press)
            {
                self.activate(press);
            }
            if let Some(press) = self.press {
                self.pointer_events(press, Phase::Up);
            }
            self.press = None;
            self.drag = None;
        }
        self.wheel();
        if self.edit.is_some() {
            self.field_keys();
        } else {
            self.keys();
        }
    }

    fn wheel(&mut self) {
        let wheel = self.input.wheel();
        if wheel == Vec2::ZERO {
            return;
        }
        let pointer = self.pointer;
        let owner = self
            .hits
            .iter()
            .rev()
            .find(|hit| hit.scroll && hit.clip.contains(pointer) && hit.rect.contains(pointer))
            .map(|hit| hit.node);
        let Some(node) = owner else {
            return;
        };
        let axis = self.tree.nodes[node]
            .scroll_spec
            .map_or(crate::element::Axis::Column, |spec| spec.axis);
        let amount = match axis {
            crate::element::Axis::Column => -wheel.y,
            crate::element::Axis::Row => -wheel.x - wheel.y,
        };
        if amount == 0.0 {
            return;
        }
        if let Some(scroll) = self.tree.nodes[node].scroll.as_mut() {
            scroll.scroll_by(amount * 42.0);
            self.tree.nodes[node].paint_dirty = true;
            self.animating = true;
        }
    }

    fn drag_values(&mut self) {
        let Some(node) = self.drag.or(self.press) else {
            return;
        };
        if !self.tree.nodes[node].enabled {
            return;
        }
        let dragging = self.drag == Some(node);
        let pointer = self.pointer;
        let rect = self.tree.nodes[node].rect;
        let mut action = None;
        let mut dragging_toggle = false;
        let mut hue_update = None;
        match &self.tree.nodes[node].interaction {
            Interaction::Toggle { .. } if dragging => dragging_toggle = true,
            Interaction::Slider {
                range,
                logarithmic,
                action: callback,
            } => {
                let value = slider_value(
                    &self.tree.nodes[node].content,
                    rect,
                    pointer,
                    range,
                    *logarithmic,
                    &mut self.text,
                    &self.theme,
                );
                let current = match &self.tree.nodes[node].content {
                    crate::element::Content::Slider(slider) => slider.value,
                    _ => value,
                };
                if (value - current).abs() > 1.0e-6 {
                    action = Some(callback(value));
                }
            }
            Interaction::RangeSlider {
                range,
                action: callback,
            } => {
                let (low, high) = range_slider_value(
                    &self.tree.nodes[node].content,
                    rect,
                    pointer,
                    range,
                    &mut self.text,
                    &self.theme,
                );
                let (start, end) = match &self.tree.nodes[node].content {
                    crate::element::Content::RangeSlider(slider) => (slider.start, slider.end),
                    _ => (low, high),
                };
                if (low - start).abs() > 1.0e-6 || (high - end).abs() > 1.0e-6 {
                    action = Some(callback(low, high));
                }
            }
            Interaction::Segment {
                action: callback, ..
            } if dragging => {
                if let Some(index) = segment_value(&self.tree.nodes[node].content, rect, pointer) {
                    action = Some(callback(index));
                }
            }
            Interaction::Color {
                color,
                action: callback,
            } => {
                let (stored, seeded) = (
                    self.tree.nodes[node].runtime.value.value,
                    self.tree.nodes[node].runtime.seeded,
                );
                if let Some((picked, hue)) =
                    crate::widget::color::pick(rect, *color, stored, seeded, pointer)
                {
                    hue_update = Some(hue);
                    if (picked[0] - color[0]).abs()
                        + (picked[1] - color[1]).abs()
                        + (picked[2] - color[2]).abs()
                        > 1.0e-6
                    {
                        action = Some(callback(picked));
                    }
                }
            }
            Interaction::DragValue {
                speed,
                action: callback,
                value,
            } if dragging => {
                let delta = self.input.delta().x;
                let scale = if self.input.modifiers().shift {
                    0.1
                } else {
                    1.0
                };
                if delta != 0.0 {
                    action = Some(callback(value + delta * speed * scale));
                }
            }
            _ => {}
        }
        if let Some(hue) = hue_update {
            let runtime = &mut self.tree.nodes[node].runtime;
            runtime.value.snap(hue);
            runtime.seeded = true;
        }
        if dragging_toggle {
            let drag = toggle_drag(rect, pointer);
            let runtime = &mut self.tree.nodes[node].runtime;
            runtime.dragging = true;
            runtime
                .value
                .animate_to(drag, Motion::response(0.14, 0.0).spring());
        }
        if let Some(action) = action {
            self.actions.push(action);
        }
    }

    fn pointer_events(&mut self, node: NodeId, phase: Phase) {
        let action = {
            let node = &self.tree.nodes[node];
            if let Interaction::Pointer { handler } = &node.interaction {
                handler(Pointer {
                    position: self.pointer,
                    delta: self.input.delta(),
                    phase,
                    button: Button::Primary,
                    modifiers: self.input.modifiers(),
                })
            } else {
                None
            }
        };
        if let Some(action) = action {
            self.actions.push(action);
        }
    }

    fn dismiss_popups(&mut self) {
        let pointer = self.pointer;
        let pressed = self.press;
        let open = self
            .popup_anchors
            .iter()
            .filter(|(_, id)| self.tree.nodes[**id].runtime.open)
            .map(|(anchor, id)| (*anchor, *id))
            .collect::<Vec<_>>();
        for (anchor, popup) in open {
            if pressed == self.tree.node_at(anchor) {
                continue;
            }
            let inside = self.tree.nodes[popup].rect.contains(pointer)
                || pressed.is_some_and(|node| is_inside(&self.tree, popup, node));
            if !inside {
                self.set_popup(popup, false);
            }
        }
    }

    fn activate(&mut self, node: NodeId) {
        let key = self.tree.nodes[node].key;
        if let Some(&popup) = self.popup_anchors.get(&key) {
            let open = !self.tree.nodes[popup].runtime.open;
            self.set_popup(popup, open);
            self.focus = Some(node);
            self.focus_visible = true;
            return;
        }
        if matches!(self.tree.nodes[node].interaction, Interaction::Field { .. }) {
            self.begin_edit(node);
            return;
        }
        if matches!(
            self.tree.nodes[node].content,
            crate::element::Content::Disclosure(_)
        ) {
            let open = !self.tree.nodes[node].runtime.open;
            self.tree.nodes[node].runtime.open = open;
            self.tree.nodes[node].runtime.open_prop = open;
            self.tree.mark_dirty();
            self.focus = Some(node);
            self.focus_visible = true;
            return;
        }
        if matches!(
            self.tree.nodes[node].content,
            crate::element::Content::Segment(_) | crate::element::Content::Tabs(_)
        ) {
            let rect = self.tree.nodes[node].rect;
            let index = {
                let content = &self.tree.nodes[node].content;
                crate::widget::hover_cell(content, rect, self.pointer, &mut self.text)
            };
            if index != usize::MAX
                && let Interaction::Segment { action } = &self.tree.nodes[node].interaction
            {
                let action = action(index);
                self.close_ancestor_popup(node);
                self.actions.push(action);
            }
            self.focus = Some(node);
            self.focus_visible = true;
            return;
        }
        let action = match &self.tree.nodes[node].interaction {
            Interaction::Press { action, .. } => action.clone(),
            Interaction::Toggle { value, action } => Some(action(!value)),
            _ => None,
        };
        if let Some(action) = action {
            self.close_ancestor_popup(node);
            self.actions.push(action);
        }
        self.focus = Some(node);
        self.focus_visible = true;
    }

    fn close_ancestor_popup(&mut self, node: NodeId) {
        let mut current = Some(node);
        while let Some(id) = current {
            if self.tree.nodes[id].popup_spec.is_some() {
                self.set_popup(id, false);
                return;
            }
            current = self.tree.nodes[id].parent;
        }
    }

    fn set_popup(&mut self, node: NodeId, open: bool) {
        self.tree.nodes[node].runtime.open = open;
        self.tree.nodes[node].paint_dirty = true;
        self.tree.nodes[node].layout_dirty = true;
        self.tree.mark_dirty();
    }

    fn begin_edit(&mut self, node: NodeId) {
        let (value, multiline) = match &self.tree.nodes[node].content {
            crate::element::Content::Field(field) => (field.value.clone(), field.multiline),
            _ => return,
        };
        self.edit = Some(node);
        self.focus = Some(node);
        self.focus_visible = false;
        self.input.clear_preedit();
        self.tree.nodes[node].runtime.edit = Some(if multiline {
            crate::edit::Edit::multiline(self.tree.nodes[node].key.0, &value, self.time)
        } else {
            crate::edit::Edit::new(self.tree.nodes[node].key.0, &value, self.time)
        });
        let caret = self.caret_at(node, self.pointer);
        if let Some(edit) = self.tree.nodes[node].runtime.edit.as_mut() {
            edit.place(caret, false, self.time);
        }
    }

    fn caret_at(&mut self, node: NodeId, pointer: Vec2) -> usize {
        let (value, style, multiline) = match &self.tree.nodes[node].content {
            crate::element::Content::Field(field) => {
                (field.value.clone(), field.style, field.multiline)
            }
            _ => return 0,
        };
        if multiline {
            return value.chars().count();
        }
        let rect = self.tree.nodes[node].rect;
        let x = pointer.x - (rect.min.x + 8.0);
        let mut best = 0usize;
        let mut distance = f32::INFINITY;
        for index in 0..=value.chars().count() {
            let offset = value
                .char_indices()
                .nth(index)
                .map_or(value.len(), |(offset, _)| offset);
            let prefix = &value[..offset];
            let width = if prefix.is_empty() {
                0.0
            } else {
                self.text
                    .layout(prefix, style.font, 0.0, false, false, style.align)
                    .size
                    .x
            };
            let candidate = (width - x).abs();
            if candidate < distance {
                distance = candidate;
                best = index;
            }
        }
        best
    }

    fn field_keys(&mut self) {
        let Some(node) = self.edit else {
            return;
        };
        let commit = {
            let Some(edit) = self.tree.nodes[node].runtime.edit.as_mut() else {
                self.edit = None;
                return;
            };
            let outcome = edit.poll(&self.input, self.clipboard.as_mut(), self.time);
            match outcome {
                Some(crate::edit::Outcome::Commit) => Some(true),
                Some(crate::edit::Outcome::Cancel) => Some(false),
                None => {
                    let primary = self.input.button(Button::Primary);
                    if primary.pressed && !self.tree.nodes[node].rect.contains(self.pointer) {
                        Some(true)
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(commit) = commit {
            let text = self.tree.nodes[node]
                .runtime
                .edit
                .take()
                .map(|edit| edit.text().to_string())
                .unwrap_or_default();
            self.edit = None;
            if commit && let Interaction::Field { commit, .. } = &self.tree.nodes[node].interaction
            {
                let action = commit(&text);
                self.actions.push(action);
            }
        }
    }

    fn keys(&mut self) {
        let modifiers = self.input.modifiers();
        let tab = self.input.keys().iter().any(|event| {
            event.pressed && !event.repeat && event.key == Some(crate::input::Key::Tab)
        });
        if tab {
            self.advance_focus(!modifiers.shift);
            return;
        }
        let escape = self.input.keys().iter().any(|event| {
            event.pressed && !event.repeat && event.key == Some(crate::input::Key::Escape)
        });
        if escape {
            for popup in self.popup_anchors.values().copied().collect::<Vec<_>>() {
                if self.tree.nodes[popup].runtime.open {
                    self.set_popup(popup, false);
                }
            }
            self.focus = None;
            self.focus_visible = false;
            return;
        }
        let activate = self.input.keys().iter().any(|event| {
            event.pressed
                && !event.repeat
                && matches!(
                    event.key,
                    Some(crate::input::Key::Enter | crate::input::Key::Space)
                )
        });
        if activate && let Some(node) = self.focus {
            self.activate(node);
        }
    }

    fn advance_focus(&mut self, forward: bool) {
        let count = self.focus_order.len();
        if count == 0 {
            return;
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
        self.focus_visible = true;
    }

    fn animate_nodes(&mut self) {
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        let elapsed = self.elapsed;
        for id in ids {
            if !self.tree.nodes[id].visible {
                continue;
            }
            let hovered = self.hover == Some(id);
            let active = self.press == Some(id);
            let animating = crate::widget::animate(
                &mut self.tree.nodes[id],
                hovered,
                active,
                elapsed,
                self.physics,
                self.reduce_motion,
                &mut self.text,
                &self.theme,
            );
            self.animating |= animating;
        }
    }

    fn sync_fields(&mut self) {
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        for id in ids {
            let editing = match &self.tree.nodes[id].content {
                crate::element::Content::Field(field) => field.editing,
                _ => false,
            };
            let active = self.tree.nodes[id].runtime.edit.is_some();
            if editing && !active {
                self.edit = Some(id);
                self.focus = Some(id);
                self.focus_visible = false;
                let (value, multiline) = match &self.tree.nodes[id].content {
                    crate::element::Content::Field(field) => (field.value.clone(), field.multiline),
                    _ => continue,
                };
                self.input.clear_preedit();
                self.tree.nodes[id].runtime.edit = Some(if multiline {
                    crate::edit::Edit::multiline(self.tree.nodes[id].key.0, &value, self.time)
                } else {
                    crate::edit::Edit::new(self.tree.nodes[id].key.0, &value, self.time)
                });
            } else if !editing && active {
                self.tree.nodes[id].runtime.edit = None;
                if self.edit == Some(id) {
                    self.edit = None;
                }
            }
        }
    }
    fn update_scroll_metrics(&mut self) {
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        for id in ids {
            let (spec, content, viewport) = {
                let node = &self.tree.nodes[id];
                let Some(spec) = node.scroll_spec else {
                    continue;
                };
                (
                    spec,
                    node.content_main,
                    match spec.axis {
                        crate::element::Axis::Column => node.rect.height(),
                        crate::element::Axis::Row => node.rect.width(),
                    },
                )
            };
            let scroll = self.tree.nodes[id]
                .scroll
                .get_or_insert_with(ScrollState::new);
            scroll.viewport = viewport;
            scroll.content = spec.content.unwrap_or(content).max(viewport);
            scroll.target = scroll.target.clamp(0.0, scroll.limit());
        }
    }

    fn rebuild_hits(&mut self) {
        let mut hits = Vec::new();
        collect_hits(
            &self.tree,
            self.tree.root,
            &mut hits,
            &mut self.text,
            &self.theme,
        );
        self.hits = hits;
    }

    fn paint_layers<'a>(&mut self, canvases: &[(NodeId, CanvasPaint<'a>)]) {
        for layer in &mut self.layers {
            layer.clear();
        }
        self.paint_node(self.tree.root, canvases);
    }

    fn paint_node<'a>(&mut self, id: NodeId, canvases: &[(NodeId, CanvasPaint<'a>)]) {
        if !self.tree.nodes[id].visible {
            return;
        }
        let layer = self.tree.nodes[id].layer.index();
        let mut instances = std::mem::take(&mut self.tree.nodes[id].paint);
        let dirty = self.tree.nodes[id].paint_dirty;
        if dirty {
            instances.clear();
            let node = &self.tree.nodes[id];
            let clip = Clip::new(node.clip, node.clip_corners);
            let vibrancy = matches!(node.layer, Layer::Labels | Layer::Popup | Layer::Hud);
            let glass = self
                .glass_nodes
                .get(&id)
                .copied()
                .and_then(|glass| self.glass.current_frame(glass))
                .map(|frame| {
                    Rect::from_min_size(
                        Vec2::new(frame.x, frame.y),
                        Vec2::new(frame.width, frame.height),
                    )
                });
            let info = crate::widget::PaintInfo {
                theme: &self.theme,
                rect: node.rect,
                glass,
                hover: node.runtime.hover.value,
                press: node.runtime.press.value,
                engaged: node.runtime.engaged.value,
                time: self.time,
                enabled: node.enabled,
                focused: self.focus == Some(id),
            };
            let mut painter = Painter::new(
                &mut instances,
                &mut self.text,
                clip,
                self.theme.dark,
                vibrancy,
            );
            if let Some(surface) = node.background
                && !node.rect.is_empty()
            {
                painter.outlined(
                    node.rect,
                    surface.corners,
                    surface.fill,
                    surface.border,
                    surface.shadow,
                );
            }
            let mut sub = Vec::new();
            let caret =
                crate::widget::paint(&node.content, &node.runtime, &mut painter, &info, &mut sub);
            if sub.is_empty() {
                self.sub_glass.remove(&id);
            } else {
                self.sub_glass.insert(id, sub);
            }
            if self.edit == Some(id) {
                self.edit_caret = caret;
            }
            if let Some(paint) = canvases
                .iter()
                .find(|(node, _)| *node == id)
                .map(|(_, paint)| paint)
            {
                let info = CanvasInfo {
                    rect: node.rect,
                    glass,
                    theme: &self.theme,
                    hover: node.runtime.hover.value,
                    press: node.runtime.press.value,
                    engaged: node.runtime.engaged.value,
                    time: self.time,
                    scale: self.scale,
                };
                paint(&mut painter, info);
            }
        }
        self.layers[layer]
            .material
            .extend(instances.material.iter().copied());
        self.layers[layer].ink.extend(instances.ink.iter().copied());
        self.tree.nodes[id].paint = instances;
        self.tree.nodes[id].paint_dirty = false;
        let children = self.tree.nodes[id].children.clone();
        for child in children {
            self.paint_node(child, canvases);
        }
    }

    fn sync_glass(&mut self) {
        let ids = self.tree.nodes.keys().collect::<Vec<_>>();
        let mut seen = Vec::with_capacity(ids.len());
        for id in ids {
            let (spec, rect, clip, visible, active, old_style) = {
                let node = &self.tree.nodes[id];
                (
                    node.glass_spec,
                    node.rect,
                    node.clip,
                    node.visible,
                    self.press == Some(id) || self.drag == Some(id),
                    node.glass_style,
                )
            };
            let Some(spec) = spec else {
                continue;
            };
            if !visible || rect.is_empty() {
                continue;
            }
            seen.push(id);
            let container = spec.group.map(|group| {
                let layer = spec.style.layer;
                *self
                    .glass_groups
                    .entry(group)
                    .or_insert_with(|| self.glass.add_container(0.0, layer))
            });
            let frame = glass_rect(rect);
            let clip = glass_rect(clip);
            match self.glass_nodes.get(&id).copied() {
                Some(glass) => {
                    if old_style != Some(spec.style) {
                        let fade = self.motion(Motion::response(0.4, 0.0)).spring();
                        self.glass.set_shape(glass, spec.style.shape);
                        self.glass
                            .set_material_with(glass, spec.style.material, fade);
                        self.glass
                            .set_behavior(glass, spec.style.interactive, spec.style.lens);
                        self.tree.nodes[id].glass_style = Some(spec.style);
                    }
                    self.glass.place(glass, frame);
                    self.glass.set_clip(glass, Some(clip));
                    let engaged = active && self.inside;
                    self.glass
                        .set_engaged(glass, engaged, engaged.then_some(self.pointer));
                }
                None => {
                    let mut glass = LayerGlass::new(frame)
                        .shape(spec.style.shape)
                        .material(spec.style.material)
                        .z(spec.style.layer)
                        .clip(clip);
                    if let Some(container) = container {
                        glass = glass.container(container);
                    }
                    if spec.style.interactive {
                        glass = glass.interactive();
                    }
                    if spec.style.lens {
                        glass = glass.lens();
                    }
                    let glass = self.glass.add(glass);
                    self.glass_nodes.insert(id, glass);
                    self.tree.nodes[id].glass_style = Some(spec.style);
                }
            }
        }
        for id in self.glass_nodes.keys().copied().collect::<Vec<_>>() {
            if !seen.contains(&id)
                && let Some(glass) = self.glass_nodes.remove(&id)
            {
                self.glass.remove(glass);
            }
        }
        let sub = self.sub_glass.clone();
        let mut live = Vec::new();
        for (node, items) in &sub {
            let node_clip = self.tree.nodes[*node].clip;
            let ids = self.sub_glass_ids.entry(*node).or_default();
            while ids.len() > items.len() {
                if let Some(glass) = ids.pop() {
                    self.glass.remove(glass);
                }
            }
            while ids.len() < items.len() {
                let item = items[ids.len()];
                let mut glass = LayerGlass::new(glass_rect(item.rect))
                    .shape(item.style.shape)
                    .material(item.style.material)
                    .z(item.style.layer)
                    .clip(glass_rect(node_clip));
                if item.style.interactive {
                    glass = glass.interactive();
                }
                if item.style.lens {
                    glass = glass.lens();
                }
                ids.push(self.glass.add(glass));
            }
            for (index, item) in items.iter().enumerate() {
                let glass = ids[index];
                self.glass.place(glass, glass_rect(item.rect));
                self.glass.set_clip(glass, Some(glass_rect(node_clip)));
                let engaged = item.engaged && self.inside;
                self.glass
                    .set_engaged(glass, engaged, engaged.then_some(self.pointer));
            }
            live.push(*node);
        }
        for node in self.sub_glass_ids.keys().copied().collect::<Vec<_>>() {
            if !live.contains(&node)
                && let Some(ids) = self.sub_glass_ids.remove(&node)
            {
                for glass in ids {
                    self.glass.remove(glass);
                }
            }
        }
        self.occluders = self
            .glass_nodes
            .keys()
            .filter_map(|id| {
                let node = &self.tree.nodes[*id];
                (node.visible && !node.rect.is_empty()).then_some(node.rect)
            })
            .collect();
        for id in self.tree.nodes.keys().collect::<Vec<_>>() {
            if self.tree.nodes[id].occluder {
                self.occluders.push(self.tree.nodes[id].rect);
            }
        }
    }

    fn update_tooltip(&mut self) {
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

    fn paint_overlays(&mut self) {
        let mut rings = Vec::new();
        if self.focus_visible
            && let Some(node) = self.focus
            && let Some(corners) = self.tree.nodes[node].focus_ring
        {
            rings.push((
                self.tree.nodes[node].rect,
                corners,
                self.tree.nodes[node].clip,
            ));
        }
        let scroll_bars = self
            .tree
            .nodes
            .keys()
            .filter_map(|id| {
                let node = &self.tree.nodes[id];
                let scroll = node.scroll?;
                if scroll.bar <= 0.01 || scroll.limit() <= 0.0 {
                    return None;
                }
                Some((node.rect, self.tree.nodes[id].scroll_spec?, scroll))
            })
            .collect::<Vec<_>>();
        let tooltip = self.tooltip.clone();
        let mut instances = LayerInstances::default();
        {
            let clip = Clip::new(Rect::from_min_size(Vec2::ZERO, self.size), Corners::ZERO);
            let mut painter =
                Painter::new(&mut instances, &mut self.text, clip, self.theme.dark, true);
            for (rect, corners, ring_clip) in rings {
                let previous = painter.clip();
                painter.push_clip(ring_clip, Corners::ZERO);
                painter.outlined(
                    rect.expand(2.0),
                    corners,
                    crate::style::Fill::None,
                    crate::style::Border {
                        width: 1.5,
                        color: self.theme.palette.focus,
                    },
                    crate::style::Shadow::NONE,
                );
                painter.pop_clip(previous);
            }
            for (outer, spec, scroll) in scroll_bars {
                let thumb = match spec.axis {
                    crate::element::Axis::Column => {
                        let track = Rect::from_min_size(
                            Vec2::new(outer.max.x - 8.0, outer.min.y + 4.0),
                            Vec2::new(4.0, (outer.height() - 8.0).max(8.0)),
                        );
                        let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
                        let height = (track.height() * ratio).max(24.0).min(track.height());
                        let travel = track.height() - height;
                        let position = if scroll.limit() > 0.0 {
                            (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        Rect::from_min_size(
                            Vec2::new(track.min.x, track.min.y + travel * position),
                            Vec2::new(track.width(), height),
                        )
                    }
                    crate::element::Axis::Row => {
                        let track = Rect::from_min_size(
                            Vec2::new(outer.min.x + 4.0, outer.max.y - 8.0),
                            Vec2::new((outer.width() - 8.0).max(8.0), 4.0),
                        );
                        let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
                        let width = (track.width() * ratio).max(24.0).min(track.width());
                        let travel = track.width() - width;
                        let position = if scroll.limit() > 0.0 {
                            (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        Rect::from_min_size(
                            Vec2::new(track.min.x + travel * position, track.min.y),
                            Vec2::new(width, track.height()),
                        )
                    }
                };
                painter.rect(
                    thumb,
                    Corners::capsule(),
                    self.theme
                        .palette
                        .text_quaternary
                        .with_alpha(0.7 * scroll.bar),
                );
            }
            if let Some((_, text)) = tooltip {
                let style = TextStyle::new(
                    self.theme.font(Level::Caption),
                    self.theme.palette.text_primary,
                );
                let layout = painter.layout(&text, style, 0.0);
                let pointer = self.pointer;
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
                self.place_tooltip_glass(rect.expand(2.0));
            }
        }
        self.layers[Layer::Hud.index()]
            .material
            .extend(instances.material);
        self.layers[Layer::Hud.index()].ink.extend(instances.ink);
    }

    fn place_tooltip_glass(&mut self, rect: Rect) {
        let frame = glass_rect(rect);
        match self.tooltip_glass {
            Some(glass) => {
                self.glass.place(glass, frame);
                self.glass.set_clip(glass, None);
            }
            None => {
                let glass = LayerGlass::new(frame)
                    .shape(GlassStyle::tooltip().shape)
                    .material(GlassStyle::tooltip().material)
                    .z(GlassStyle::tooltip().layer);
                let glass = self.glass.add(glass);
                self.tooltip_glass = Some(glass);
            }
        }
    }
}

fn collect_hits<A>(
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
    let scroll = node.scroll_spec.is_some();
    let popup = node.popup_spec.is_some() && node.runtime.open;
    if visible && (node.interaction.is_interactive() || scroll || popup) && node.enabled {
        let rect = if node.interaction.is_interactive() {
            crate::widget::hit_rect(node, text, theme).unwrap_or(node.rect)
        } else {
            node.rect
        };
        hits.push(Hit {
            node: id,
            rect,
            clip: node.clip,
            scroll,
        });
    }
    for child in &node.children {
        collect_hits(tree, *child, hits, text, theme);
    }
}

fn is_inside<A>(tree: &Tree<A>, root: NodeId, node: NodeId) -> bool {
    let mut current = Some(node);
    while let Some(id) = current {
        if id == root {
            return true;
        }
        current = tree.nodes[id].parent;
    }
    false
}

fn glass_rect(rect: Rect) -> grana_glass::Rect {
    grana_glass::Rect::new(rect.min.x, rect.min.y, rect.width(), rect.height())
}

fn hash_layer(layer: &LayerInstances, mut hash: u64) -> u64 {
    hash ^= layer.material.len() as u64;
    hash = hash.wrapping_mul(0x100_0000_01b3);
    for instance in layer.material.iter().chain(&layer.ink) {
        for word in bytemuck::cast_slice::<Instance, u32>(std::slice::from_ref(instance)) {
            hash ^= u64::from(*word);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
    }
    hash
}

fn toggle_drag(rect: Rect, pointer: Vec2) -> f32 {
    let knob = rect.height();
    let travel = (rect.width() - knob).max(1.0);
    ((pointer.x - rect.min.x - knob * 0.5) / travel).clamp(0.0, 1.0)
}

fn slider_value(
    content: &crate::element::Content,
    rect: Rect,
    pointer: Vec2,
    range: &std::ops::RangeInclusive<f32>,
    logarithmic: bool,
    text: &mut Text,
    theme: &Theme,
) -> f32 {
    let track = crate::widget::slider_track(content, rect, text, theme);
    let t = ((pointer.x - track.min.x) / track.width().max(1.0)).clamp(0.0, 1.0);
    let (minimum, maximum) = (*range.start(), *range.end());
    if logarithmic && minimum > 0.0 {
        (minimum.ln() + t * (maximum.ln() - minimum.ln())).exp()
    } else {
        minimum + t * (maximum - minimum)
    }
}

fn range_slider_value(
    content: &crate::element::Content,
    rect: Rect,
    pointer: Vec2,
    range: &std::ops::RangeInclusive<f32>,
    text: &mut Text,
    theme: &Theme,
) -> (f32, f32) {
    let (minimum, maximum) = (*range.start(), *range.end());
    let (start, end) = match content {
        crate::element::Content::RangeSlider(slider) => (slider.start, slider.end),
        _ => (minimum, maximum),
    };
    let span = (maximum - minimum).max(1.0e-6);
    let track = crate::widget::slider_track(content, rect, text, theme);
    let value =
        minimum + ((pointer.x - track.min.x) / track.width().max(1.0)).clamp(0.0, 1.0) * span;
    let distance_start = (value - start).abs();
    let distance_end = (value - end).abs();
    if distance_start <= distance_end {
        (value.min(end - 1.0e-4), end)
    } else {
        (start, value.max(start + 1.0e-4))
    }
}

fn segment_value(content: &crate::element::Content, rect: Rect, pointer: Vec2) -> Option<usize> {
    let count = match content {
        crate::element::Content::Segment(segment) => segment.options.len(),
        crate::element::Content::Tabs(tabs) => tabs.options.len(),
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let value = ((pointer.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 0.9999);
    Some((value * count as f32).floor() as usize)
}

pub struct BuildContext<'a, A> {
    text: &'a mut Text,
    theme: &'a Theme,
    tree: &'a Tree<A>,
    animations: &'a mut HashMap<u64, Animated>,
    animating: &'a mut bool,
    size: Vec2,
    scale: f32,
    time: f32,
    elapsed: f32,
    pointer: Vec2,
    physics: bool,
    reduce_motion: bool,
    stepped: HashSet<u64>,
}

impl<A> BuildContext<'_, A> {
    pub fn theme(&self) -> &Theme {
        self.theme
    }

    pub fn size(&self) -> Vec2 {
        self.size
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn measure(&mut self, text: &str, style: TextStyle) -> Vec2 {
        self.text
            .layout(text, style.font, 0.0, false, style.truncate, style.align)
            .size
    }

    pub fn layout_text(
        &mut self,
        text: &str,
        style: TextStyle,
        width: f32,
    ) -> std::sync::Arc<TextLayout> {
        self.text.layout(
            text,
            style.font,
            width,
            style.wrap,
            style.truncate,
            style.align,
        )
    }

    pub fn animate(&mut self, key: u64, target: f32, motion: Motion) -> f32 {
        let motion = if !self.physics {
            Motion::INSTANT
        } else if self.reduce_motion {
            motion.reduced()
        } else {
            motion
        };
        let spring = self
            .animations
            .entry(key)
            .or_insert_with(|| Animated::new(target, motion.spring()));
        if motion.is_instant() {
            spring.snap(target);
            return target;
        }
        spring.animate_to(target, motion.spring());
        if !spring.is_at_rest() {
            if self.stepped.insert(key) {
                spring.step(self.elapsed);
            }
            *self.animating = true;
        }
        spring.value
    }

    pub fn spring_value(&self, key: u64, fallback: f32) -> f32 {
        self.animations
            .get(&key)
            .map_or(fallback, |spring| spring.value)
    }

    pub fn surface(&self, role: Role, state: State) -> crate::style::Surface {
        self.theme.surface(role, state)
    }

    pub fn rect_of(&self, key: Key) -> Option<Rect> {
        self.tree.node_at(key).map(|id| self.tree.nodes[id].rect)
    }

    pub fn scroll_offset(&self, key: Key) -> f32 {
        self.tree
            .node_at(key)
            .and_then(|id| self.tree.nodes[id].scroll)
            .map_or(0.0, |scroll| scroll.offset)
    }

    pub fn popup_open(&self, key: Key) -> bool {
        self.tree
            .nodes
            .values()
            .any(|node| node.popup_spec.is_some_and(|spec| spec.anchor == key) && node.runtime.open)
    }

    pub fn disclosure_open(&self, key: Key) -> bool {
        self.tree
            .node_at(key)
            .is_some_and(|id| self.tree.nodes[id].runtime.open)
    }

    pub fn is_editing(&self, key: Key) -> bool {
        self.tree
            .node_at(key)
            .is_some_and(|id| self.tree.nodes[id].runtime.edit.is_some())
    }

    pub fn edit_text(&self, key: Key) -> Option<&str> {
        self.tree
            .node_at(key)
            .and_then(|id| self.tree.nodes[id].runtime.edit.as_ref())
            .map(|edit| edit.text())
    }

    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }
}
