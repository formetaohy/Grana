use std::collections::HashMap;
use std::ops::Range;

use glam::Vec2;
use grana_glass::{
    ContainerId, Glass, GlassId, Material, Rect as GlassRect, Scene as GlassScene,
    Spring as GlassSpring,
};

use crate::clipboard::{Clipboard, NullClipboard};
use crate::edit::{Edit, Outcome as EditOutcome};
use crate::geometry::{Corners, Edges, Rect};
use crate::glass::{GlassStyle, MENU, OVERLAY};
use crate::input::{Button, Input};
use crate::motion::Motion;
use crate::paint::{Clip, Instance, LayerInstances, Painter};
use crate::registry::{Cursor, FrameInput, Registry};
use crate::render::{Ink, Renderer};
use crate::style::{Border, Fill, Shadow, State, Surface};
use crate::text::{Align, Text, TextLayout, TextStyle};
use crate::theme::{Level, Role, Theme};

const PROPERTY_SALT: u64 = 0x9e37_79b9_7f4a_7c15;
const GRID_SALT: u64 = 0x517c_c1b7_2722_0a95;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Content,
    Chrome,
    Labels,
    Popup,
    Hud,
}

#[derive(Clone, Copy, Debug)]
struct GlassEntry {
    id: GlassId,
    style: GlassStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlassGroup(pub(crate) ContainerId);

struct GlassTransition {
    anchor: GlassId,
    popover: Option<GlassId>,
    closing: Option<GlassId>,
    body: Rect,
    snapshot: LayerInstances,
    touched: f32,
    closing_at: Option<f32>,
}

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
    Minimum(f32),
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
    background: Option<Surface>,
    clip: bool,
    wrapping: bool,
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
            wrapping: false,
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

    pub fn background(mut self, surface: Surface) -> Self {
        self.background = Some(surface);
        self
    }

    pub fn clipped(mut self) -> Self {
        self.clip = true;
        self
    }

    pub fn wrapping(mut self) -> Self {
        self.wrapping = true;
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
    loose_width: bool,
    loose_height: bool,
    wrapping: bool,
    line: f32,
    line_height: f32,
}

const LIST_COUNT: usize = 9;

#[derive(Clone, Copy, Debug)]
struct Span {
    lists: [usize; LIST_COUNT],
    glass: usize,
    hit: usize,
    wheel: usize,
    offset: f32,
    size: f32,
    direct: bool,
    baseline: Option<f32>,
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

struct Cell {
    lists: [Range<usize>; LIST_COUNT],
    glass: Range<usize>,
    hits: Range<usize>,
    wheels: Range<usize>,
    height: f32,
}

pub struct Grid<'a> {
    ui: &'a mut Ui,
    widths: &'a [f32],
    gap: f32,
    origin: Vec2,
    row: usize,
    column: usize,
    row_y: f32,
    total_width: f32,
    align: Align,
    cells: Vec<Cell>,
}

pub struct Ui {
    renderer: Renderer,
    text: Text,
    theme: Theme,
    input: Input,
    registry: Registry,
    content: LayerInstances,
    chrome: LayerInstances,
    labels: LayerInstances,
    popup: LayerInstances,
    hud: LayerInstances,
    areas: Vec<Area>,
    frames: Vec<SpanFrame>,
    span_pool: Vec<Vec<Span>>,
    ids: Vec<u64>,
    scale: f32,
    size: Vec2,
    time: f32,
    elapsed: f32,
    occluders: Vec<Rect>,
    focus_rings: Vec<(Rect, Corners, Clip)>,
    popup_clip: Clip,
    hud_clip: Clip,
    layer: Layer,
    glass: GlassScene,
    glass_entries: HashMap<u64, GlassEntry>,
    glass_groups: HashMap<u64, ContainerId>,
    glass_engaged: HashMap<GlassId, bool>,
    glass_placed: Vec<GlassId>,
    glass_seen: Vec<u64>,
    transitions: HashMap<u64, GlassTransition>,
    floating_container: Option<ContainerId>,
    floating: bool,
    floating_draining: bool,
    edit: Option<Edit>,
    edit_seen: bool,
    edit_caret: Option<Rect>,
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
            content: LayerInstances::default(),
            chrome: LayerInstances::default(),
            labels: LayerInstances::default(),
            popup: LayerInstances::default(),
            hud: LayerInstances::default(),
            areas: Vec::new(),
            frames: Vec::new(),
            span_pool: Vec::new(),
            ids: Vec::new(),
            scale: 1.0,
            size: Vec2::ZERO,
            time: 0.0,
            elapsed: 0.0,
            occluders: Vec::new(),
            focus_rings: Vec::new(),
            popup_clip: Clip::new(Rect::ZERO, Corners::ZERO),
            hud_clip: Clip::new(Rect::ZERO, Corners::ZERO),
            layer: Layer::Chrome,
            glass: GlassScene::new(),
            glass_entries: HashMap::new(),
            glass_groups: HashMap::new(),
            glass_engaged: HashMap::new(),
            glass_placed: Vec::new(),
            glass_seen: Vec::new(),
            transitions: HashMap::new(),
            floating_container: None,
            floating: false,
            floating_draining: false,
            edit: None,
            edit_seen: false,
            edit_caret: None,
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

    fn motion(&self, motion: Motion) -> Motion {
        if !self.physics {
            Motion::INSTANT
        } else if self.reduce_motion {
            motion.reduced()
        } else {
            motion
        }
    }

    pub fn text(&mut self) -> &mut Text {
        &mut self.text
    }

    pub fn begin_edit(&mut self, key: u64, text: &str) {
        self.begin_edit_with(Edit::new(key, text, self.time));
    }

    pub fn begin_edit_multiline(&mut self, key: u64, text: &str) {
        self.begin_edit_with(Edit::multiline(key, text, self.time));
    }

    fn begin_edit_with(&mut self, edit: Edit) {
        self.input.clear_preedit();
        let key = edit.key();
        self.edit = Some(edit);
        self.edit_seen = true;
        self.registry.set_focus(key);
    }

    pub fn end_edit(&mut self) {
        self.input.clear_preedit();
        self.edit = None;
    }

    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    pub fn copy(&mut self, text: &str) {
        self.clipboard.write(text);
    }

    pub fn edit_caret(&self) -> Option<Rect> {
        self.edit_caret
    }

    pub(crate) fn set_edit_caret(&mut self, caret: Rect) {
        self.edit_caret = Some(caret);
    }

    pub(crate) fn touch_edit(&mut self) {
        self.edit_seen = true;
    }

    pub fn editing(&self) -> bool {
        self.edit.is_some()
    }

    pub fn is_editing(&self, key: u64) -> bool {
        self.edit.as_ref().is_some_and(|edit| edit.key() == key)
    }

    pub fn edit(&self) -> Option<&Edit> {
        self.edit.as_ref()
    }

    pub fn edit_mut(&mut self) -> Option<&mut Edit> {
        self.edit.as_mut()
    }

    pub fn poll_edit(&mut self) -> Option<EditOutcome> {
        let Self {
            edit,
            input,
            clipboard,
            time,
            ..
        } = self;
        let outcome = edit.as_mut()?.poll(input, clipboard.as_mut(), *time)?;
        Some(outcome)
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
        let modifiers = self.input.modifiers();
        let tab = self
            .input
            .keys()
            .iter()
            .find(|event| {
                event.pressed && !event.repeat && event.key == Some(crate::input::Key::Tab)
            })
            .map(|_| !modifiers.shift);
        let escape = self
            .input
            .keys()
            .iter()
            .any(|event| event.pressed && event.key == Some(crate::input::Key::Escape));
        self.registry.begin(FrameInput {
            position: pointer,
            pressed,
            released,
            down,
            tab,
            escape,
            time: self.time,
        });
        self.content.clear();
        self.chrome.clear();
        self.labels.clear();
        self.popup.clear();
        self.hud.clear();
        self.glass_engaged.clear();
        self.glass_placed.clear();
        self.occluders.clear();
        self.focus_rings.clear();
        self.areas.clear();
        self.frames.clear();
        self.ids.clear();
        self.glass_seen.clear();
        self.floating = false;
        self.layer = Layer::Chrome;
        self.edit_seen = false;
        self.edit_caret = None;
        self.popup_clip = Clip::new(Rect::from_min_size(Vec2::ZERO, size), Corners::ZERO);
        self.hud_clip = self.popup_clip;
        self.glass.appearance.dark = self.theme.dark;
        self.glass.appearance.transparency = 0.5;
        self.glass.appearance.reduce_motion = self.reduce_motion || !self.physics;
        self.glass.physics = self.physics;
        self.glass.glass_on_glass = self.theme.glass_on_glass;
        self.glass.set_light_angle(self.theme.light_angle);
    }

    pub fn end(&mut self) -> Frame {
        self.registry.end(self.elapsed, self.physics);
        if !self.edit_seen {
            self.end_edit();
        }
        if !self.focus_rings.is_empty() {
            let dark = self.theme.dark;
            let color = self.theme.palette.focus;
            let rings = std::mem::take(&mut self.focus_rings);
            for (rect, corners, ring_clip) in rings {
                let mut painter =
                    Painter::new(&mut self.hud, &mut self.text, ring_clip, dark, true);
                painter.outlined(
                    rect.expand(2.0),
                    corners,
                    Fill::None,
                    Border { width: 1.5, color },
                    Shadow::NONE,
                );
            }
        }
        self.retire_glass();
        let pointer = self.input.inside().then_some(self.input.position());
        for (&id, &engaged) in &self.glass_engaged {
            self.glass
                .set_engaged(id, engaged, engaged.then_some(pointer).flatten());
        }
        self.settle_transitions();
        let animating_glass = self.glass.update(self.elapsed);
        let glass_changed = self.glass.take_dirty();
        if !self.floating && !animating_glass && self.transitions.is_empty() {
            self.floating_draining = false;
        }
        let base_hash = hash_layer(&self.content, 0xcbf2_9ce4_8422_2325);
        let mut interface_hash = 0x9e37_79b9_7f4a_7c15;
        for layer in [&self.chrome, &self.labels, &self.popup, &self.hud] {
            interface_hash = hash_layer(layer, interface_hash);
        }
        self.base_changed = self.base_hash != Some(base_hash) || self.content_reset;
        self.interface_changed =
            self.interface_hash != Some(interface_hash) || self.base_changed || glass_changed;
        self.base_hash = Some(base_hash);
        self.interface_hash = Some(interface_hash);
        self.content_reset = false;
        self.text.flush(self.renderer.queue());
        self.renderer.write_uniforms(self.scale);
        self.areas.clear();
        self.recycle_frames();
        self.ids.clear();
        Frame {
            animating: self.registry.animating() || animating_glass,
            cursor: self.registry.cursor(),
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
                content,
                chrome,
                labels,
                popup,
                hud,
                glass,
                theme,
                ..
            } = self;
            renderer.paint(
                encoder,
                crate::render::PaintFrame {
                    target,
                    atlas: text.atlas(),
                    content: &content.material,
                    chrome,
                    labels,
                    popup,
                    hud,
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

    pub fn glass(&mut self, key: u64, rect: Rect, style: GlassStyle) {
        self.glass_in(None, key, rect, style);
    }

    pub fn glass_group(&mut self, key: u64, spacing: f32, layer: i32) -> GlassGroup {
        if let Some(id) = self.glass_groups.get(&key).copied() {
            return GlassGroup(id);
        }
        let id = self.glass.add_container(spacing, layer);
        self.glass_groups.insert(key, id);
        GlassGroup(id)
    }

    pub fn set_glass_group_spacing(&mut self, group: GlassGroup, spacing: f32) {
        self.glass.set_container_spacing(group.0, spacing);
    }

    pub fn glass_in(&mut self, group: Option<GlassGroup>, key: u64, rect: Rect, style: GlassStyle) {
        if rect.is_empty() {
            return;
        }
        let frame = GlassRect::new(rect.min.x, rect.min.y, rect.width(), rect.height());
        if style.layer >= OVERLAY {
            self.floating = true;
            self.floating_draining = true;
        }
        self.glass_seen.push(key);
        self.register_occluder(rect);
        let clip = self.area().clip.rect;
        let engaged = self.registry.active(key) && self.input.button(Button::Primary).down;
        let entry = self.glass_entries.get(&key).copied();
        match entry {
            Some(entry) => {
                if entry.style != style {
                    let fade = self.motion(Motion::response(0.4, 0.0)).spring();
                    self.glass.set_shape(entry.id, style.shape);
                    self.glass.set_material_with(entry.id, style.material, fade);
                    self.glass
                        .set_behavior(entry.id, style.interactive, style.lens);
                    self.glass_entries.insert(
                        key,
                        GlassEntry {
                            id: entry.id,
                            style,
                        },
                    );
                }
                self.glass.place(entry.id, frame);
                self.glass.set_clip(entry.id, Some(glass_rect(clip)));
                self.glass_engaged
                    .entry(entry.id)
                    .and_modify(|e| *e |= engaged)
                    .or_insert(engaged);
                self.glass_placed.push(entry.id);
            }
            None => {
                let mut glass = Glass::new(frame)
                    .shape(style.shape)
                    .material(style.material)
                    .z(style.layer)
                    .clip(glass_rect(clip));
                if let Some(group) = group {
                    glass = glass.container(group.0);
                }
                if style.interactive {
                    glass = glass.interactive();
                }
                if style.lens {
                    glass = glass.lens();
                }
                let id = self.glass.add(glass);
                self.glass_engaged
                    .entry(id)
                    .and_modify(|e| *e |= engaged)
                    .or_insert(engaged);
                self.glass_placed.push(id);
                self.glass_entries.insert(key, GlassEntry { id, style });
            }
        }
    }

    pub fn engage_glass(&mut self, key: u64, engaged: bool) {
        if let Some(entry) = self.glass_entries.get(&key).copied() {
            let active = engaged && self.input.button(Button::Primary).down;
            self.glass_engaged
                .entry(entry.id)
                .and_modify(|e| *e |= active)
                .or_insert(active);
        }
    }

    pub fn glass_frame(&self, key: u64) -> Option<Rect> {
        let entry = self.glass_entries.get(&key)?;
        let frame = self.glass.current_frame(entry.id)?;
        Some(Rect::from_min_size(
            Vec2::new(frame.x, frame.y),
            Vec2::new(frame.width, frame.height),
        ))
    }

    pub fn glass_expand(&mut self, key: u64, anchor: Rect, body: Rect, style: GlassStyle) -> f32 {
        if body.is_empty() {
            return 1.0;
        }
        let time = self.time;
        if self
            .transitions
            .get(&key)
            .is_some_and(|transition| transition.popover.is_none())
            && let Some(transition) = self.transitions.remove(&key)
        {
            self.glass.remove(transition.anchor);
        }
        if let Some(transition) = self.transitions.get_mut(&key) {
            transition.touched = time;
            self.glass.place(transition.anchor, glass_rect(anchor));
            if let Some(id) = transition.popover {
                if transition.body != body {
                    transition.body = body;
                    self.glass
                        .set_frame_with(id, glass_rect(body), GlassSpring::new(0.3, 0.0));
                }
                let ratio = self
                    .glass
                    .current_frame(id)
                    .map_or(1.0, |frame| glass_ratio(frame, body));
                let anchor_material = if ratio > 0.92 {
                    Material::identity()
                } else {
                    Material::regular()
                };
                self.glass.set_material(transition.anchor, anchor_material);
                return expand_alpha(&self.glass, id, body);
            }
        }
        let container = match self.floating_container {
            Some(container) => container,
            None => {
                let container = self.glass.add_container(16.0, MENU);
                self.floating_container = Some(container);
                container
            }
        };
        let anchor_id = self.glass.add(
            Glass::new(glass_rect(anchor))
                .shape(grana_glass::Shape::Capsule)
                .material(Material::regular())
                .container(container),
        );
        self.glass.snap_presence(anchor_id, 1.0);
        let popover = Glass::new(glass_rect(body))
            .shape(style.shape)
            .material(style.material)
            .container(container);
        let id = self
            .glass
            .expand_from(anchor_id, popover, GlassSpring::new(0.45, 0.22));
        self.transitions.insert(
            key,
            GlassTransition {
                anchor: anchor_id,
                popover: Some(id),
                closing: None,
                body,
                snapshot: LayerInstances::default(),
                touched: time,
                closing_at: None,
            },
        );
        self.floating = true;
        self.floating_draining = true;
        expand_alpha(&self.glass, id, body)
    }

    pub fn glass_collapse(&mut self, key: u64) -> bool {
        let time = self.time;
        let Some(transition) = self.transitions.get_mut(&key) else {
            return false;
        };
        transition.touched = time;
        if transition.closing.is_some() {
            return true;
        }
        let Some(id) = transition.popover.take() else {
            return false;
        };
        let anchor = transition.anchor;
        transition.closing = Some(id);
        transition.closing_at = Some(time);
        self.glass.set_material(anchor, Material::regular());
        self.glass.snap_presence(anchor, 1.0);
        self.glass
            .collapse_into(id, anchor, GlassSpring::new(0.35, 0.0));
        self.floating_draining = true;
        true
    }

    pub fn popup_len(&self) -> (usize, usize) {
        (self.popup.material.len(), self.popup.ink.len())
    }

    pub fn record_popup(&mut self, key: u64, start: (usize, usize)) {
        if !self.transitions.contains_key(&key) {
            return;
        }
        let snapshot = LayerInstances {
            material: self
                .popup
                .material
                .get(start.0..)
                .unwrap_or_default()
                .to_vec(),
            ink: self.popup.ink.get(start.1..).unwrap_or_default().to_vec(),
        };
        let morph = self.transitions.get(&key).and_then(|transition| {
            let id = transition.popover?;
            let frame = self.glass.current_frame(id)?;
            let corner = self.glass.current_corner(id).map_or(24.0, |(r, _)| r);
            Some((frame, corner, transition.body))
        });
        if let Some(transition) = self.transitions.get_mut(&key) {
            transition.snapshot = snapshot;
        }
        if let Some((frame, corner, body)) = morph {
            let current = Rect::from_min_size(
                Vec2::new(frame.x, frame.y),
                Vec2::new(frame.width, frame.height),
            );
            let delta = Vec2::new(
                current.center().x - body.center().x,
                current.min.y - body.min.y,
            );
            let corners = Corners::all(corner);
            let material = start.0.min(self.popup.material.len());
            let ink = start.1.min(self.popup.ink.len());
            for instance in &mut self.popup.material[material..] {
                instance.translate(delta);
                clip_instance(instance, current, corners);
            }
            for instance in &mut self.popup.ink[ink..] {
                instance.translate(delta);
                clip_instance(instance, current, corners);
            }
        }
    }

    pub fn fade_popup(&mut self, start: (usize, usize), factor: f32) {
        if factor >= 1.0 {
            return;
        }
        let material = start.0.min(self.popup.material.len());
        let ink = start.1.min(self.popup.ink.len());
        for instance in &mut self.popup.material[material..] {
            scale_alpha(instance, factor);
        }
        for instance in &mut self.popup.ink[ink..] {
            scale_alpha(instance, factor);
        }
    }

    pub fn replay_popup(&mut self, key: u64) -> bool {
        let Some(transition) = self.transitions.get(&key) else {
            return false;
        };
        let Some(id) = transition.closing else {
            return false;
        };
        let Some(frame) = self.glass.current_frame(id) else {
            return false;
        };
        let corner = self.glass.current_corner(id).map_or(24.0, |(r, _)| r);
        let current = Rect::from_min_size(
            Vec2::new(frame.x, frame.y),
            Vec2::new(frame.width, frame.height),
        );
        let delta = Vec2::new(
            current.center().x - transition.body.center().x,
            current.min.y - transition.body.min.y,
        );
        let corners = Corners::all(corner);
        let factor = closing_alpha(frame, transition.body);
        let snapshot = transition.snapshot.clone();
        for instance in &snapshot.material {
            let mut instance = *instance;
            instance.translate(delta);
            clip_instance(&mut instance, current, corners);
            scale_alpha(&mut instance, factor);
            self.popup.material.push(instance);
        }
        for instance in &snapshot.ink {
            let mut instance = *instance;
            instance.translate(delta);
            clip_instance(&mut instance, current, corners);
            scale_alpha(&mut instance, factor);
            self.popup.ink.push(instance);
        }
        true
    }

    fn settle_transitions(&mut self) {
        let now = self.time;
        let mut collapsing = Vec::new();
        for transition in self.transitions.values_mut() {
            if transition.popover.is_some() && transition.touched < now - 0.05 {
                let id = transition.popover.take().expect("the transition is open");
                transition.closing = Some(id);
                transition.closing_at = Some(now);
                collapsing.push((id, transition.anchor));
            }
        }
        if !collapsing.is_empty() {
            self.floating_draining = true;
        }
        for (id, anchor) in collapsing {
            self.glass.set_material(anchor, Material::regular());
            self.glass.snap_presence(anchor, 1.0);
            self.glass
                .collapse_into(id, anchor, GlassSpring::new(0.35, 0.0));
        }
        let expired = self
            .transitions
            .iter()
            .filter(|(_, transition)| transition.closing_at.is_some_and(|at| now - at > 0.8))
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for key in expired {
            if let Some(transition) = self.transitions.remove(&key) {
                self.glass.remove(transition.anchor);
            }
        }
    }

    fn retire_glass(&mut self) {
        let stale = self
            .glass_entries
            .keys()
            .copied()
            .filter(|key| !self.glass_seen.contains(key))
            .collect::<Vec<_>>();
        for key in stale {
            if let Some(entry) = self.glass_entries.remove(&key) {
                self.glass.remove(entry.id);
            }
        }
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
            loose_width: false,
            loose_height: false,
            wrapping: false,
            line: 0.0,
            line_height: 0.0,
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
        self.frames.push(SpanFrame {
            spans: self.span_pool.pop().unwrap_or_default(),
            open: None,
        });
    }

    fn pop_frame(&mut self) {
        if let Some(frame) = self.frames.pop() {
            self.recycle_frame(frame);
        }
    }

    fn recycle_frame(&mut self, mut frame: SpanFrame) {
        frame.spans.clear();
        frame.open = None;
        self.span_pool.push(frame.spans);
    }

    fn recycle_frames(&mut self) {
        while let Some(frame) = self.frames.pop() {
            self.recycle_frame(frame);
        }
    }

    fn begin_span(&mut self, offset: f32, size: f32, direct: bool) {
        let span = Span {
            lists: self.instance_lengths(),
            glass: self.glass_placed.len(),
            hit: self.registry.hit_count(),
            wheel: self.registry.wheel_count(),
            offset,
            size,
            direct,
            baseline: None,
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

    fn note_baseline(&mut self, baseline: f32) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        if let Some(open) = frame.open.as_mut()
            && open.baseline.is_none()
        {
            open.baseline = Some(baseline);
        }
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
        let ends = self.instance_lengths();
        let glass_end = self.glass_placed.len();
        let hit_end = self.registry.hit_count();
        let wheel_end = self.registry.wheel_count();
        let baseline = (axis == Axis::Row && align == Align::Baseline)
            .then(|| {
                spans
                    .iter()
                    .filter_map(|span| span.baseline)
                    .fold(f32::MIN, f32::max)
            })
            .filter(|baseline| *baseline > f32::MIN);
        for index in 0..spans.len() {
            let span = spans[index];
            let cross = if let Some(baseline) = baseline {
                span.baseline.map_or(0.0, |own| baseline - own)
            } else if auto_cross && !span.direct {
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
            let stops = next.map_or(ends, |span| span.lists);
            let glass_stop = next.map_or(glass_end, |span| span.glass);
            let hit_stop = next.map_or(hit_end, |span| span.hit);
            let wheel_stop = next.map_or(wheel_end, |span| span.wheel);
            self.translate_lists(span.lists, stops, delta);
            for entry in span.glass..glass_stop {
                let id = self.glass_placed[entry];
                self.glass.translate(id, delta);
            }
            self.registry.shift_hits(span.hit..hit_stop, delta);
            self.registry.shift_wheels(span.wheel..wheel_stop, delta);
        }
        spans.clear();
        self.span_pool.push(spans);
    }

    fn instance_lengths(&self) -> [usize; LIST_COUNT] {
        [
            self.content.material.len(),
            self.chrome.material.len(),
            self.chrome.ink.len(),
            self.labels.material.len(),
            self.labels.ink.len(),
            self.popup.material.len(),
            self.popup.ink.len(),
            self.hud.material.len(),
            self.hud.ink.len(),
        ]
    }

    fn translate_lists(&mut self, from: [usize; LIST_COUNT], to: [usize; LIST_COUNT], delta: Vec2) {
        translate_range(&mut self.content.material, from[0], to[0], delta);
        translate_range(&mut self.chrome.material, from[1], to[1], delta);
        translate_range(&mut self.chrome.ink, from[2], to[2], delta);
        translate_range(&mut self.labels.material, from[3], to[3], delta);
        translate_range(&mut self.labels.ink, from[4], to[4], delta);
        translate_range(&mut self.popup.material, from[5], to[5], delta);
        translate_range(&mut self.popup.ink, from[6], to[6], delta);
        translate_range(&mut self.hud.material, from[7], to[7], delta);
        translate_range(&mut self.hud.ink, from[8], to[8], delta);
    }

    fn translate_ranges(&mut self, ranges: &[Range<usize>; LIST_COUNT], delta: Vec2) {
        translate_range(
            &mut self.content.material,
            ranges[0].start,
            ranges[0].end,
            delta,
        );
        translate_range(
            &mut self.chrome.material,
            ranges[1].start,
            ranges[1].end,
            delta,
        );
        translate_range(&mut self.chrome.ink, ranges[2].start, ranges[2].end, delta);
        translate_range(
            &mut self.labels.material,
            ranges[3].start,
            ranges[3].end,
            delta,
        );
        translate_range(&mut self.labels.ink, ranges[4].start, ranges[4].end, delta);
        translate_range(
            &mut self.popup.material,
            ranges[5].start,
            ranges[5].end,
            delta,
        );
        translate_range(&mut self.popup.ink, ranges[6].start, ranges[6].end, delta);
        translate_range(
            &mut self.hud.material,
            ranges[7].start,
            ranges[7].end,
            delta,
        );
        translate_range(&mut self.hud.ink, ranges[8].start, ranges[8].end, delta);
    }

    fn layer_len(&self) -> usize {
        match self.layer {
            Layer::Content => self.content.material.len(),
            Layer::Chrome => self.chrome.material.len(),
            Layer::Labels => self.labels.material.len(),
            Layer::Popup => self.popup.material.len(),
            Layer::Hud => self.hud.material.len(),
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
            Length::Minimum(minimum) => available.max(minimum),
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
        let mut area = *self.area();
        let axis = area.axis;
        let requested = size.x.max(0.0);
        if axis == Axis::Row && area.wrapping {
            let fits = area.cursor.x + requested <= area.rect.max.x + 0.01
                || area.cursor.x <= area.rect.min.x + 0.01;
            if !fits {
                area.line += area.line_height + area.gap;
                area.cursor.x = area.rect.min.x;
                area.line_height = 0.0;
            }
        }
        let remaining = area.remaining();
        let width = if fill && axis == Axis::Column {
            remaining.x
        } else if area.loose_width {
            size.x.max(0.0)
        } else {
            size.x.min(remaining.x).max(0.0)
        };
        let height = if fill && axis == Axis::Row {
            remaining.y
        } else {
            size.y.max(0.0)
        };
        let size = Vec2::new(width, height);
        let align = area.align;
        let origin = match axis {
            Axis::Column => Vec2::new(
                area.rect.min.x + (area.rect.width() - size.x) * cross_factor(align),
                area.cursor.y,
            ),
            Axis::Row if area.wrapping => Vec2::new(area.cursor.x, area.rect.min.y + area.line),
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
        match axis {
            Axis::Column => {
                area.cursor.y += size.y + area.gap;
                area.cross = area.cross.max(size.x);
                area.main = (area.cursor.y - area.origin.y - area.gap).max(0.0);
            }
            Axis::Row if area.wrapping => {
                area.cursor.x += size.x + area.gap;
                area.line_height = area.line_height.max(size.y);
                area.cross = area.cross.max(area.line + area.line_height);
                area.main = area
                    .main
                    .max((area.cursor.x - area.rect.min.x - area.gap).max(0.0));
            }
            Axis::Row => {
                area.cursor.x += size.x + area.gap;
                area.cross = area.cross.max(size.y);
                area.main = (area.cursor.x - area.origin.x - area.gap).max(0.0);
            }
        }
        *self.area_mut() = area;
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
        let loose_width = matches!(block.width, Length::Auto)
            || (parent.loose_width && matches!(block.width, Length::Fill | Length::Maximum(_)));
        let loose_height = matches!(block.height, Length::Auto)
            || (parent.loose_height && matches!(block.height, Length::Fill | Length::Maximum(_)));
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
                block
                    .background
                    .map_or(Corners::ZERO, |surface| surface.corners),
            )
        } else {
            parent.clip
        };
        let index = self.layer_len();
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
            loose_width,
            loose_height,
            wrapping: block.wrapping,
            line: 0.0,
            line_height: 0.0,
        });
        self.push_frame();
        self.begin_span(0.0, 0.0, true);
        content(self);
        let area = self.areas.pop().expect("a layout area is active");
        let auto_cross = match axis {
            Axis::Row => {
                matches!(block.height, Length::Auto)
                    || (parent.loose_height && matches!(block.height, Length::Fill))
            }
            Axis::Column => {
                matches!(block.width, Length::Auto)
                    || (parent.loose_width && matches!(block.width, Length::Fill))
            }
        };
        let auto_main = match axis {
            Axis::Row => {
                matches!(block.width, Length::Auto)
                    || (parent.loose_width && matches!(block.width, Length::Fill))
            }
            Axis::Column => {
                matches!(block.height, Length::Auto)
                    || (parent.loose_height && matches!(block.height, Length::Fill))
            }
        };
        let inner_main = match axis {
            Axis::Row => area.rect.width(),
            Axis::Column => area.rect.height(),
        };
        let main_space = if auto_main || block.wrapping {
            0.0
        } else {
            (inner_main - area.main).max(0.0)
        };
        self.settle_frame(
            axis,
            block.align,
            block.justify,
            auto_cross && !block.wrapping,
            area.cross,
            main_space,
        );
        let measured = area.content_size() + block.padding.size();
        let width = match block.width {
            Length::Auto => {
                if parent.loose_width {
                    measured.x
                } else {
                    measured.x.min(remaining.x)
                }
            }
            Length::Maximum(maximum) => {
                if parent.loose_width {
                    measured.x.min(maximum)
                } else {
                    measured.x.min(maximum).min(remaining.x)
                }
            }
            Length::Fill if parent.loose_width => measured.x,
            Length::Fill | Length::Fixed(_) | Length::Minimum(_) => width,
        };
        let height = match block.height {
            Length::Auto => measured.y,
            Length::Maximum(maximum) => measured.y.min(maximum),
            Length::Fill if parent.loose_height => measured.y,
            Length::Fill | Length::Fixed(_) | Length::Minimum(_) => height,
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

    pub fn grid(
        &mut self,
        columns: &[Length],
        gap: f32,
        content: impl FnOnce(&mut Grid<'_>),
    ) -> Rect {
        if columns.is_empty() {
            return Rect::ZERO;
        }
        let widths = column_widths(columns, self.available().x, gap);
        let total = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
        self.block(
            Axis::Column,
            Block::column()
                .width(Length::Fixed(total))
                .height(Length::Auto)
                .gaps(0.0),
            |ui| {
                let origin = ui.cursor_position();
                let mut grid = Grid {
                    ui,
                    widths: &widths,
                    gap,
                    origin,
                    row: 0,
                    column: 0,
                    row_y: origin.y,
                    total_width: total,
                    align: Align::Center,
                    cells: Vec::new(),
                };
                content(&mut grid);
                grid.finish();
            },
        )
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
            loose_width: false,
            loose_height: false,
            wrapping: false,
            line: 0.0,
            line_height: 0.0,
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
            loose_width: false,
            loose_height: false,
            wrapping: false,
            line: 0.0,
            line_height: 0.0,
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
        match self.layer {
            Layer::Content => Painter::new(&mut self.content, &mut self.text, clip, dark, false),
            Layer::Chrome => Painter::new(&mut self.chrome, &mut self.text, clip, dark, false),
            Layer::Labels => Painter::new(&mut self.labels, &mut self.text, clip, dark, true),
            Layer::Popup => Painter::new(&mut self.popup, &mut self.text, clip, dark, true),
            Layer::Hud => Painter::new(&mut self.hud, &mut self.text, clip, dark, true),
        }
    }

    pub fn painter_in(&mut self, clip: Clip) -> Painter<'_> {
        let dark = self.theme.dark;
        Painter::new(&mut self.content, &mut self.text, clip, dark, false)
    }

    pub fn content_painter(&mut self) -> Painter<'_> {
        let clip = self.area().clip;
        let dark = self.theme.dark;
        Painter::new(&mut self.content, &mut self.text, clip, dark, false)
    }

    pub fn content(&mut self, content: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.layer, Layer::Content);
        content(self);
        self.layer = previous;
    }

    pub fn labels_painter(&mut self) -> Painter<'_> {
        let clip = self.area().clip;
        let dark = self.theme.dark;
        Painter::new(&mut self.labels, &mut self.text, clip, dark, true)
    }

    pub fn hud_painter(&mut self) -> Painter<'_> {
        let clip = self.hud_clip;
        let dark = self.theme.dark;
        Painter::new(&mut self.hud, &mut self.text, clip, dark, true)
    }

    fn paint_background(&mut self, index: usize, rect: Rect, surface: Surface) {
        if rect.is_empty() {
            return;
        }
        let clip = self.area().clip;
        let dark = self.theme.dark;
        let target = match self.layer {
            Layer::Content => &mut self.content.material,
            Layer::Chrome => &mut self.chrome.material,
            Layer::Labels => &mut self.labels.material,
            Layer::Popup => &mut self.popup.material,
            Layer::Hud => &mut self.hud.material,
        };
        let start = target.len();
        let mut instances = LayerInstances {
            material: std::mem::take(target),
            ink: Vec::new(),
        };
        {
            let mut painter = Painter::new(&mut instances, &mut self.text, clip, dark, false);
            painter.outlined(
                rect,
                surface.corners,
                surface.fill,
                surface.border,
                surface.shadow,
            );
        }
        let added = instances.material.len() - start;
        *target = instances.material;
        if added > 0 && index < start {
            target[index..].rotate_right(added);
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
            if matches!(style.align, Align::Start | Align::Baseline) || available <= 0.0 {
                layout.size.x
            } else {
                available
            },
            layout.size.y,
        );
        let rect = self.allocate(size);
        self.note_baseline(rect.min.y + layout.baseline);
        let mut painter = self.painter();
        painter.text(&layout, rect.min, style.color);
        rect
    }

    pub fn text_style_wrapped(&mut self, text: &str, style: TextStyle, width: f32) -> Rect {
        let layout = self.layout_text(text, style, width);
        let rect = self.allocate(layout.size);
        self.note_baseline(rect.min.y + layout.baseline);
        let mut painter = self.painter();
        painter.text(&layout, rect.min, style.color);
        rect
    }

    pub fn labels_scope(&mut self, rect: Rect, content: impl FnOnce(&mut Self)) -> f32 {
        let clip = self.area().clip;
        self.scoped(rect, clip, Layer::Labels, content)
    }

    pub fn popup_scope(&mut self, rect: Rect, content: impl FnOnce(&mut Self)) -> f32 {
        let clip = self.popup_clip;
        self.scoped(rect, clip, Layer::Popup, content)
    }

    pub fn hud_scope(&mut self, rect: Rect, content: impl FnOnce(&mut Self)) -> f32 {
        let clip = self.hud_clip;
        self.scoped(rect, clip, Layer::Hud, content)
    }

    fn scoped(
        &mut self,
        rect: Rect,
        parent: Clip,
        layer: Layer,
        content: impl FnOnce(&mut Self),
    ) -> f32 {
        let clip = parent.intersect(rect, Corners::ZERO);
        let previous = std::mem::replace(&mut self.layer, layer);
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
            loose_width: false,
            loose_height: false,
            wrapping: false,
            line: 0.0,
            line_height: 0.0,
        });
        self.push_frame();
        content(self);
        self.pop_frame();
        let area = self.areas.pop().expect("a layout area is active");
        self.layer = previous;
        area.main
    }

    pub fn text_in(&mut self, rect: Rect, text: &str, style: TextStyle, vertical: Align) -> Rect {
        let mut painter = self.painter();
        painter.text_in(rect, text, style, vertical);
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
        let motion = self.motion(motion);
        self.registry
            .spring(mix(key, PROPERTY_SALT), target, motion, self.elapsed)
    }

    pub fn spring_at(&self, key: u64, fallback: f32) -> f32 {
        self.registry
            .spring_value(mix(key, PROPERTY_SALT), fallback)
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
        let max = if wrap || !matches!(style.align, Align::Start | Align::Baseline) {
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

    pub fn id_index(&self, salt: &str, index: usize) -> u64 {
        mix(self.id(salt), index as u64)
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
        self.pointer_over_widget()
    }

    pub fn pointer_over_widget(&self) -> bool {
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

    pub fn focusable(&mut self, key: u64, rect: Rect, corners: Corners) -> bool {
        let clip = self.area().clip;
        if rect.is_empty() || !clip.rect.intersects(rect) {
            return false;
        }
        self.registry.register_focus(key);
        let focused = self.registry.focused(key);
        if focused && self.registry.focus_visible() {
            self.focus_rings.push((rect, corners, clip));
        }
        focused
    }

    pub fn focus_key(&mut self, key: u64) {
        self.registry.set_focus(key);
    }

    pub fn focused(&self, key: u64) -> bool {
        self.registry.focused(key)
    }

    pub fn focus_visible(&self) -> bool {
        self.registry.focus_visible()
    }

    pub fn activated(&self, key: u64) -> bool {
        if self.editing() || !self.registry.focused(key) {
            return false;
        }
        self.input.keys().iter().any(|event| {
            event.pressed
                && !event.repeat
                && matches!(
                    event.key,
                    Some(crate::input::Key::Enter | crate::input::Key::Space)
                )
        })
    }

    pub fn animate(&mut self, key: u64, target: f32, motion: Motion) -> f32 {
        let motion = self.motion(motion);
        self.registry
            .spring(mix(key, PROPERTY_SALT), target, motion, self.elapsed)
    }

    pub fn surface(&mut self, key: u64, role: Role, response: &Response) -> Surface {
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
        let rest = self.theme.surface(role, State::Rest);
        let hovered = self.theme.surface(role, State::Hover);
        let pressed = self.theme.surface(role, State::Pressed);
        rest.lerp(hovered, hover).lerp(pressed, press)
    }

    pub fn surface_disabled(&mut self, role: Role) -> Surface {
        self.theme.surface(role, State::Disabled)
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
        self.registry
            .register_wheel(key, outer, self.layer == Layer::Popup);
        self.registry.register(key, outer, self.area().clip.rect);
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
            loose_width: false,
            loose_height: false,
            wrapping: false,
            line: 0.0,
            line_height: 0.0,
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
        let thumb = match axis {
            Axis::Column => {
                let track = Rect::from_min_size(
                    Vec2::new(outer.max.x - 8.0, outer.min.y + 4.0),
                    Vec2::new(4.0, (outer.height() - 8.0).max(8.0)),
                );
                let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
                let thumb_height = (track.height() * ratio).max(24.0).min(track.height());
                let travel = track.height() - thumb_height;
                let position = if scroll.limit() > 0.0 {
                    (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Rect::from_min_size(
                    Vec2::new(track.min.x, track.min.y + travel * position),
                    Vec2::new(track.width(), thumb_height),
                )
            }
            Axis::Row => {
                let track = Rect::from_min_size(
                    Vec2::new(outer.min.x + 4.0, outer.max.y - 8.0),
                    Vec2::new((outer.width() - 8.0).max(8.0), 4.0),
                );
                let ratio = (scroll.viewport / scroll.content.max(1.0)).clamp(0.1, 1.0);
                let thumb_width = (track.width() * ratio).max(24.0).min(track.width());
                let travel = track.width() - thumb_width;
                let position = if scroll.limit() > 0.0 {
                    (scroll.offset / scroll.limit()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                Rect::from_min_size(
                    Vec2::new(track.min.x + travel * position, track.min.y),
                    Vec2::new(thumb_width, track.height()),
                )
            }
        };
        let color = self.theme.palette.text_quaternary.with_alpha(0.7 * bar);
        let mut painter = self.hud_painter();
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

    pub fn popup_open_any(&self) -> bool {
        self.registry.popup().is_some()
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
        let style = TextStyle::new(
            self.theme.font(Level::Caption),
            self.theme.palette.text_primary,
        );
        let layout = self.layout_text(&text, style, 0.0);
        let pointer = self.input.position();
        let size = layout.size + Vec2::new(14.0, 10.0);
        let mut origin = pointer + Vec2::new(14.0, 18.0);
        if origin.x + size.x > self.size.x - 8.0 {
            origin.x = self.size.x - 8.0 - size.x;
        }
        if origin.y + size.y > self.size.y - 8.0 {
            origin.y = pointer.y - 14.0 - size.y;
        }
        let rect = Rect::from_min_size(origin, size);
        self.glass(u64::MAX, rect.expand(2.0), GlassStyle::tooltip());
        let clip = self.hud_clip;
        let dark = self.theme.dark;
        {
            let mut painter = Painter::new(&mut self.hud, &mut self.text, clip, dark, true);
            painter.text(
                &layout,
                rect.min + Vec2::new(7.0, 5.0),
                self.theme.palette.text_primary,
            );
        }
    }

    pub fn instances(&self) -> &[Instance] {
        &self.content.material
    }

    pub fn chrome_instances(&self) -> &[Instance] {
        &self.chrome.material
    }

    pub fn atlas(&self) -> &crate::atlas::Atlas {
        self.text.atlas()
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        self.text.flush(queue);
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
        let mut instances = LayerInstances::default();
        {
            let mut painter = Painter::new(&mut instances, &mut self.text, clip, dark, false);
            content(&mut painter);
        }
        self.text.begin(self.scale);
        instances.material
    }

    pub fn description(&self) -> String {
        format!(
            "{} - {} instances",
            self.text.description(),
            self.content.len()
                + self.chrome.len()
                + self.labels.len()
                + self.popup.len()
                + self.hud.len()
        )
    }
}

impl<'a> Grid<'a> {
    pub fn align(&mut self, align: Align) -> &mut Self {
        self.align = align;
        self
    }

    pub fn cell(&mut self, content: impl FnOnce(&mut Ui)) {
        if self.column == 0 {
            self.begin_row();
        }
        let column = self.column;
        let offset = self.widths[..column].iter().sum::<f32>() + self.gap * column as f32;
        let rect = Rect::from_min_size(
            Vec2::new(self.origin.x + offset, self.row_y),
            Vec2::new(self.widths[column], 0.0),
        );
        let ui = &mut *self.ui;
        let clip = ui.clip();
        let lists = ui.instance_lengths();
        let glass = ui.glass_placed.len();
        let hits = ui.registry.hit_count();
        let wheels = ui.registry.wheel_count();
        ui.ids
            .push(mix(mix(self.row as u64, column as u64), GRID_SALT));
        ui.areas.push(Area {
            rect,
            cursor: rect.min,
            axis: Axis::Column,
            gap: 0.0,
            align: Align::Start,
            clip,
            origin: rect.min,
            cross: 0.0,
            main: 0.0,
            loose_width: false,
            loose_height: false,
            wrapping: false,
            line: 0.0,
            line_height: 0.0,
        });
        ui.push_frame();
        content(ui);
        ui.pop_frame();
        let area = ui.areas.pop().expect("a layout area is active");
        ui.ids.pop();
        let ends = ui.instance_lengths();
        self.cells.push(Cell {
            lists: std::array::from_fn(|index| lists[index]..ends[index]),
            glass: glass..ui.glass_placed.len(),
            hits: hits..ui.registry.hit_count(),
            wheels: wheels..ui.registry.wheel_count(),
            height: area.main,
        });
        self.column += 1;
        if self.column == self.widths.len() {
            self.finish_row();
        }
    }

    fn begin_row(&mut self) {
        if self.row > 0 {
            self.ui.area_mut().cursor.y += self.gap;
        }
        self.row_y = self.ui.cursor_position().y;
    }

    fn finish_row(&mut self) {
        if self.cells.is_empty() {
            return;
        }
        let height = self
            .cells
            .iter()
            .fold(0.0f32, |height, cell| height.max(cell.height));
        let factor = cross_factor(self.align);
        for cell in self.cells.drain(..) {
            let delta = Vec2::new(0.0, (height - cell.height) * factor);
            if delta.y == 0.0 {
                continue;
            }
            self.ui.translate_ranges(&cell.lists, delta);
            for entry in cell.glass.clone() {
                let id = self.ui.glass_placed[entry];
                self.ui.glass.translate(id, delta);
            }
            self.ui.registry.shift_hits(cell.hits, delta);
            self.ui.registry.shift_wheels(cell.wheels, delta);
        }
        let area = self.ui.area_mut();
        area.cursor.y += height;
        area.main = (area.cursor.y - area.origin.y).max(0.0);
        area.cross = self.total_width;
        self.row += 1;
        self.column = 0;
    }

    fn finish(&mut self) {
        if self.column > 0 {
            self.finish_row();
        }
    }
}

fn column_widths(columns: &[Length], available: f32, gap: f32) -> Vec<f32> {
    let fills = columns
        .iter()
        .filter(|column| matches!(column, Length::Auto | Length::Fill))
        .count();
    let fixed = columns
        .iter()
        .map(|column| match column {
            Length::Auto | Length::Fill => 0.0,
            Length::Fixed(value) | Length::Minimum(value) | Length::Maximum(value) => *value,
        })
        .sum::<f32>();
    let gaps = gap * columns.len().saturating_sub(1) as f32;
    let share = (available - fixed - gaps).max(0.0) / fills.max(1) as f32;
    columns
        .iter()
        .map(|column| match column {
            Length::Auto | Length::Fill => share,
            Length::Fixed(value) => *value,
            Length::Minimum(value) => value.max(share),
            Length::Maximum(value) => value.min(share),
        })
        .collect()
}

pub fn resolve(length: Length, available: f32) -> f32 {
    match length {
        Length::Auto => available,
        Length::Fill => available,
        Length::Fixed(value) => value,
        Length::Minimum(minimum) => available.max(minimum),
        Length::Maximum(maximum) => available.min(maximum),
    }
}

fn cross_factor(align: Align) -> f32 {
    match align {
        Align::Start | Align::Baseline => 0.0,
        Align::Center => 0.5,
        Align::End => 1.0,
    }
}

fn glass_rect(rect: Rect) -> GlassRect {
    GlassRect::new(rect.min.x, rect.min.y, rect.width(), rect.height())
}

fn expand_alpha(scene: &GlassScene, id: GlassId, body: Rect) -> f32 {
    let Some(frame) = scene.current_frame(id) else {
        return 1.0;
    };
    smoothstep(0.28, 1.0, glass_ratio(frame, body))
}

fn closing_alpha(frame: GlassRect, body: Rect) -> f32 {
    smoothstep(0.0, 0.65, glass_ratio(frame, body))
}

fn glass_ratio(frame: GlassRect, body: Rect) -> f32 {
    let width = frame.width / body.width().max(1.0);
    let height = frame.height / body.height().max(1.0);
    width.min(height).clamp(0.0, 1.0)
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn translate_range(list: &mut [Instance], from: usize, to: usize, delta: Vec2) {
    let start = from.min(list.len());
    let end = to.min(list.len());
    if start >= end {
        return;
    }
    for instance in &mut list[start..end] {
        instance.translate(delta);
    }
}

fn hash_layer(layer: &LayerInstances, seed: u64) -> u64 {
    let mut hash = seed;
    for list in [&layer.material, &layer.ink] {
        hash ^= list.len() as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
        for instance in list {
            for word in bytemuck::cast_slice::<Instance, u32>(std::slice::from_ref(instance)) {
                hash ^= u64::from(*word);
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    hash
}

fn scale_alpha(instance: &mut Instance, factor: f32) {
    instance.color[3] *= factor;
    instance.color2[3] *= factor;
}

fn clip_instance(instance: &mut Instance, bounds: Rect, corners: Corners) {
    let current = Rect::from_min_size(
        Vec2::new(instance.clip[0], instance.clip[1]),
        Vec2::new(instance.clip[2], instance.clip[3]),
    );
    let intersected = current.intersect(bounds);
    let resolved = corners.resolved(bounds.size());
    instance.clip = [
        intersected.min.x,
        intersected.min.y,
        intersected.width().max(0.0),
        intersected.height().max(0.0),
    ];
    instance.clip_corners = [
        instance.clip_corners[0].max(resolved.top_left),
        instance.clip_corners[1].max(resolved.top_right),
        instance.clip_corners[2].max(resolved.bottom_right),
        instance.clip_corners[3].max(resolved.bottom_left),
    ];
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Event;

    fn frame(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
        ui.resize([400, 300]);
        ui.begin(Vec2::new(400.0, 300.0), 1.0, 0.0);
        ui.root(content);
        let _ = ui.end();
    }

    #[test]
    fn text_in_centers_the_glyph_ink() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let rect = Rect::from_min_size(Vec2::new(20.0, 20.0), Vec2::new(200.0, 40.0));
        let style = TextStyle::new(ui.theme().font(Level::Body), crate::Color::WHITE)
            .aligned(Align::Center);
        frame(&mut ui, |ui| {
            ui.text_in(rect, "Hg", style, Align::Center);
        });
        let glyphs = ui
            .chrome_instances()
            .iter()
            .filter(|instance| instance.meta[0] == crate::paint::KIND_GLYPH);
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        let mut count = 0;
        for instance in glyphs {
            let origin = Vec2::new(instance.rect[0], instance.rect[1]);
            let size = Vec2::new(instance.rect[2], instance.rect[3]);
            min = min.min(origin);
            max = max.max(origin + size);
            count += 1;
        }
        assert!(count > 0, "the text emits glyphs");
        let center = (min + max) * 0.5;
        assert!((center.x - rect.center().x).abs() <= 1.0);
        assert!((center.y - rect.center().y).abs() <= 1.0);
    }

    fn step(ui: &mut Ui, time: f64, events: &[Event], content: impl FnOnce(&mut Ui)) {
        for event in events {
            ui.input_mut().push(event.clone());
        }
        ui.begin(Vec2::new(400.0, 300.0), 1.0, time);
        ui.root(content);
        let _ = ui.end();
    }

    #[test]
    fn toggle_drag_changes_the_value() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let rect = Rect::from_min_size(Vec2::new(20.0, 20.0), Vec2::new(42.0, 26.0));
        let mut value = false;
        let mut changed = None;
        step(&mut ui, 0.0, &[], |ui| {
            ui.place(rect, |ui| {
                changed = crate::widget::toggle(ui, "toggle", value);
            });
        });
        step(
            &mut ui,
            1.0 / 60.0,
            &[crate::input::Event::Moved(Vec2::new(30.0, 33.0))],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::toggle(ui, "toggle", value);
                });
            },
        );
        step(
            &mut ui,
            2.0 / 60.0,
            &[crate::input::Event::Down(crate::input::Button::Primary)],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::toggle(ui, "toggle", value);
                });
            },
        );
        step(
            &mut ui,
            3.0 / 60.0,
            &[crate::input::Event::Moved(Vec2::new(56.0, 33.0))],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::toggle(ui, "toggle", value);
                });
            },
        );
        if let Some(next) = changed.take() {
            value = next;
        }
        step(
            &mut ui,
            4.0 / 60.0,
            &[crate::input::Event::Up(crate::input::Button::Primary)],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::toggle(ui, "toggle", value);
                });
            },
        );
        if let Some(next) = changed.take() {
            value = next;
        }
        assert!(value, "dragging the knob to the right turns it on");
    }

    #[test]
    fn segmented_drag_selects_the_cell_under_the_pointer() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let rect = Rect::from_min_size(Vec2::new(20.0, 80.0), Vec2::new(300.0, 28.0));
        let mut selected = 0;
        let mut changed = None;
        step(&mut ui, 0.0, &[], |ui| {
            ui.place(rect, |ui| {
                changed = crate::widget::segmented_fill(
                    ui,
                    "segmented",
                    &["One", "Two", "Three"],
                    selected,
                );
            });
        });
        step(
            &mut ui,
            1.0 / 60.0,
            &[crate::input::Event::Moved(Vec2::new(72.0, 94.0))],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::segmented_fill(
                        ui,
                        "segmented",
                        &["One", "Two", "Three"],
                        selected,
                    );
                });
            },
        );
        step(
            &mut ui,
            2.0 / 60.0,
            &[crate::input::Event::Down(crate::input::Button::Primary)],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::segmented_fill(
                        ui,
                        "segmented",
                        &["One", "Two", "Three"],
                        selected,
                    );
                });
            },
        );
        step(
            &mut ui,
            3.0 / 60.0,
            &[crate::input::Event::Moved(Vec2::new(268.0, 94.0))],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::segmented_fill(
                        ui,
                        "segmented",
                        &["One", "Two", "Three"],
                        selected,
                    );
                });
            },
        );
        if let Some(next) = changed.take() {
            selected = next;
        }
        assert_eq!(
            selected, 2,
            "dragging across the segments selects the target"
        );
        step(
            &mut ui,
            4.0 / 60.0,
            &[crate::input::Event::Up(crate::input::Button::Primary)],
            |ui| {
                ui.place(rect, |ui| {
                    changed = crate::widget::segmented_fill(
                        ui,
                        "segmented",
                        &["One", "Two", "Three"],
                        selected,
                    );
                });
            },
        );
    }

    #[test]
    fn columns_share_the_remaining_space() {
        let widths = column_widths(
            &[Length::Fixed(100.0), Length::Fill, Length::Fill],
            400.0,
            10.0,
        );
        assert_eq!(widths, [100.0, 140.0, 140.0]);
    }

    #[test]
    fn grid_centers_cells_and_advances_rows() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let mut keys = [0u64; 2];
        frame(&mut ui, |ui| {
            ui.grid(&[Length::Fixed(100.0), Length::Fill], 10.0, |grid| {
                grid.cell(|ui| {
                    let rect = ui.allocate(Vec2::new(ui.available().x, 40.0));
                    keys[0] = ui.id("cell");
                    ui.interact(keys[0], rect);
                });
                grid.cell(|ui| {
                    let rect = ui.allocate(Vec2::new(ui.available().x, 20.0));
                    keys[1] = ui.id("cell");
                    ui.interact(keys[1], rect);
                });
            });
        });
        let hits = ui.registry.hits();
        let first = hits
            .iter()
            .find(|hit| hit.key == keys[0])
            .expect("first cell");
        let second = hits
            .iter()
            .find(|hit| hit.key == keys[1])
            .expect("second cell");
        assert_eq!(first.rect.min.x, 0.0);
        assert_eq!(second.rect.min.x, 110.0);
        assert_eq!(second.rect.min.y - first.rect.min.y, 10.0);
    }

    #[test]
    fn auto_width_columns_measure_their_content() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let mut rect = Rect::ZERO;
        frame(&mut ui, |ui| {
            ui.column(Block::column().width(Length::Auto), |ui| {
                rect = ui.label("Auto width", Level::Body);
            });
        });
        assert!(rect.width() > 0.0);
    }

    #[test]
    fn wrapping_rows_start_a_new_line() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let mut cells = [Rect::ZERO; 3];
        let mut row = Rect::ZERO;
        frame(&mut ui, |ui| {
            row = ui.row(
                Block::row()
                    .width(Length::Fixed(100.0))
                    .gaps(5.0)
                    .wrapping(),
                |ui| {
                    for cell in &mut cells {
                        *cell = ui.allocate(Vec2::new(40.0, 10.0));
                    }
                },
            );
        });
        assert_eq!(cells[0].min, Vec2::new(0.0, 0.0));
        assert_eq!(cells[1].min, Vec2::new(45.0, 0.0));
        assert_eq!(cells[2].min, Vec2::new(0.0, 15.0));
        assert_eq!(row.height(), 25.0);
    }

    #[test]
    fn instance_layout_is_packed() {
        assert_eq!(std::mem::size_of::<Instance>(), 144);
    }

    #[test]
    fn paints_instances_into_the_color_target() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        frame(&mut ui, |ui| {
            ui.painter().rect(
                Rect::from_min_size(Vec2::ZERO, Vec2::splat(64.0)),
                Corners::ZERO,
                crate::color::Color::WHITE,
            );
        });
        ui.resize([64, 64]);
        let target = grana_gpu::Target::new(
            &device,
            "grana.ui.test",
            [64, 64],
            crate::test_gpu::format(),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.ui.test"),
        });
        ui.paint(&mut encoder, &target.view);
        queue.submit([encoder.finish()]);
        let (pixels, size, _) = ui.renderer_mut().read_color();
        assert_eq!(size, [64, 64]);
        assert!(
            pixels.chunks(4).any(|pixel| pixel[3] > 0),
            "the instances reach the colour target"
        );
    }

    #[test]
    fn combo_chooses_with_the_keyboard() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let items = [
            (0u8, "A".to_string()),
            (1, "B".to_string()),
            (2, "C".to_string()),
        ];
        let mut chosen = None;
        let mut render = |ui: &mut Ui| {
            ui.resize([400, 300]);
            ui.begin(Vec2::new(400.0, 300.0), 1.0, 0.0);
            ui.root(|ui| {
                chosen = crate::popover::combo(ui, "combo", 180.0, &items, Some(0));
            });
            let _ = ui.end();
        };
        render(&mut ui);
        ui.input_mut()
            .push(crate::input::Event::Moved(Vec2::new(10.0, 10.0)));
        ui.input_mut()
            .push(crate::input::Event::Down(crate::input::Button::Primary));
        render(&mut ui);
        ui.input_mut()
            .push(crate::input::Event::Up(crate::input::Button::Primary));
        render(&mut ui);
        ui.input_mut().push(crate::input::Event::Key {
            key: crate::input::Key::Down,
            pressed: true,
            repeat: false,
        });
        render(&mut ui);
        ui.input_mut().push(crate::input::Event::Key {
            key: crate::input::Key::Enter,
            pressed: true,
            repeat: false,
        });
        render(&mut ui);
        assert_eq!(chosen, Some(1));
    }

    #[test]
    fn scrolled_controls_receive_clicks() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let mut toggled = None;
        let mut time = 0.0;
        for step in 0..4 {
            time += 0.1;
            match step {
                1 => ui
                    .input_mut()
                    .push(crate::input::Event::Moved(Vec2::new(21.0, 13.0))),
                2 => {
                    ui.input_mut()
                        .push(crate::input::Event::Down(crate::input::Button::Primary));
                }
                3 => ui
                    .input_mut()
                    .push(crate::input::Event::Up(crate::input::Button::Primary)),
                _ => {}
            }
            ui.resize([400, 300]);
            ui.begin(Vec2::new(400.0, 300.0), 1.0, time);
            ui.root(|ui| {
                ui.scroll(ui.id("scroll"), Length::Fill, |ui| {
                    if let Some(value) = crate::widget::toggle(ui, "toggle", false) {
                        toggled = Some(value);
                    }
                });
            });
            let _ = ui.end();
        }
        assert_eq!(toggled, Some(true));
    }

    #[test]
    fn text_field_returns_the_committed_text() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let key = ui.id("field");
        let style = TextStyle::new(ui.theme().font(Level::Body), crate::color::Color::WHITE);
        let mut result = None;
        ui.input_mut()
            .push(crate::input::Event::Text("New".to_string()));
        ui.input_mut().push(crate::input::Event::Key {
            key: crate::input::Key::Enter,
            pressed: true,
            repeat: false,
        });
        ui.resize([400, 300]);
        ui.begin(Vec2::new(400.0, 300.0), 1.0, 0.0);
        ui.begin_edit(key, "Old");
        ui.root(|ui| {
            result = crate::widget::text_field(
                ui,
                "field",
                Rect::from_min_size(Vec2::ZERO, Vec2::new(200.0, 30.0)),
                style,
            );
        });
        let _ = ui.end();
        assert_eq!(
            result,
            Some((crate::edit::Outcome::Commit, "New".to_string()))
        );
    }

    #[test]
    fn text_area_commits_multiline_text() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let key = ui.id("area");
        let style = TextStyle::new(ui.theme().font(Level::Body), crate::color::Color::WHITE);
        let mut result = None;
        ui.input_mut()
            .push(crate::input::Event::Text("a\nb".to_string()));
        ui.input_mut().push(crate::input::Event::Key {
            key: crate::input::Key::Tab,
            pressed: true,
            repeat: false,
        });
        ui.resize([400, 300]);
        ui.begin(Vec2::new(400.0, 300.0), 1.0, 0.0);
        ui.begin_edit_multiline(key, "old");
        ui.root(|ui| {
            result = crate::widget::text_area(
                ui,
                "area",
                Rect::from_min_size(Vec2::ZERO, Vec2::new(200.0, 120.0)),
                style,
            );
        });
        let _ = ui.end();
        assert_eq!(
            result,
            Some((crate::edit::Outcome::Commit, "a\nb".to_string()))
        );
    }

    #[test]
    fn baseline_alignment_uses_text_baselines() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let mut second = Rect::ZERO;
        frame(&mut ui, |ui| {
            ui.row(Block::row().align(Align::Baseline).gaps(20.0), |ui| {
                let _ = ui.label("A", Level::LargeTitle);
                second = ui.label("B", Level::Micro);
            });
        });
        let glyphs = ui
            .instances()
            .iter()
            .filter(|instance| instance.meta[0] == crate::paint::KIND_GLYPH)
            .collect::<Vec<_>>();
        let bottom = |left: bool| {
            glyphs
                .iter()
                .filter(|glyph| (glyph.rect[0] < second.min.x) == left)
                .map(|glyph| glyph.rect[1] + glyph.rect[3])
                .fold(f32::MIN, f32::max)
        };
        assert!((bottom(true) - bottom(false)).abs() < 2.0);
    }

    #[test]
    fn tab_focuses_and_enter_activates() {
        let (device, queue) = crate::test_gpu::gpu();
        let mut ui = Ui::new(&device, &queue, crate::test_gpu::format());
        let first = ui.id("first");
        let mut clicked = false;
        let mut render = |ui: &mut Ui| {
            ui.resize([400, 300]);
            ui.begin(Vec2::new(400.0, 300.0), 1.0, 0.0);
            ui.root(|ui| {
                clicked = crate::widget::button(ui, "first", "First", Role::Control);
                crate::widget::button(ui, "second", "Second", Role::Quiet);
            });
            let _ = ui.end();
        };
        render(&mut ui);
        ui.input_mut().push(crate::input::Event::Key {
            key: crate::input::Key::Tab,
            pressed: true,
            repeat: false,
        });
        render(&mut ui);
        assert!(ui.focused(first));
        ui.input_mut().push(crate::input::Event::Key {
            key: crate::input::Key::Enter,
            pressed: true,
            repeat: false,
        });
        render(&mut ui);
        assert!(clicked);
    }
}
