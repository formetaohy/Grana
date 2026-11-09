use std::collections::HashMap;

use glam::Vec2;
use grana_glass::{Animated, ContainerId, GlassId, Scene as GlassScene};

use crate::clipboard::{Clipboard, NullClipboard};
use crate::element::{CanvasPaint, Element, Key, Layer};
use crate::geometry::{Corners, Rect};
use crate::image::ImageAtlas;
use crate::input::{Cursor, Input};
use crate::layout::LayoutEngine;
use crate::motion::Motion;
use crate::node::NodeId;
use crate::paint::{Clip, Instance, LayerInstances, Painter};
use crate::render::{Ink, PaintFrame, Renderer};
use crate::text::{Text, TextLayout, TextStyle};
use crate::theme::Theme;
use crate::tree::Tree;

pub(crate) const CLICK_SLOP: f32 = 3.0;
pub(crate) const TOOLTIP_DELAY: f32 = 0.55;

#[derive(Clone, Copy)]
pub(crate) struct PopupTransition {
    pub glass: GlassId,
    pub anchor: GlassId,
    pub body: Rect,
}

#[derive(Default)]
pub(crate) struct LayerChange {
    pub hashes: [Option<u64>; Layer::ALL.len()],
    pub orders: [u64; Layer::ALL.len()],
    pub next_orders: [u64; Layer::ALL.len()],
    pub touched: [bool; Layer::ALL.len()],
    pub base: bool,
    pub interface: bool,
    pub reset: bool,
    pub restructured: bool,
}

pub struct Ui<A = ()> {
    pub(crate) renderer: Renderer,
    pub(crate) text: Text,
    pub(crate) images: ImageAtlas,
    pub(crate) theme: Theme,
    pub(crate) input: Input,
    pub(crate) tree: Tree<A>,
    pub(crate) animations: HashMap<u64, Animated>,
    pub(crate) scene: GlassScene,
    pub(crate) glass_nodes: HashMap<NodeId, GlassId>,
    pub(crate) glass_groups: HashMap<Key, ContainerId>,
    pub(crate) tooltip_glass: Option<GlassId>,
    pub(crate) sub_glass: HashMap<NodeId, Vec<GlassId>>,
    pub(crate) popup_glass: HashMap<NodeId, PopupTransition>,
    pub(crate) popup_windows: HashMap<NodeId, (Rect, Corners)>,
    pub(crate) popup_retire: Vec<(GlassId, GlassId)>,
    pub(crate) floating: Option<ContainerId>,
    pub(crate) layers: [LayerInstances; Layer::ALL.len()],
    pub(crate) change: LayerChange,
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
    pub(crate) theme_stamp: Appearance,
    pub(crate) scene_revision: Option<u64>,
}

impl<A: Clone> Ui<A> {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            renderer: Renderer::new(device, queue, format),
            text: Text::new(device),
            images: ImageAtlas::new(device),
            theme: Theme::dark(),
            input: Input::new(),
            tree: Tree::new(),
            animations: HashMap::new(),
            scene: GlassScene::new(),
            glass_nodes: HashMap::new(),
            glass_groups: HashMap::new(),
            tooltip_glass: None,
            sub_glass: HashMap::new(),
            popup_glass: HashMap::new(),
            popup_windows: HashMap::new(),
            popup_retire: Vec::new(),
            floating: None,
            layers: std::array::from_fn(|_| LayerInstances::default()),
            change: LayerChange::default(),
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
            theme_stamp: appearance(Theme::dark()),
            scene_revision: None,
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

    pub fn scene_view(&self) -> &wgpu::TextureView {
        self.renderer.scene_view()
    }

    pub fn set_scene_revision(&mut self, revision: u64) {
        if self.scene_revision == Some(revision) {
            return;
        }
        self.scene_revision = Some(revision);
        self.change.base = true;
    }

    pub fn pointer_over_widget(&self) -> bool {
        crate::hit::resolve(&self.hits, self.pointer).is_some()
    }

    pub fn pointer_over_chrome(&self) -> bool {
        let pointer = self.pointer;
        self.occluders.iter().any(|rect| rect.contains(pointer))
    }

    pub fn pointer_captured(&self) -> bool {
        self.press.is_some()
    }

    pub fn popup_open_any(&self) -> bool {
        self.popup_anchors
            .values()
            .any(|id| self.tree.nodes[*id].runtime.open)
    }

    pub fn update(&mut self, size: Vec2, scale: f32, time: f64) -> Vec<A> {
        let rescaled = self.scale != scale;
        let resized = self.size != size;
        self.size = size;
        self.scale = scale;
        self.time = time as f32;
        self.elapsed = self.input.begin(size, scale, time);
        self.text.begin(scale);
        self.actions.clear();
        self.occluders.clear();
        self.cursor = Cursor::Arrow;
        self.animating = false;
        self.edit_caret = None;
        self.inside = self.input.inside();
        self.pointer = self.input.position();
        self.scene.appearance.dark = self.theme.dark;
        self.scene.appearance.transparency = 0.5;
        if resized || rescaled {
            self.tree.mark_dirty();
            self.change.reset = true;
        }
        if rescaled {
            self.invalidate();
        }
        if self.theme_appearance() != self.theme_stamp {
            self.theme_stamp = self.theme_appearance();
            self.invalidate();
        }
        self.advance_scroll();
        self.collect_focus();
        self.dispatch();
        self.animate_nodes();
        std::mem::take(&mut self.actions)
    }

    pub fn present<'a>(&mut self, element: Element<'a, A>) {
        let mut canvases: HashMap<NodeId, CanvasPaint<'a>> = HashMap::new();
        let removed = self.tree.apply(element, &mut canvases);
        self.change.restructured = !removed.is_empty() || self.tree.layout_dirty;
        self.prune(&removed);
        self.sync_fields();
        self.rebuild_popup_anchors();
        self.collect_popup_windows();
        if self.tree.layout_dirty {
            self.run_layout();
        }
        if self.ensure_popups() {
            self.collect_popup_windows();
            self.run_layout();
            self.change.restructured = true;
        }
        self.paint_layers(&canvases);
        self.sync_glass();
        self.update_tooltip();
        self.paint_overlays();
        self.change.touched[Layer::Hud.index()] = true;
    }

    fn run_layout(&mut self) {
        let mut engine = LayoutEngine {
            text: &mut self.text,
            theme: &self.theme,
            windows: &self.popup_windows,
            screen: self.size,
        };
        engine.run(&mut self.tree);
        self.update_scroll_metrics();
        self.rebuild_hits();
    }

    pub fn finish(&mut self) -> bool {
        let animating_glass = self.scene.update(self.elapsed);
        let glass_changed = self.scene.take_dirty();
        self.retire_popup_anchors();
        self.text.flush(self.renderer.queue());
        self.images.flush(self.renderer.queue());
        self.renderer.write_uniforms(self.scale);
        self.measure_layers(glass_changed);
        self.animating || animating_glass || !self.popup_retire.is_empty()
    }

    pub fn render(&mut self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let base_changed = self.change.base || self.change.reset;
        let interface_changed = self.change.interface || base_changed;
        {
            let Self {
                renderer,
                text,
                images,
                layers,
                scene,
                theme,
                ..
            } = self;
            renderer.paint(
                encoder,
                PaintFrame {
                    target,
                    atlases: crate::render::Atlases {
                        glyphs: text.atlas(),
                        images,
                    },
                    content: &layers[Layer::Content.index()].material,
                    chrome: &layers[Layer::Chrome.index()],
                    labels: &layers[Layer::Labels.index()],
                    popup: &layers[Layer::Popup.index()],
                    hud: &layers[Layer::Hud.index()],
                    glass: scene,
                    base_changed,
                    interface_changed,
                    ink: if theme.dark { Ink::Light } else { Ink::Dark },
                },
            );
        }
        self.change.base = false;
        self.change.interface = false;
        self.change.reset = false;
    }

    pub fn figure_instances(
        &mut self,
        size: Vec2,
        pixels_per_point: f32,
        content: impl FnOnce(&mut Painter),
    ) -> Vec<Instance> {
        self.text.begin(pixels_per_point);
        let clip = Clip::new(
            Rect::from_min_size(Vec2::ZERO, size),
            crate::geometry::Corners::ZERO,
        );
        let dark = self.theme.dark;
        let mut instances = LayerInstances::default();
        {
            let mut painter = Painter::new(
                &mut instances,
                &mut self.text,
                &mut self.images,
                clip,
                dark,
                false,
            );
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
            elapsed: self.elapsed,
            stepped: std::collections::HashSet::new(),
            pointer: self.pointer,
        }
    }

    fn invalidate(&mut self) {
        for index in 0..self.tree.ids.len() {
            self.tree.nodes[self.tree.ids[index]].paint_dirty = true;
        }
        self.tree.mark_dirty();
        self.change.reset = true;
    }

    fn theme_appearance(&self) -> Appearance {
        Appearance {
            dark: self.theme.dark,
            palette: self.theme.palette,
            typography: self.theme.typography,
            metrics: self.theme.metrics,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Appearance {
    pub dark: bool,
    pub palette: crate::theme::Palette,
    pub typography: crate::theme::Typography,
    pub metrics: crate::theme::Metrics,
}

pub struct BuildContext<'a, A> {
    text: &'a mut Text,
    theme: &'a Theme,
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

    pub fn is_editing(&self, key: Key) -> bool {
        self.tree
            .node_at(key)
            .is_some_and(|id| self.tree.nodes[id].runtime.edit.is_some())
    }

    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }
}

pub(crate) fn glass_rect(rect: Rect) -> grana_glass::Rect {
    grana_glass::Rect::new(rect.min.x, rect.min.y, rect.width(), rect.height())
}

fn appearance(theme: Theme) -> Appearance {
    Appearance {
        dark: theme.dark,
        palette: theme.palette,
        typography: theme.typography,
        metrics: theme.metrics,
    }
}
