use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use grana_animation::Keyframe;
use grana_display::{Viewport, plane_basis};
use grana_editor::{
    AnnotationTool, FrameStats, GrabDrag, Pane, Stage, ToolDrag, ViewStats, Workspace,
};
use grana_engine::{Destination, Engine, Outcome, Panels, Picked, Plan, Pose, SAMPLE_LIMIT};
use grana_gpu::Gpu;
use grana_project::ViewState;
use grana_render::{FrameInfo, PickPane};
use grana_scene::ObjectSetId;
use grana_session::{Command, Effects, Export, Session, SessionView};
use grana_ui::{
    Button as UiButton, Event as UiEvent, Modifiers as UiModifiers, Ui, Vec2 as UiVec2,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::clipboard::SystemClipboard;

const STREAM_POLL: Duration = Duration::from_millis(16);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);
const TOOL_CLICK_SLOP: f32 = 3.0;

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    ui: Ui<grana_editor::Message>,
}

pub struct App {
    surface: Option<Surface>,
    session: Session,
    engine: Option<Engine>,
    start: Instant,
    capture: Option<PathBuf>,
    elapsed_frames: u32,
    exiting: bool,
    viewport: Viewport,
    stage: Stage,
    workspace: Workspace,
    info: FrameInfo,
    generation: u64,
    reload: u64,
    needs_fit: bool,
    fit_section: bool,
    last_advance: Option<Instant>,
    last_animation: Option<Instant>,
    repaint_at: Option<Instant>,
    title: Option<String>,
    modifiers: ModifiersState,
    message: Option<String>,
    resume: Option<(u32, f32)>,
    ime: bool,
    ime_area: Option<(i32, i32, u32, u32)>,
    scene_changed: bool,
    pick_toggle: bool,
}

impl App {
    pub fn new(capture: Option<PathBuf>) -> Self {
        let mut session = Session::new();
        session.set_snapshot_factory(Arc::new(|| Arc::new(crate::snapshot::Snapshot::new())));
        Self {
            surface: None,
            session,
            engine: None,
            start: Instant::now(),
            capture,
            elapsed_frames: 0,
            exiting: false,
            viewport: Viewport::new(),
            stage: Stage::new(),
            workspace: Workspace::default(),
            info: FrameInfo::default(),
            generation: 0,
            reload: 0,
            needs_fit: true,
            fit_section: false,
            last_advance: None,
            last_animation: None,
            repaint_at: None,
            title: None,
            modifiers: ModifiersState::default(),
            message: None,
            resume: None,
            ime: false,
            ime_area: None,
            scene_changed: true,
            pick_toggle: false,
        }
    }

    fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Grana")
                        .with_visible(self.capture.is_none())
                        .with_inner_size(LogicalSize::new(1440.0, 900.0))
                        .with_min_inner_size(LogicalSize::new(1080.0, 700.0)),
                )
                .expect("failed to create the window"),
        );
        let size = window.inner_size();
        window.set_ime_allowed(false);
        window.set_theme(Some(winit::window::Theme::Dark));
        let gpu = Gpu::new(window.clone(), [size.width, size.height]);
        let mut ui: Ui<grana_editor::Message> = Ui::new(&gpu.device, &gpu.queue, gpu.format());
        ui.set_clipboard(Box::new(SystemClipboard::new()));
        let mut engine = Engine::new(&gpu.device, &gpu.queue, gpu.format());
        self.session.open_sample();
        if let Some(scene) = self.session.scene() {
            engine.set_dataset(&gpu.device, scene.dataset.pyramid());
        }
        self.generation = self.session.generation();
        self.surface = Some(Surface { window, gpu, ui });
        self.engine = Some(engine);
    }

    fn redraw(&mut self) {
        if self.session.poll() {
            self.scene_changed = true;
        }
        if let Some(set) = self.workspace.seed
            && self
                .session
                .scene()
                .is_none_or(|scene| scene.objects.get(set).is_none())
        {
            self.workspace.seed = None;
        }
        self.exchange_bricks();
        self.poll_pick();
        self.session.sync_measure();
        let (commands, animating) = self.run_ui();
        self.sync_ime();
        self.interact_viewport();
        if self.apply(commands) {
            self.scene_changed = true;
            return;
        }
        self.session.apply_measure();
        self.sync_title();
        self.advance_playback();
        self.advance_animation();
        self.fit_views();
        let exporting = self.engine.as_ref().is_some_and(Engine::exporting);
        let mut delay = if exporting {
            self.export_tick().unwrap_or(Duration::MAX)
        } else {
            self.render().unwrap_or(Duration::MAX)
        };
        if exporting || animating {
            delay = Duration::ZERO;
        } else if self.workspace.playback.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.info.samples < SAMPLE_LIMIT {
            delay = Duration::ZERO;
        }
        if self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if self.workspace.playback.playing {
            delay = delay.min(PLAYBACK_INTERVAL);
        }
        self.schedule(delay);
    }

    fn exchange_bricks(&mut self) {
        let App {
            surface,
            engine,
            session,
            info,
            scene_changed,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_mut()) else {
            return;
        };
        if let Some(requests) = engine.poll_requests(&surface.gpu.device) {
            let current = session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time);
            if current {
                info.volume = requests.volume_view;
                info.section = requests.section_view;
                let mut bricks = Vec::with_capacity(
                    requests.volume.len() + requests.section.len() + requests.prefetch.len(),
                );
                bricks.extend_from_slice(&requests.section);
                bricks.extend_from_slice(&requests.volume);
                bricks.extend_from_slice(&requests.prefetch);
                session.request(requests.time, &bricks);
            }
        }
        for (request, data) in session.take_bricks() {
            engine.upload(&surface.gpu.queue, request, &data);
            *scene_changed = true;
        }
        if session.generation() != self.generation {
            self.generation = session.generation();
            *scene_changed = true;
            if let Some(scene) = session.scene() {
                engine.set_dataset(&surface.gpu.device, scene.dataset.pyramid());
            }
            self.needs_fit = true;
            self.fit_section = false;
            self.workspace.playback.playing = false;
            self.workspace.playback.previewing = false;
            self.last_advance = None;
            self.last_animation = None;
            *info = FrameInfo::default();
        }
        let view = session.take_view();
        let reload = session.reload();
        if reload != self.reload {
            self.reload = reload;
            *scene_changed = true;
            if let Some(scene) = session.scene() {
                engine.set_dataset(&surface.gpu.device, scene.dataset.pyramid());
            }
            *info = FrameInfo::default();
        }
        if let Some(view) = view {
            *scene_changed = true;
            self.apply_view(&view);
        }
    }

    fn fit_views(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let panes =
            Panels::new(self.stage.volume_rect(), self.stage.section_rect()).screen(scale, size);
        self.stage.set_regions(panes.volume, panes.section);
        let volume_aspect = panes.volume[2] as f32 / panes.volume[3] as f32;
        let section_aspect = panes.section[2] as f32 / panes.section[3] as f32;
        let sized = panes.volume[2] > 64
            && panes.volume[3] > 64
            && panes.section[2] > 64
            && panes.section[3] > 64;
        let Some(scene) = self.session.scene() else {
            return;
        };
        let world = scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size();
        if self.fit_section && sized {
            self.viewport.fit_section(world, voxel, section_aspect);
            self.fit_section = false;
            self.scene_changed = true;
        }
        if self.needs_fit && sized {
            self.viewport
                .fit(world, voxel, volume_aspect, section_aspect);
            self.needs_fit = false;
            self.scene_changed = true;
        }
    }

    fn schedule(&mut self, delay: Duration) {
        self.elapsed_frames += 1;
        let streamed = self.info.volume.resident >= self.info.volume.demand
            && self.info.section.resident >= self.info.section.demand;
        if let Some(path) = self.capture.clone()
            && ((self.elapsed_frames > 30 && streamed && self.info.samples >= SAMPLE_LIMIT)
                || self.elapsed_frames > 150)
        {
            self.capture = None;
            self.write_capture(&path);
            self.exiting = true;
        }
        if delay.is_zero() {
            self.repaint_at = None;
            self.request_redraw();
        } else if delay < MAX_REPAINT_DELAY {
            self.repaint_at = Some(Instant::now() + delay);
        } else {
            self.repaint_at = None;
        }
    }
    fn write_capture(&mut self, path: &Path) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let (pixels, size, format) = surface.ui.renderer_mut().read_color();
        let pixels = if format == wgpu::TextureFormat::Bgra8Unorm {
            let mut swapped = pixels;
            for pixel in swapped.chunks_mut(4) {
                pixel.swap(0, 2);
            }
            swapped
        } else {
            pixels
        };
        let raster = grana_raster::Raster::new(size[0], size[1], pixels);
        match grana_raster::write(path, &raster) {
            Ok(()) => eprintln!("grana: saved {}", path.display()),
            Err(detail) => eprintln!("{detail}"),
        }
    }

    fn interact_viewport(&mut self) {
        let (interaction, hovered) = {
            let App {
                surface,
                session,
                viewport,
                stage,
                workspace,
                ..
            } = self;
            let Some(surface) = surface.as_mut() else {
                return;
            };
            let spacing = session
                .scene()
                .map_or(grana_volume::Spacing::new(1.0, 1.0, 1.0), |scene| {
                    scene.dataset.spacing()
                });
            let tool = workspace
                .annotation_tool
                .filter(|_| workspace.seed.is_none());
            let blocked = surface.ui.pointer_over_chrome()
                || surface.ui.pointer_over_widget()
                || surface.ui.pointer_captured();
            let hovered = (!blocked && tool.is_none() && workspace.seed.is_none())
                .then(|| annotation_hit(surface, session, viewport, stage))
                .flatten();
            let pressed = surface.ui.input().button(UiButton::Primary).pressed;
            let grabbed = hovered.is_some() && pressed;
            let interaction = stage.input(&mut surface.ui, viewport, spacing, tool, grabbed);
            if interaction.camera {
                if let Some(scene) = session.scene() {
                    viewport.clamp_focus(0.5 * scene.dataset.world_size());
                }
                surface.window.request_redraw();
            }
            let cursor = if workspace.seed.is_some() || tool.is_some() {
                CursorIcon::Crosshair
            } else if interaction.grab.is_some() {
                CursorIcon::Grabbing
            } else if hovered.is_some() {
                CursorIcon::Move
            } else {
                interface_cursor(surface.ui.cursor())
            };
            surface.window.set_cursor(cursor);
            (interaction, hovered)
        };
        if let Some((index, part)) = hovered
            && interaction.grab.is_some()
            && self.workspace.annotation_drag.is_none()
        {
            self.workspace.annotation_drag = Some((index, part));
            self.apply(vec![Command::SelectAnnotation(Some(index))]);
        }
        if let Some(tool) = interaction.tool {
            self.use_tool(tool);
        }
        if let Some(grab) = interaction.grab {
            self.drag_annotation(grab);
        }
        let picking = interaction.pick.is_some();
        if let Some((pane, point)) = interaction.pick {
            match self.workspace.seed.filter(|_| pane == Pane::Volume) {
                Some(set) => self.place_seed(set, point, interaction.modifiers.alt),
                None => {
                    self.pick_toggle = interaction.modifiers.shift || interaction.modifiers.control;
                    self.pick(pane, point);
                }
            }
            self.request_redraw();
        }
        if interaction.camera || interaction.tool.is_some() || interaction.grab.is_some() || picking
        {
            self.scene_changed = true;
        }
    }

    fn use_tool(&mut self, drag: ToolDrag) {
        let Some(tool) = self.workspace.annotation_tool else {
            return;
        };
        match tool {
            AnnotationTool::Roi => self.draw_region(drag),
            AnnotationTool::Point | AnnotationTool::Distance => {
                if !drag.finished || (drag.position - drag.start).length() > TOOL_CLICK_SLOP {
                    return;
                }
                self.place_annotation(tool, drag.pane, drag.position);
            }
        }
        self.scene_changed = true;
    }

    fn draw_region(&mut self, drag: ToolDrag) {
        match drag.pane {
            Pane::Section => {
                if (drag.position - drag.start).length() <= TOOL_CLICK_SLOP {
                    if drag.finished {
                        self.default_region(Pane::Section, drag.start);
                    }
                    return;
                }
                let Some(from) = self.annotation_point(Pane::Section, drag.start) else {
                    return;
                };
                let Some(to) = self.annotation_point(Pane::Section, drag.position) else {
                    return;
                };
                let Some(scene) = self.session.scene() else {
                    return;
                };
                let half = 0.5 * scene.dataset.world_size();
                let voxel = scene.dataset.voxel_size();
                let axis = self.viewport.section().axis();
                let (u, v) = axis.in_plane();
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for point in [from, to] {
                    for index in 0..3 {
                        min[index] = min[index].min(point[index]);
                        max[index] = max[index].max(point[index]);
                    }
                }
                min[axis.index()] = -half[axis.index()];
                max[axis.index()] = half[axis.index()];
                for index in [u.index(), v.index()] {
                    let low = min[index].max(-half[index]);
                    let high = max[index].min(half[index]);
                    if high - low < voxel[index] {
                        let centre = 0.5 * (low + high);
                        min[index] = (centre - 0.5 * voxel[index]).max(-half[index]);
                        max[index] = (centre + 0.5 * voxel[index]).min(half[index]);
                    } else {
                        min[index] = low;
                        max[index] = high;
                    }
                }
                if min.cmplt(max).all() {
                    self.apply(vec![Command::SetRegion(grana_volume::Region::new(
                        min, max,
                    ))]);
                }
            }
            Pane::Volume => {
                if !drag.finished {
                    return;
                }
                self.default_region(Pane::Volume, drag.start);
            }
        }
    }

    fn default_region(&mut self, pane: Pane, point: Vec2) {
        let Some(centre) = self.annotation_point(pane, point) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let half = 0.5 * scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size();
        let reach = (0.06 * half.min_element()).max(voxel.min_element());
        let min = (Vec3::from(centre) - Vec3::splat(reach)).max(-half);
        let max = (Vec3::from(centre) + Vec3::splat(reach)).min(half);
        if min.cmplt(max).all() {
            self.apply(vec![Command::SetRegion(grana_volume::Region::new(
                min, max,
            ))]);
        }
    }

    fn drag_annotation(&mut self, drag: GrabDrag) {
        let Some((index, part)) = self.workspace.annotation_drag else {
            return;
        };
        if drag.finished {
            self.workspace.annotation_drag = None;
        }
        if drag.delta.length_squared() <= 0.0 {
            return;
        }
        let Some(offset) = self.annotation_offset(drag.pane, index, drag.delta) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let Some(annotation) = scene.annotations.get(index) else {
            return;
        };
        let kind = annotation.kind();
        let next = match part {
            grana_figure::AnnotationPart::Anchor => {
                let at = (Vec3::from(kind.anchor()) + offset).to_array();
                (at.iter().all(|value| value.is_finite()) && kind.end() != Some(at))
                    .then(|| kind.with_anchor(at))
            }
            grana_figure::AnnotationPart::End => kind.end().and_then(|end| {
                let at = (Vec3::from(end) + offset).to_array();
                (at.iter().all(|value| value.is_finite()) && kind.anchor() != at)
                    .then(|| kind.with_end(at))
            }),
            grana_figure::AnnotationPart::Body => offset
                .is_finite()
                .then(|| kind.translated(offset.to_array())),
        };
        if let Some(kind) = next {
            self.apply(vec![Command::SetAnnotationKind { index, kind }]);
        }
    }

    fn annotation_offset(&self, pane: Pane, index: usize, delta: Vec2) -> Option<Vec3> {
        let scene = self.session.scene()?;
        let anchor = scene.annotations.get(index)?.kind().anchor();
        match pane {
            Pane::Volume => {
                let rect = self.stage.volume_rect();
                if rect.height() <= 0.0 {
                    return None;
                }
                let camera = self.viewport.camera();
                let depth = (Vec3::from(anchor) - camera.eye()).dot(camera.forward());
                let per_point =
                    2.0 * depth.abs().max(1.0e-6) * (camera.fov_y() * 0.5).tan() / rect.height();
                Some(camera.right() * delta.x * per_point - camera.up() * delta.y * per_point)
            }
            Pane::Section => {
                let rect = self.stage.section_rect();
                if rect.height() <= 0.0 {
                    return None;
                }
                let axis = self.viewport.section().axis();
                let (right, up) = plane_basis(axis);
                let per_point = self.viewport.section().span() / rect.height();
                Some(right * delta.x * per_point - up * delta.y * per_point)
            }
        }
    }

    fn place_seed(&mut self, set: ObjectSetId, point: Vec2, remove: bool) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        if self
            .session
            .scene()
            .is_none_or(|scene| scene.seed_source(set).is_some())
        {
            return;
        }
        let region = self.stage.volume_region();
        if region[2] == 0 || region[3] == 0 {
            return;
        }
        let scale = surface.window.scale_factor() as f32;
        let x = point.x * scale;
        let y = point.y * scale;
        let ndc = Vec2::new(
            2.0 * (x - region[0] as f32) / region[2] as f32 - 1.0,
            1.0 - 2.0 * (y - region[1] as f32) / region[3] as f32,
        );
        let aspect = region[2] as f32 / region[3] as f32;
        let ray = self.viewport.camera().ray(ndc, aspect);
        let Some(scene) = self.session.scene() else {
            return;
        };
        let threshold = 3.0 * scene.dataset.voxel_size().min_element();
        if remove {
            let mut nearest: Option<(f32, usize)> = None;
            for (index, seed) in scene.seeds(set).iter().enumerate() {
                let offset = Vec3::from(*seed) - ray.origin;
                let along = offset.dot(ray.direction);
                if along < 0.0 {
                    continue;
                }
                let distance = (offset - ray.direction * along).length();
                if distance <= threshold && nearest.is_none_or(|(best, _)| distance < best) {
                    nearest = Some((distance, index));
                }
            }
            if let Some((_, index)) = nearest {
                self.apply(vec![Command::RemoveSeed { set, index }]);
            }
            return;
        }
        let hit = match scene.surface_ray(set, ray.origin, ray.direction) {
            Ok(Some(hit)) => hit,
            Ok(None) => return,
            Err(detail) => {
                self.message = Some(detail);
                return;
            }
        };
        self.apply(vec![Command::AddSeed {
            set,
            position: hit.to_array(),
        }]);
    }

    fn volume_ray(&self, point: Vec2) -> Option<(Vec3, Vec3)> {
        let surface = self.surface.as_ref()?;
        let region = self.stage.volume_region();
        if region[2] == 0 || region[3] == 0 {
            return None;
        }
        let scale = surface.window.scale_factor() as f32;
        let x = point.x * scale;
        let y = point.y * scale;
        let ndc = Vec2::new(
            2.0 * (x - region[0] as f32) / region[2] as f32 - 1.0,
            1.0 - 2.0 * (y - region[1] as f32) / region[3] as f32,
        );
        let aspect = region[2] as f32 / region[3] as f32;
        let ray = self.viewport.camera().ray(ndc, aspect);
        Some((ray.origin, ray.direction))
    }

    fn annotation_point(&self, pane: Pane, point: Vec2) -> Option<[f32; 3]> {
        let scene = self.session.scene()?;
        match pane {
            Pane::Volume => {
                let (origin, direction) = self.volume_ray(point)?;
                if let Ok(Some(hit)) = scene.surface_hit(origin, direction) {
                    return Some(hit.to_array());
                }
                let axis = self.viewport.section().axis();
                let component = direction[axis.index()];
                if component.abs() <= 1.0e-6 {
                    return None;
                }
                let plane = self.viewport.camera().focus()[axis.index()];
                let distance = (plane - origin[axis.index()]) / component;
                if distance <= 0.0 {
                    return None;
                }
                let position = origin + direction * distance;
                let half = 0.5 * scene.dataset.world_size();
                Some([0, 1, 2].map(|axis| position[axis].clamp(-half[axis], half[axis])))
            }
            Pane::Section => {
                let rect = self.stage.section_rect();
                if rect.height() <= 0.0 {
                    return None;
                }
                let axis = self.viewport.section().axis();
                let (u, v) = axis.in_plane();
                let focus = self.viewport.camera().focus();
                let points_per_micron = rect.height() / self.viewport.section().span();
                let mut position = [0.0f32; 3];
                position[u.index()] =
                    focus[u.index()] + (point.x - rect.center().x) / points_per_micron;
                position[v.index()] =
                    focus[v.index()] - (point.y - rect.center().y) / points_per_micron;
                position[axis.index()] = focus[axis.index()];
                let half = 0.5 * scene.dataset.world_size();
                for axis in 0..3 {
                    position[axis] = position[axis].clamp(-half[axis], half[axis]);
                }
                Some(position)
            }
        }
    }

    fn place_annotation(&mut self, tool: AnnotationTool, pane: Pane, point: Vec2) {
        let Some(position) = self.annotation_point(pane, point) else {
            return;
        };
        match tool {
            AnnotationTool::Point => {
                self.apply(vec![Command::AddPoint { at: position }]);
            }
            AnnotationTool::Distance => match self.workspace.annotation_pending.take() {
                Some(from) if from != position => {
                    self.apply(vec![Command::AddDistance { from, to: position }]);
                }
                Some(from) => self.workspace.annotation_pending = Some(from),
                None => self.workspace.annotation_pending = Some(position),
            },
            AnnotationTool::Roi => {}
        }
    }

    fn apply(&mut self, commands: Vec<Command>) -> bool {
        let view = self.view_state();
        self.session.observe_view(view);
        for command in commands {
            let effects = self.session.execute(command);
            if self.react(effects) {
                return true;
            }
        }
        false
    }

    fn react(&mut self, effects: Effects) -> bool {
        if effects.repaint {
            self.request_redraw();
        }
        if effects.open_dataset
            && let Some(path) = Self::pick_folder()
        {
            self.session.begin_load(path);
            return true;
        }
        if effects.open_image
            && let Some(path) = Self::pick_image()
        {
            self.session.begin_load(path);
            return true;
        }
        if effects.open_project
            && let Some(path) = Self::pick_project()
        {
            self.session.begin_project(path);
            return true;
        }
        if let Some(choose) = effects.save_project {
            let view = self.view_state();
            if let Some(path) = self.project_path(choose) {
                self.session.save_project(path, view);
            }
        }
        if let Some(export) = effects.snapshot {
            self.begin_snapshot(export);
        }
        if let Some(export) = effects.movie {
            self.begin_movie(export);
        }
        if let Some(export) = effects.sequence {
            self.begin_sequence(export);
        }
        if effects.cancel_export {
            self.cancel_export();
        }
        if effects.keyframe {
            self.set_keyframe();
        }
        false
    }

    fn run_ui(&mut self) -> (Vec<Command>, bool) {
        let App {
            surface,
            engine,
            session,
            viewport,
            stage,
            workspace,
            info,
            ..
        } = self;
        let Some(surface) = surface.as_mut() else {
            return (Vec::new(), false);
        };
        let Some(engine) = engine.as_mut() else {
            return (Vec::new(), false);
        };
        let Some(scene) = session.scene() else {
            return (Vec::new(), false);
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let points = UiVec2::new(size[0] as f32 / scale, size[1] as f32 / scale);
        surface.ui.resize(size);
        let actions = surface
            .ui
            .update(points, scale, self.start.elapsed().as_secs_f64());
        let adapter = surface.gpu.adapter.clone();
        let export = engine.progress();
        let export_message = engine.output_message();
        let view = SessionView {
            scene,
            measure: session.measure(),
            timeline: session.timeline(),
            jobs: session.progress(),
            io: session.io(&adapter, export, export_message),
            batch: session.batch(),
            history: session.history(),
            series: session.series(),
            labels: session.label_images(),
        };
        let frame = frame_stats(*info);
        let commands = grana_editor::build(
            &mut surface.ui,
            actions,
            &view,
            viewport,
            stage,
            workspace,
            frame,
        );
        let animating = surface.ui.finish();
        (commands, animating)
    }

    fn sync_ime(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let editing = surface.ui.editing();
        if editing != self.ime {
            surface.window.set_ime_allowed(editing);
            self.ime = editing;
            self.ime_area = None;
        }
        let Some(caret) = surface.ui.edit_caret() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let area = (
            (caret.min.x * scale).round() as i32,
            (caret.min.y * scale).round() as i32,
            (caret.width().max(1.0) * scale).round().max(1.0) as u32,
            (caret.height().max(1.0) * scale).round().max(1.0) as u32,
        );
        if self.ime_area == Some(area) {
            return;
        }
        surface.window.set_ime_cursor_area(
            PhysicalPosition::new(area.0, area.1),
            PhysicalSize::new(area.2, area.3),
        );
        self.ime_area = Some(area);
    }

    fn render(&mut self) -> Option<Duration> {
        if self.engine.as_ref().is_some_and(Engine::exporting) {
            let delay = self.export_tick();
            self.present_ui();
            return delay;
        }
        let size = self.surface.as_ref()?.gpu.size();
        let scale = self.surface.as_ref()?.window.scale_factor() as f32;
        let panes = self.panels().screen(scale, size);
        let playing = self.workspace.playback.playing;
        let App {
            surface,
            engine,
            session,
            viewport,
            scene_changed,
            ..
        } = self;
        let surface = surface.as_mut()?;
        let engine = engine.as_mut()?;
        let scene = session.scene()?;
        let frame = surface.gpu.acquire()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let info = match engine.draw_viewport(
            &surface.gpu.device,
            &surface.gpu.queue,
            grana_engine::ViewportFrame {
                target: surface.ui.scene_view(),
                scene,
                viewport,
                size,
                volume_region: panes.volume,
                section_region: panes.section,
                prefetch: playing,
            },
        ) {
            Ok(info) => info,
            Err(detail) => {
                engine.set_output_message(Some(detail));
                return Some(Duration::ZERO);
            }
        };
        let mut encoder =
            surface
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("grana.ui.paint"),
                });
        surface.ui.set_scene_changed(std::mem::take(scene_changed));
        surface.ui.render(&mut encoder, &view);
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
        self.info = info;
        Some(Duration::MAX)
    }

    fn panels(&self) -> Panels {
        Panels::new(self.stage.volume_rect(), self.stage.section_rect())
    }

    fn pose(&self) -> Pose {
        Pose {
            time: self.session.scene().map_or(0, |scene| scene.time()),
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
        }
    }

    fn export_tick(&mut self) -> Option<Duration> {
        let Some(pose) = self.engine.as_ref().and_then(Engine::pose) else {
            self.present_ui();
            return Some(Duration::ZERO);
        };
        self.session.set_time(pose.time);
        let outcome = {
            let App {
                surface,
                engine,
                session,
                ..
            } = self;
            let (Some(surface), Some(engine), Some(scene)) =
                (surface.as_mut(), engine.as_mut(), session.scene())
            else {
                return Some(Duration::ZERO);
            };
            engine.render_export(
                &surface.gpu.device,
                &surface.gpu.queue,
                &mut surface.ui,
                scene,
            )
        };
        self.present_ui();
        match outcome {
            Outcome::Running => {}
            Outcome::Complete { .. } => self.finish_export(),
            Outcome::Failed { detail } => {
                self.message = Some(detail);
                self.finish_export();
            }
        }
        Some(Duration::ZERO)
    }

    fn start_export(&mut self, plan: Plan) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let Some(time) = self.session.scene().map(|scene| scene.time()) else {
            return;
        };
        let position = self.workspace.playback.position;
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        match engine.start_export(&surface.gpu.device, plan) {
            Ok(()) => {
                self.resume = Some((time, position));
                self.workspace.playback.previewing = false;
                self.workspace.playback.playing = false;
                self.last_advance = None;
                self.last_animation = None;
                self.message = None;
                self.repaint_at = Some(Instant::now());
            }
            Err(detail) => self.message = Some(detail),
        }
    }

    fn finish_export(&mut self) {
        if let Some((time, position)) = self.resume.take() {
            self.restore_time(time, position);
        }
        self.scene_changed = true;
        self.repaint_at = Some(Instant::now());
    }

    fn begin_snapshot(&mut self, export: Export) {
        if self.engine.as_ref().is_some_and(Engine::exporting) || self.session.loading().is_some() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let slug = grana_session::slug(scene.dataset.name());
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let width = if export.width == 0 {
            self.panels().native_width(scale)
        } else {
            export.width
        };
        let suggested = format!("{slug}-image.png");
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save an image")
            .set_file_name(suggested)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        let panels = self.panels();
        let poses = vec![self.pose()];
        self.start_export(Plan {
            destination: Destination::Image { path },
            panels,
            width,
            transparent: export.transparent,
            poses,
            fps: 0,
        });
    }

    fn begin_movie(&mut self, export: Export) {
        if self.engine.as_ref().is_some_and(Engine::exporting) || self.session.loading().is_some() {
            return;
        }
        if self.session.timeline().is_empty() {
            self.set_output_message("set a keyframe before exporting");
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let slug = grana_session::slug(scene.dataset.name());
        let (suggested, title, filter, extensions) = if export.transparent {
            (
                format!("{slug}-movie.mov"),
                "Save a transparent movie",
                "MOV",
                &["mov"][..],
            )
        } else {
            (
                format!("{slug}-movie.mp4"),
                "Save a movie",
                "MP4",
                &["mp4"][..],
            )
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(suggested)
            .add_filter(filter, extensions)
            .save_file()
        else {
            return;
        };
        let poses = poses(self.session.timeline());
        let fps = self.session.timeline().fps();
        let panels = self.panels();
        self.start_export(Plan {
            destination: Destination::Movie { path },
            panels,
            width: export.width,
            transparent: export.transparent,
            poses,
            fps,
        });
    }

    fn begin_sequence(&mut self, export: Export) {
        if self.engine.as_ref().is_some_and(Engine::exporting) || self.session.loading().is_some() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let frames = scene.frame_count();
        if frames == 0 {
            self.set_output_message("the dataset has no time point to export");
            return;
        }
        let stem = grana_session::slug(scene.dataset.name());
        let camera = self.viewport.camera_state();
        let section = self.viewport.section_state();
        let Some(parent) = rfd::FileDialog::new()
            .set_title("Choose a folder for the frame sequence")
            .pick_folder()
        else {
            return;
        };
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let width = if export.width == 0 {
            self.panels().native_width(scale)
        } else {
            export.width
        };
        let directory = parent.join(format!("{stem}-sequence"));
        let poses = (0..frames)
            .map(|time| Pose {
                time,
                camera,
                section,
            })
            .collect();
        let panels = self.panels();
        self.start_export(Plan {
            destination: Destination::Sequence { directory, stem },
            panels,
            width,
            transparent: export.transparent,
            poses,
            fps: 0,
        });
    }

    fn cancel_export(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if engine.cancel_export() {
            self.finish_export();
        }
        self.repaint_at = Some(Instant::now());
    }

    fn set_output_message(&mut self, message: &str) {
        if let Some(engine) = self.engine.as_mut() {
            engine.set_output_message(Some(message.to_string()));
        }
    }

    fn restore_time(&mut self, time: u32, position: f32) {
        self.session.set_time(time);
        self.workspace.playback.position = position.clamp(0.0, self.session.timeline().duration());
    }

    fn present_ui(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(frame) = surface.gpu.acquire() else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            surface
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("grana.ui.present"),
                });
        let changed = std::mem::take(&mut self.scene_changed);
        surface.ui.set_scene_changed(changed);
        surface.ui.render(&mut encoder, &view);
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
    }

    fn streaming(&self) -> bool {
        let Some(engine) = self.engine.as_ref() else {
            return false;
        };
        self.session.batch().is_some_and(|batch| batch.running)
            || self.workspace.playback.playing
            || self.session.loading().is_some()
            || self.session.transferring()
            || self.session.writing()
            || !self.session.progress().is_empty()
            || self.session.pending() > 0
            || engine.requests_pending()
            || engine.pick_pending()
            || engine.exporting()
            || (self.session.scene().is_some()
                && (self.info.volume.resident < self.info.volume.demand
                    || self.info.section.resident < self.info.section.demand))
    }

    fn view_state(&self) -> ViewState {
        ViewState {
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
            split: self.viewport.split(),
            timeline: self.session.timeline().clone(),
            export: self.session.export(),
        }
    }

    fn apply_view(&mut self, view: &ViewState) {
        self.viewport.restore(view.camera, view.section);
        self.viewport.set_split(view.split);
        self.session.restore_timeline(view.timeline.clone());
        self.workspace.playback.selected = None;
        self.workspace.playback.position = 0.0;
        self.needs_fit = false;
    }

    fn dirty(&self) -> bool {
        self.session.dirty()
    }

    fn sync_title(&mut self) {
        let name = self
            .session
            .project_name()
            .unwrap_or_else(|| "untitled".to_string());
        let marker = if self.dirty() { " •" } else { "" };
        let title = format!("Grana — {name}{marker}");
        if self.title.as_deref() == Some(title.as_str()) {
            return;
        }
        self.title = Some(title.clone());
        if let Some(surface) = &self.surface {
            surface.window.set_title(&title);
        }
    }

    fn pick(&mut self, pane: Pane, point: Vec2) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let point = [
            (point.x * scale).max(0.0).round() as u32,
            (point.y * scale).max(0.0).round() as u32,
        ];
        let pane = match pane {
            Pane::Volume => PickPane::Volume,
            Pane::Section => PickPane::Section,
        };
        engine.request_pick(&surface.gpu.device, &surface.gpu.queue, pane, point);
    }

    fn poll_pick(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if !engine.pick_pending() {
            return;
        }
        let Some(picked) = engine.poll_pick(&surface.gpu.device) else {
            return;
        };
        let selection = match picked {
            Picked::Hit(selection) => Some(selection),
            Picked::Seed => return,
            Picked::Miss => None,
        };
        self.scene_changed = true;
        match (selection, std::mem::take(&mut self.pick_toggle)) {
            (Some(selection), true) => {
                self.apply(vec![Command::ToggleSelect(selection)]);
            }
            (selection, _) => {
                self.apply(vec![Command::Select(selection)]);
            }
        }
    }

    fn set_keyframe(&mut self) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let duration = self.session.timeline().duration();
        let at = self.workspace.playback.position.clamp(0.0, duration);
        let key = Keyframe {
            at,
            time: scene.time(),
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
        };
        let index = self.session.insert_keyframe(key);
        self.workspace.playback.position = at;
        self.workspace.playback.selected = Some(index);
        if let Some(engine) = self.engine.as_mut() {
            engine.set_output_message(None);
        }
        self.request_redraw();
    }

    fn advance_playback(&mut self) {
        if !self.workspace.playback.playing {
            return;
        }
        let Some(scene) = self.session.scene() else {
            self.workspace.playback.playing = false;
            return;
        };
        if scene.frame_count() <= 1 {
            self.workspace.playback.playing = false;
            return;
        }
        let now = Instant::now();
        let due = self
            .last_advance
            .is_none_or(|last| now.duration_since(last) >= PLAYBACK_INTERVAL);
        if !due {
            return;
        }
        let next = (scene.time() + 1) % scene.frame_count();
        self.session.set_time(next);
        self.scene_changed = true;
        self.workspace.playback.previewing = false;
        self.last_advance = Some(now);
    }

    fn advance_animation(&mut self) {
        if !self.workspace.playback.previewing {
            return;
        }
        let duration = self.session.timeline().duration();
        let now = Instant::now();
        if let Some(last) = self.last_animation.replace(now) {
            let elapsed = now.duration_since(last).as_secs_f32().min(0.25);
            self.workspace.playback.position =
                (self.workspace.playback.position + elapsed) % duration;
        }
        self.pose_animation();
        self.scene_changed = true;
    }

    fn pose_animation(&mut self) {
        let position = self.workspace.playback.position;
        if let Some(sample) = self.session.timeline().sample(position) {
            self.viewport.restore(sample.camera, sample.section);
            self.session.set_time(sample.time);
        }
    }

    fn toggle_play(&mut self) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        if scene.frame_count() <= 1 {
            return;
        }
        let playing = !self.workspace.playback.playing;
        self.workspace.playback.playing = playing;
        self.last_advance = playing.then(Instant::now);
        if playing {
            self.workspace.playback.previewing = false;
            self.last_animation = None;
        }
    }

    fn step_time(&mut self, delta: i64) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        let frames = scene.frame_count();
        if frames <= 1 {
            return;
        }
        let time = (i64::from(scene.time()) + delta).rem_euclid(i64::from(frames)) as u32;
        self.workspace.playback.playing = false;
        self.workspace.playback.previewing = false;
        self.last_advance = None;
        self.last_animation = None;
        self.session.set_time(time);
        self.scene_changed = true;
    }

    fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open an OME-Zarr folder")
            .pick_folder()
    }

    fn pick_image() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a microscopy image")
            .add_filter(
                "Microscopy image",
                &["czi", "nd2", "lif", "ome.tif", "ome.tiff", "tif", "tiff"],
            )
            .add_filter("ZEISS CZI", &["czi"])
            .add_filter("Nikon ND2", &["nd2"])
            .add_filter("Leica LIF", &["lif"])
            .add_filter("OME-TIFF", &["ome.tif", "ome.tiff", "tif", "tiff"])
            .pick_file()
    }

    fn pick_project() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a grana project")
            .add_filter("grana project", &["grana"])
            .pick_file()
    }

    fn project_path(&self, choose: bool) -> Option<PathBuf> {
        if !choose && let Some(path) = self.session.project() {
            return Some(path.to_path_buf());
        }
        rfd::FileDialog::new()
            .set_title("Save a grana project")
            .set_file_name(self.session.suggested_project())
            .add_filter("grana project", &["grana"])
            .save_file()
    }

    fn on_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        if self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.ui.editing())
        {
            self.request_redraw();
            return;
        }
        let focused = self
            .surface
            .as_ref()
            .is_some_and(|surface| surface.ui.focus_visible() || surface.ui.popup_open_any());
        if focused
            && matches!(
                event.logical_key,
                Key::Named(NamedKey::Space)
                    | Key::Named(NamedKey::ArrowLeft)
                    | Key::Named(NamedKey::ArrowRight)
                    | Key::Named(NamedKey::ArrowUp)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::Delete)
                    | Key::Named(NamedKey::Home)
                    | Key::Named(NamedKey::End)
            )
        {
            self.request_redraw();
            return;
        }
        if shortcut(self.modifiers) {
            if let Key::Character(text) = &event.logical_key {
                if text.as_str().eq_ignore_ascii_case("s") {
                    let view = self.view_state();
                    if let Some(path) = self.project_path(self.modifiers.shift_key()) {
                        self.session.save_project(path, view);
                    }
                } else if text.as_str().eq_ignore_ascii_case("o")
                    && let Some(path) = Self::pick_project()
                {
                    self.session.begin_project(path);
                } else if text.as_str().eq_ignore_ascii_case("z") {
                    let command = if self.modifiers.shift_key() {
                        Command::Redo
                    } else {
                        Command::Undo
                    };
                    self.apply(vec![command]);
                } else if text.as_str().eq_ignore_ascii_case("y") {
                    self.apply(vec![Command::Redo]);
                }
            }
            self.request_redraw();
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_play(),
            Key::Named(NamedKey::ArrowRight) => self.step_time(1),
            Key::Named(NamedKey::ArrowLeft) => self.step_time(-1),
            Key::Named(NamedKey::Escape) => {
                if self.workspace.annotation_pending.take().is_none()
                    && self.workspace.annotation_tool.take().is_none()
                    && self.workspace.seed.take().is_none()
                {
                    return;
                }
            }
            Key::Named(NamedKey::Delete) => {
                let annotation = self
                    .session
                    .scene()
                    .and_then(|scene| scene.annotations.selected());
                if let Some(index) = annotation {
                    self.apply(vec![Command::RemoveAnnotation(index)]);
                } else if self.session.delete_selected()
                    && let Some(engine) = self.engine.as_mut()
                {
                    engine.set_output_message(None);
                }
            }
            Key::Character(ref text) if text.as_str().eq_ignore_ascii_case("h") => {
                self.session.hide_selected();
            }
            _ => return,
        }
        self.request_redraw();
    }

    fn confirm_close(&self) -> bool {
        rfd::MessageDialog::new()
            .set_title("Unsaved analysis")
            .set_description("The project has unsaved changes. Close grana without saving them?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            == rfd::MessageDialogResult::Yes
    }

    fn request_redraw(&self) {
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    fn pump_capture(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            return;
        }
        if self
            .repaint_at
            .is_none_or(|deadline| Instant::now() >= deadline)
        {
            self.repaint_at = None;
            self.redraw();
        }
        if self.exiting {
            event_loop.exit();
            return;
        }
        match self.repaint_at {
            Some(deadline) => event_loop.set_control_flow(ControlFlow::WaitUntil(deadline)),
            None => event_loop.set_control_flow(ControlFlow::Poll),
        }
    }
}

fn annotation_hit(
    surface: &Surface,
    session: &Session,
    viewport: &Viewport,
    stage: &Stage,
) -> Option<(usize, grana_figure::AnnotationPart)> {
    let pointer = surface.ui.input().position();
    let pane = stage.pane_at(pointer)?;
    let scene = session.scene()?;
    let items = grana_figure::annotations(scene, scene.time());
    if items.is_empty() {
        return None;
    }
    let placed = match pane {
        Pane::Volume => {
            let rect = stage.volume_rect();
            if rect.height() <= 0.0 {
                return None;
            }
            let world = scene.dataset.world_size();
            let aspect = rect.width() / rect.height();
            let view_projection = viewport
                .camera()
                .view_projection(aspect, 0.5 * world.length());
            grana_figure::volume_annotations(&items, view_projection, rect)
        }
        Pane::Section => {
            let rect = stage.section_rect();
            if rect.height() <= 0.0 {
                return None;
            }
            let axis = viewport.section().axis();
            let tolerance = 1.5 * scene.dataset.voxel_size()[axis.index()];
            grana_figure::section_annotations(
                &items,
                viewport.camera().focus(),
                axis,
                viewport.section().span(),
                rect,
                tolerance,
            )
        }
    };
    grana_figure::pick_annotation(&placed, pointer)
}

fn poses(timeline: &grana_animation::Timeline) -> Vec<Pose> {
    (0..timeline.frames())
        .filter_map(|frame| timeline.sample_frame(frame))
        .map(|sample| Pose {
            time: sample.time,
            camera: sample.camera,
            section: sample.section,
        })
        .collect()
}

fn frame_stats(info: FrameInfo) -> FrameStats {
    FrameStats {
        volume: view_stats(info.volume),
        section: view_stats(info.section),
        bytes: info.bytes,
        geometry: info.geometry,
        fragments: info.fragments,
        indices: info.cut.indices,
        samples: info.samples,
        oit_layers: info.oit.layers,
        oit_dropped: info.oit.dropped,
    }
}

fn view_stats(info: grana_render::ViewInfo) -> ViewStats {
    ViewStats {
        level: info.level,
        visible: info.visible,
        demand: info.demand,
        resident: info.resident,
    }
}

fn interface_key(key: &Key) -> Option<grana_ui::InputKey> {
    match key {
        Key::Named(NamedKey::Escape) => Some(grana_ui::InputKey::Escape),
        Key::Named(NamedKey::Enter) => Some(grana_ui::InputKey::Enter),
        Key::Named(NamedKey::Tab) => Some(grana_ui::InputKey::Tab),
        Key::Named(NamedKey::Space) => Some(grana_ui::InputKey::Space),
        Key::Named(NamedKey::ArrowLeft) => Some(grana_ui::InputKey::Left),
        Key::Named(NamedKey::ArrowRight) => Some(grana_ui::InputKey::Right),
        Key::Named(NamedKey::ArrowUp) => Some(grana_ui::InputKey::Up),
        Key::Named(NamedKey::ArrowDown) => Some(grana_ui::InputKey::Down),
        Key::Named(NamedKey::Home) => Some(grana_ui::InputKey::Home),
        Key::Named(NamedKey::End) => Some(grana_ui::InputKey::End),
        Key::Named(NamedKey::PageUp) => Some(grana_ui::InputKey::PageUp),
        Key::Named(NamedKey::PageDown) => Some(grana_ui::InputKey::PageDown),
        Key::Named(NamedKey::Backspace) => Some(grana_ui::InputKey::Backspace),
        Key::Named(NamedKey::Delete) => Some(grana_ui::InputKey::Delete),
        Key::Character(text) => text.chars().next().map(grana_ui::InputKey::Character),
        _ => None,
    }
}

fn interface_cursor(cursor: grana_ui::Cursor) -> CursorIcon {
    match cursor {
        grana_ui::Cursor::Arrow => CursorIcon::Default,
        grana_ui::Cursor::Pointing => CursorIcon::Pointer,
        grana_ui::Cursor::Crosshair => CursorIcon::Crosshair,
        grana_ui::Cursor::Grab => CursorIcon::Grab,
        grana_ui::Cursor::Grabbing => CursorIcon::Grabbing,
        grana_ui::Cursor::ResizeHorizontal => CursorIcon::ColResize,
        grana_ui::Cursor::ResizeVertical => CursorIcon::RowResize,
        grana_ui::Cursor::Move => CursorIcon::Move,
        grana_ui::Cursor::Text => CursorIcon::Text,
    }
}

fn shortcut(modifiers: ModifiersState) -> bool {
    modifiers.control_key() || modifiers.super_key()
}

impl App {
    fn push_event(&mut self, event: &WindowEvent) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let input = surface.ui.input_mut();
        match event {
            WindowEvent::CursorMoved { position, .. } => input.push(UiEvent::Moved(UiVec2::new(
                position.x as f32 / scale,
                position.y as f32 / scale,
            ))),
            WindowEvent::CursorLeft { .. } => input.push(UiEvent::Leave),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => UiButton::Primary,
                    MouseButton::Right => UiButton::Secondary,
                    MouseButton::Middle => UiButton::Middle,
                    _ => return,
                };
                input.push(match state {
                    ElementState::Pressed => UiEvent::Down(button),
                    ElementState::Released => UiEvent::Up(button),
                });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(x, y) => UiVec2::new(*x, *y),
                    MouseScrollDelta::PixelDelta(position) => {
                        UiVec2::new(position.x as f32, position.y as f32) / 48.0
                    }
                };
                input.push(UiEvent::Wheel(lines));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                input.push(UiEvent::Modifiers(UiModifiers {
                    shift: state.shift_key(),
                    control: state.control_key(),
                    alt: state.alt_key(),
                    logo: state.super_key(),
                }));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let Some(key) = interface_key(&event.logical_key) else {
                    return;
                };
                input.push(UiEvent::Key {
                    key,
                    pressed: event.state == ElementState::Pressed,
                    repeat: event.repeat,
                });
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                input.push(UiEvent::Preedit((!text.is_empty()).then(|| text.clone())));
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                input.push(UiEvent::Text(text.clone()));
            }
            _ => {}
        }
        self.request_redraw();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.surface.is_none() {
            self.resume(event_loop);
        }
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.surface.is_none() {
            return;
        }
        self.push_event(&event);
        match event {
            WindowEvent::CloseRequested => {
                if !self.dirty() || self.confirm_close() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.surface.as_mut() {
                    surface.gpu.resize([size.width, size.height]);
                    surface.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                if path
                    .extension()
                    .is_some_and(|extension| extension == "grana")
                {
                    self.session.begin_project(path);
                } else if grana_session::is_image(&path) {
                    self.session.begin_load(path);
                } else {
                    self.message = Some(format!(
                        "{} is neither a supported image nor a grana project",
                        path.display()
                    ));
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.on_key(&event);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exiting {
            event_loop.exit();
            return;
        }
        if self.capture.is_some() {
            self.pump_capture(event_loop);
            return;
        }
        if self.repaint_at.is_none() {
            if self.workspace.playback.playing {
                self.repaint_at = Some(Instant::now() + PLAYBACK_INTERVAL);
            } else if self.streaming() {
                self.repaint_at = Some(Instant::now() + STREAM_POLL);
            }
        }
        let Some(deadline) = self.repaint_at else {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        };
        if Instant::now() >= deadline {
            self.repaint_at = None;
            if let Some(surface) = &self.surface {
                surface.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }
}
