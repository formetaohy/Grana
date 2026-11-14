use std::collections::HashMap;

use glam::Vec2;
use grana_i18n::Language;
use slotmap::SecondaryMap;

use crate::clipboard::{Clipboard, NullClipboard};
use crate::element::{Axis, CanvasPaint, Element, Key, Layer, Layout, Length};
use crate::geometry::Rect;
use crate::image::ImageAtlas;
use crate::input::{Cursor, Input, Key as InputKey};
use crate::layout::LayoutEngine;
use crate::motion::{Animated, Motion};
use crate::node::NodeId;
use crate::paint::{Clip, Instances, LayerBuffer, Painter};
use crate::render::{PaintFrame, Renderer};
use crate::text::{Text, TextLayout, TextStyle};
use crate::theme::Theme;
use crate::tree::Tree;

pub(crate) const CLICK_SLOP: f32 = 3.0;
pub(crate) const TOOLTIP_DELAY: f32 = 0.55;

#[derive(Default)]
pub(crate) struct Change {
    pub reset: bool,
    pub scene: bool,
}

pub struct Ui<A = ()> {
    pub(crate) renderer: Renderer,
    pub(crate) text: Text,
    pub(crate) images: ImageAtlas,
    pub(crate) theme: Theme,
    pub(crate) language: Language,
    pub(crate) input: Input,
    pub(crate) tree: Tree<A>,
    pub(crate) animations: HashMap<u64, Animated>,
    pub(crate) layers: [LayerBuffer; Layer::ALL.len()],
    pub(crate) scratch: Instances,
    pub(crate) overlay: LayerBuffer,
    pub(crate) change: Change,
    pub(crate) scale: f32,
    pub(crate) size: Vec2,
    pub(crate) time: f32,
    pub(crate) elapsed: f32,
    pub(crate) actions: Vec<A>,
    pub(crate) hits: Vec<crate::hit::Hit>,
    pub(crate) hover: Option<NodeId>,
    pub(crate) press: Option<NodeId>,
    pub(crate) drag: Option<NodeId>,
    pub(crate) press_at: Vec2,
    pub(crate) focus: Option<NodeId>,
    pub(crate) focus_visible: bool,
    pub(crate) focus_order: Vec<NodeId>,
    pub(crate) edit: Option<NodeId>,
    pub(crate) edit_drag: bool,
    pub(crate) edit_caret: Option<Rect>,
    pub(crate) popup_anchors: HashMap<Key, NodeId>,
    pub(crate) cursor: Cursor,
    pub(crate) animating: bool,
    pub(crate) inside: bool,
    pub(crate) pointer: Vec2,
    pub(crate) occluders: Vec<Rect>,
    pub(crate) hovered_since: Option<(NodeId, f32)>,
    pub(crate) tooltip: Option<(NodeId, String)>,
    pub(crate) clipboard: Box<dyn Clipboard>,
    pub(crate) scene_revision: Option<u64>,
    pub(crate) preedit: Option<String>,
    pub(crate) animation: Vec<NodeId>,
    pub(crate) scrolling: Vec<NodeId>,
    pub(crate) scrollbar: Option<(NodeId, Vec2, f32)>,
}

impl<A: Clone> Ui<A> {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        pipelines: &grana_gpu::Pipelines,
    ) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format, pipelines),
            text: Text::new(device),
            images: ImageAtlas::new(device),
            theme: Theme::dark(),
            language: Language::English,
            input: Input::new(),
            tree: Tree::new(),
            animations: HashMap::new(),
            layers: std::array::from_fn(|_| LayerBuffer::default()),
            scratch: Instances::default(),
            overlay: LayerBuffer::default(),
            change: Change::default(),
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
            edit_drag: false,
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
            scene_revision: None,
            preedit: None,
            animation: Vec::new(),
            scrolling: Vec::new(),
            scrollbar: None,
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn set_language(&mut self, language: Language) {
        if self.language == language {
            return;
        }
        self.language = language;
        self.invalidate();
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

    pub fn text(&mut self) -> &mut Text {
        &mut self.text
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

    pub fn editing_text(&self) -> Option<&str> {
        let node = self.edit?;
        self.tree.nodes[node]
            .runtime
            .edit
            .as_ref()
            .map(|edit| edit.text())
    }

    pub fn edit_caret(&self) -> Option<Rect> {
        self.edit_caret
    }

    pub fn focus_visible(&self) -> bool {
        self.focus_visible
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: Cursor) {
        self.cursor = cursor;
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    pub fn renderer_and_atlas(
        &mut self,
    ) -> (
        &mut Renderer,
        &crate::atlas::Atlas,
        &crate::image::ImageAtlas,
    ) {
        (&mut self.renderer, self.text.atlas(), &self.images)
    }

    pub fn flush(&mut self, queue: &wgpu::Queue) {
        self.text.flush(queue);
        self.images.flush(queue);
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        if self.renderer.resize(size) {
            self.change.reset = true;
            self.tree.mark_dirty();
        }
    }

    pub fn set_scene_revision(&mut self, revision: u64) {
        if self.scene_revision == Some(revision) {
            return;
        }
        self.scene_revision = Some(revision);
        self.change.scene = true;
    }

    pub fn pointer_over_widget(&self) -> bool {
        self.pointer_over_widget_at(self.pointer)
    }

    pub fn pointer_over_widget_at(&self, position: Vec2) -> bool {
        crate::hit::resolve(&self.hits, position).is_some()
    }

    pub fn pointer_over_chrome(&self) -> bool {
        let pointer = self.pointer;
        self.occluders.iter().any(|rect| rect.contains(pointer))
    }

    pub fn pointer_captured(&self) -> bool {
        self.press.is_some() || self.scrollbar.is_some()
    }

    pub fn popup_open_any(&self) -> bool {
        self.popup_anchors
            .values()
            .any(|id| self.tree.nodes[*id].runtime.open)
    }

    pub fn claims_key(&self, key: InputKey) -> bool {
        let modifiers = self.input.modifiers();
        let command = modifiers.control || modifiers.logo;
        if self.edit.is_some() {
            return match key {
                InputKey::Character(character) if command => {
                    matches!(character.to_ascii_lowercase(), 'a' | 'c' | 'v' | 'x' | 'z')
                }
                InputKey::Character(_) | InputKey::Space => true,
                InputKey::Backspace
                | InputKey::Delete
                | InputKey::Enter
                | InputKey::Escape
                | InputKey::Tab
                | InputKey::Left
                | InputKey::Right
                | InputKey::Up
                | InputKey::Down
                | InputKey::Home
                | InputKey::End => true,
                InputKey::PageUp | InputKey::PageDown => false,
            };
        }
        if self.focus_visible || self.popup_open_any() {
            return matches!(
                key,
                InputKey::Tab
                    | InputKey::Escape
                    | InputKey::Enter
                    | InputKey::Space
                    | InputKey::Left
                    | InputKey::Right
                    | InputKey::Up
                    | InputKey::Down
                    | InputKey::Home
                    | InputKey::End
                    | InputKey::PageUp
                    | InputKey::PageDown
            );
        }
        false
    }

    pub fn update(&mut self, size: Vec2, scale: f32, time: f64) -> Vec<A> {
        let rescaled = self.scale != scale;
        let resized = self.size != size;
        self.size = size;
        self.scale = scale;
        self.time = time as f32;
        self.elapsed = self.input.begin(size, scale, time);
        let preedit = self.input.preedit().map(str::to_owned);
        if preedit != self.preedit {
            self.preedit = preedit;
            if let Some(edit) = self.edit {
                self.tree.nodes[edit].paint_dirty = true;
            }
        }
        self.text.begin(scale);
        self.actions.clear();
        self.cursor = Cursor::Arrow;
        self.animating = false;
        self.edit_caret = None;
        self.inside = self.input.inside();
        self.pointer = self.input.position();
        if resized || rescaled {
            self.tree.mark_dirty();
            self.change.reset = true;
        }
        if rescaled {
            self.invalidate();
        }
        self.advance_scroll();
        if self.tree.focus {
            self.collect_focus();
            self.tree.focus = false;
        }
        self.dispatch();
        self.animate_nodes();
        std::mem::take(&mut self.actions)
    }

    pub fn present<'a>(&mut self, element: Element<'a, A>) {
        let mut canvases: SecondaryMap<NodeId, CanvasPaint<'a>> = SecondaryMap::new();
        let removed = self.tree.apply(element, &mut canvases);
        self.prune(&removed);
        if self.tree.fields {
            self.sync_fields();
            self.tree.fields = false;
        }
        if self.tree.popups {
            self.rebuild_popup_anchors();
            self.tree.popups = false;
        }
        if self.tree.layout {
            self.run_layout();
            self.tree.layout = false;
        }
        self.update_tooltip();
        self.paint_overlays();
        self.paint_layers(&canvases);
    }

    fn run_layout(&mut self) {
        let mut engine = LayoutEngine {
            text: &mut self.text,
            theme: &self.theme,
            screen: self.size,
        };
        let outcome = engine.run(&mut self.tree);
        self.update_scroll_metrics();
        if outcome.geometry || self.tree.hits {
            self.rebuild_hits();
            self.tree.hits = false;
        }
    }

    pub fn finish(&mut self) -> bool {
        self.text.flush(self.renderer.queue());
        self.images.flush(self.renderer.queue());
        self.renderer.write_uniforms(self.scale);
        self.animating
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        scene: Option<&wgpu::TextureView>,
    ) {
        {
            let Self {
                renderer,
                text,
                images,
                layers,
                overlay,
                change,
                ..
            } = self;
            renderer.paint(
                encoder,
                PaintFrame {
                    target,
                    scene,
                    scene_changed: change.scene,
                    reset: change.reset,
                    atlases: crate::render::Atlases {
                        glyphs: text.atlas(),
                        images,
                    },
                    overlay,
                    content: &layers[Layer::Content.index()],
                    chrome: &layers[Layer::Chrome.index()],
                    popup: &layers[Layer::Popup.index()],
                    hud: &layers[Layer::Hud.index()],
                },
            );
        }
        for layer in &mut self.layers {
            layer.dirty = None;
        }
        self.change.scene = false;
        self.change.reset = false;
    }

    pub fn figure_instances(
        &mut self,
        size: Vec2,
        pixels_per_point: f32,
        content: impl FnOnce(&mut Painter),
    ) -> Vec<crate::paint::Instance> {
        self.text.begin(pixels_per_point);
        let clip = Clip::new(
            Rect::from_min_size(Vec2::ZERO, size),
            crate::geometry::Corners::ZERO,
        );
        let mut instances = Instances::default();
        {
            let mut painter =
                Painter::new(&mut instances, &mut self.text, &mut self.images, clip, None);
            content(&mut painter);
        }
        self.text.begin(self.scale);
        instances.material
    }

    pub fn figure_marks(
        &mut self,
        size: Vec2,
        pixels_per_point: f32,
        content: impl FnOnce(&mut Painter),
    ) -> Vec<crate::record::Mark> {
        self.text.begin(pixels_per_point);
        let clip = Clip::new(
            Rect::from_min_size(Vec2::ZERO, size),
            crate::geometry::Corners::ZERO,
        );
        let mut instances = Instances::default();
        let mut marks = Vec::new();
        {
            let mut painter = Painter::new(
                &mut instances,
                &mut self.text,
                &mut self.images,
                clip,
                Some(&mut marks),
            );
            content(&mut painter);
        }
        self.text.begin(self.scale);
        marks
    }

    pub fn build(&mut self) -> BuildContext<'_, A> {
        BuildContext {
            text: &mut self.text,
            theme: &self.theme,
            language: self.language,
            tree: &self.tree,
            animations: &mut self.animations,
            animating: &mut self.animating,
            size: self.size,
            scale: self.scale,
            elapsed: self.elapsed,
            stepped: std::collections::HashSet::new(),
            pointer: self.pointer,
        }
    }

    pub(crate) fn wake_animation(&mut self, id: NodeId) {
        if self.tree.nodes[id].runtime.active {
            return;
        }
        self.tree.nodes[id].runtime.active = true;
        self.animation.push(id);
    }

    fn invalidate(&mut self) {
        for node in self.tree.nodes.values_mut() {
            node.paint_dirty = true;
        }
        self.tree.mark_dirty();
        self.change.reset = true;
    }
}

pub struct BuildContext<'a, A> {
    text: &'a mut Text,
    theme: &'a Theme,
    language: Language,
    tree: &'a Tree<A>,
    animations: &'a mut HashMap<u64, Animated>,
    animating: &'a mut bool,
    size: Vec2,
    scale: f32,
    elapsed: f32,
    pointer: Vec2,
    stepped: std::collections::HashSet<u64>,
}

impl<A> BuildContext<'_, A> {
    pub fn theme(&self) -> &Theme {
        self.theme
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn size(&self) -> Vec2 {
        self.size
    }

    pub fn scale(&self) -> f32 {
        self.scale
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

    pub fn window(&self, key: Key, row: f32, rows: usize) -> crate::scroll::Window {
        let row = row.max(1.0);
        let screen = self.size.y.max(self.size.x);
        let (offset, viewport, axis) =
            self.tree
                .node_at(key)
                .map_or((0.0, screen, Axis::Column), |id| {
                    let node = &self.tree.nodes[id];
                    let axis = node.scroll_axis();
                    let viewport = match axis {
                        Axis::Row => node.rect.width(),
                        Axis::Column => node.rect.height(),
                    };
                    let viewport = if viewport > 0.0 { viewport } else { screen };
                    (
                        node.scroll.map_or(0.0, |scroll| scroll.offset),
                        viewport,
                        axis,
                    )
                });
        let visible = (viewport / row).ceil() as usize + 2;
        let first = ((offset / row).floor().max(0.0) as usize).min(rows);
        let last = (first + visible).min(rows);
        crate::scroll::Window {
            offset,
            viewport,
            row,
            rows,
            first,
            last,
            axis,
        }
    }

    pub fn scroll<'a>(
        &self,
        key: Key,
        axis: Axis,
        length: Length,
        row: f32,
        rows: usize,
        build: impl FnOnce(crate::scroll::Window) -> Vec<Element<'a, A>>,
    ) -> Element<'a, A>
    where
        A: 'a,
    {
        let window = self.window(key, row, rows);
        let mut children = build(window);
        let lead = window.first as f32 * row;
        let tail = rows.saturating_sub(window.last) as f32 * row;
        if lead > 0.0 {
            children.insert(0, crate::element::extent(axis, lead));
        }
        if tail > 0.0 {
            children.push(crate::element::extent(axis, tail));
        }
        Element::new(Layout::column())
            .key(key)
            .scroll(axis, length, Some(rows as f32 * row))
            .clipped()
            .children(children)
    }

    pub fn popup_open(&self, key: Key) -> bool {
        self.popup_anchors_open(key)
    }

    fn popup_anchors_open(&self, key: Key) -> bool {
        self.tree
            .nodes
            .values()
            .any(|node| node.popup_spec.is_some_and(|spec| spec.anchor == key) && node.runtime.open)
    }

    pub fn is_editing(&self, key: Key) -> bool {
        self.tree
            .node_at(key)
            .is_some_and(|id| self.tree.nodes[id].runtime.edit.is_some())
    }

    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }
}
