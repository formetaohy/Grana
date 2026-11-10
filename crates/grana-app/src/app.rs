use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Vec2, Vec3};
use grana_animation::Keyframe;
use grana_display::Camera;
use grana_editor::{
    AnnotationTool, GrabDrag, MaskBrush, PaintDrag, Panel, Stage, ToolDrag, Workspace,
};
use grana_engine::{Engine, Picked, SAMPLE_LIMIT, Viewport};
use grana_gpu::Gpu;
use grana_offline::{Destination, Outcome, Plan, Pose};
use grana_project::ViewState;
use grana_render::FrameInfo;
use grana_scene::{ObjectFrame, ObjectSetId, Revision};
use grana_session::{Command, Effects, MaskMode, MaskTool, ObjectAnchor, Session, SessionView};
use grana_ui::{
    Appearance, Button as UiButton, Event as UiEvent, Modifiers as UiModifiers, Theme, Ui,
    Vec2 as UiVec2,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::clipboard::SystemClipboard;
use crate::export::{Export, Report};
use crate::preference::Preference;
use crate::stream::{Stream, View as StreamView};

const STREAM_POLL: Duration = Duration::from_millis(16);
const EXPORT_POLL: Duration = Duration::from_millis(200);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);
const CAPTURE_BUDGET: Duration = Duration::from_secs(240);
const TOOL_CLICK_SLOP: f32 = 3.0;
const AUTOSAVE_IDLE: Duration = Duration::from_secs(8);
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(120);

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    ui: Ui<grana_editor::Message>,
}

#[derive(Clone)]
enum Resume {
    Exit,
    Command(Command),
}

pub struct App {
    surface: Option<Surface>,
    session: Session,
    engine: Option<Engine>,
    start: Instant,
    capture: Option<PathBuf>,
    startup: Option<String>,
    elapsed_frames: u32,
    exiting: bool,
    camera: Camera,
    stage: Stage,
    workspace: Workspace,
    info: FrameInfo,
    replaced: Revision,
    content: Revision,
    needs_fit: bool,
    last_advance: Option<Instant>,
    last_animation: Option<Instant>,
    repaint_at: Option<Instant>,
    title: Option<String>,
    modifiers: ModifiersState,
    output: Option<String>,
    ime: bool,
    ime_area: Option<(i32, i32, u32, u32)>,
    uploads: u64,
    stream: Stream,
    pick_toggle: bool,
    mask_path: Vec<[f32; 3]>,
    plate: Option<(String, usize)>,
    export: Export,
    resume: Option<Resume>,
    autosave_at: Option<Instant>,
    changed_at: Option<Instant>,
    revision: Revision,
    appearance: Appearance,
    contrast: bool,
    system_dark: bool,
    themed: Option<(Appearance, bool)>,
}

impl App {
    pub fn new(capture: Option<PathBuf>, startup: Option<String>) -> Self {
        let mut session = Session::new();
        session.attach_recent_store();
        session.attach_recovery_store();
        session.set_capture_factory(Arc::new(|| Arc::new(crate::capture::OfflineCapture::new())));
        let preference = Preference::load();
        let mut workspace = Workspace::default();
        workspace.appearance = preference.appearance;
        workspace.contrast = preference.contrast;
        Self {
            surface: None,
            session,
            engine: None,
            start: Instant::now(),
            capture,
            startup,
            elapsed_frames: 0,
            exiting: false,
            camera: Camera::new(35.0),
            stage: Stage::new(),
            workspace,
            info: FrameInfo::default(),
            replaced: Revision::ZERO,
            content: Revision::ZERO,
            needs_fit: true,
            last_advance: None,
            last_animation: None,
            repaint_at: None,
            title: None,
            modifiers: ModifiersState::default(),
            output: None,
            ime: false,
            ime_area: None,
            uploads: 0,
            stream: Stream::new(),
            pick_toggle: false,
            mask_path: Vec::new(),
            plate: None,
            export: Export::new(),
            resume: None,
            autosave_at: None,
            changed_at: None,
            revision: Revision::ZERO,
            appearance: preference.appearance,
            contrast: preference.contrast,
            system_dark: true,
            themed: None,
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
        self.system_dark = window.theme() != Some(winit::window::Theme::Light);
        let gpu = Gpu::new(window.clone(), [size.width, size.height]);
        let mut ui: Ui<grana_editor::Message> = Ui::new(&gpu.device, &gpu.queue, gpu.format());
        ui.set_clipboard(Box::new(SystemClipboard::new()));
        let mut engine = Engine::new(&gpu.device, &gpu.queue, gpu.format());
        match self.startup.take() {
            None => {
                if self.capture.is_some() || self.session.recovery().is_none() {
                    self.session.open_sample();
                }
            }
            Some(target) if grana_session::is_remote(&target) => self.session.begin_remote(target),
            Some(target) => {
                let path = PathBuf::from(target);
                if path
                    .extension()
                    .is_some_and(|extension| extension == "grana")
                {
                    self.session.begin_project(path);
                } else {
                    self.session.begin_load(path);
                }
            }
        }
        if let Some(scene) = self.session.scene() {
            engine.set_dataset(&gpu.device, scene.dataset.pyramid());
        }
        self.replaced = self.session.replaced();
        self.content = self.session.content();
        self.surface = Some(Surface { window, gpu, ui });
        self.engine = Some(engine);
        self.themed = None;
        self.sync_theme();
    }

    fn sync_theme(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let key = (self.appearance, self.contrast);
        if self.themed == Some(key) {
            return;
        }
        self.themed = Some(key);
        let theme = if self.appearance.dark(self.system_dark) {
            Theme::dark()
        } else {
            Theme::light()
        };
        *surface.ui.theme_mut() = theme.with_contrast(self.contrast);
        surface.window.set_theme(match self.appearance {
            Appearance::System => None,
            Appearance::Light => Some(winit::window::Theme::Light),
            Appearance::Dark => Some(winit::window::Theme::Dark),
        });
        surface.window.request_redraw();
    }

    fn sync_preference(&mut self) {
        if self.workspace.appearance == self.appearance && self.workspace.contrast == self.contrast
        {
            return;
        }
        self.appearance = self.workspace.appearance;
        self.contrast = self.workspace.contrast;
        let preference = Preference {
            appearance: self.appearance,
            contrast: self.contrast,
        };
        if let Err(detail) = preference.save() {
            self.output = Some(detail);
        }
        self.themed = None;
        self.sync_theme();
        self.request_redraw();
    }

    fn redraw(&mut self) {
        self.session.poll();
        if self.follow_resume() {
            return;
        }
        self.track_changes();
        self.poll_export();
        self.sync_plate_panel();
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
        self.sync_preference();
        self.sync_ime();
        self.interact_viewport();
        if self.apply(commands) {
            return;
        }
        self.session.apply_measure();
        self.sync_title();
        self.autosave();
        self.advance_playback();
        self.advance_animation();
        self.fit_view();
        let mut delay = self.render().unwrap_or(Duration::MAX);
        if animating {
            delay = Duration::ZERO;
        } else if self.workspace.playback.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.session.scene().is_some()
            && (self.info.samples < SAMPLE_LIMIT || self.info.fading)
        {
            delay = Duration::ZERO;
        }
        if self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if self.workspace.playback.playing {
            delay = delay.min(PLAYBACK_INTERVAL);
        }
        if self.export.running() {
            delay = delay.min(EXPORT_POLL);
        }
        self.schedule(delay);
    }

    fn poll_export(&mut self) {
        for report in self.export.poll() {
            self.output = Some(match report {
                Report::Finished(Outcome::Complete { path }) => {
                    format!("saved {}", grana_offline::name(&path))
                }
                Report::Finished(Outcome::Failed { detail }) => detail,
                Report::Finished(Outcome::Cancelled) => "the export was cancelled".to_string(),
                Report::Closed => "the export worker stopped".to_string(),
            });
        }
    }

    fn sync_plate_panel(&mut self) {
        let identity = self
            .session
            .plate()
            .map(|view| (view.plate.name().to_string(), view.plate.len()));
        if identity == self.plate {
            return;
        }
        self.plate = identity;
        if self.plate.is_some() && self.workspace.panel.is_none() {
            self.workspace.panel = Some(Panel::Plate);
            self.workspace.last_panel = Panel::Plate;
            self.workspace.dock_height = self.workspace.dock_height.max(600.0);
        }
    }

    fn exchange_bricks(&mut self) {
        let App {
            surface,
            engine,
            session,
            info,
            uploads,
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
                info.view = requests.view;
                let mut bricks =
                    Vec::with_capacity(requests.volume.len() + requests.prefetch.len());
                bricks.extend_from_slice(&requests.volume);
                bricks.extend_from_slice(&requests.prefetch);
                session.request(requests.time, &bricks);
            }
        }
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            *uploads += engine.upload(&surface.gpu.queue, &bricks) as u64;
        }
        let replaced = session.replaced() != self.replaced;
        let reloaded = session.content() != self.content;
        if replaced || reloaded {
            self.replaced = session.replaced();
            self.content = session.content();
            if let Some(scene) = session.scene() {
                engine.set_dataset(&surface.gpu.device, scene.dataset.pyramid());
            }
            *info = FrameInfo::default();
        }
        if replaced {
            self.needs_fit = true;
            self.workspace.playback.playing = false;
            self.workspace.playback.previewing = false;
            self.last_advance = None;
            self.last_animation = None;
        }
        let view = session.take_view();
        if let Some(view) = view {
            self.apply_view(&view);
        }
    }

    fn fit_view(&mut self) {
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let viewport = Viewport::screen(scale, size);
        self.stage.set_region(viewport.region);
        let sized = viewport.region[2] > 64 && viewport.region[3] > 64;
        if self.needs_fit && sized {
            self.camera
                .fit(scene.dataset.world_size(), viewport.aspect());
            self.needs_fit = false;
        }
    }

    fn viewport(&self) -> Viewport {
        let scale = self
            .surface
            .as_ref()
            .map_or(1.0, |surface| surface.window.scale_factor() as f32);
        let size = self
            .surface
            .as_ref()
            .map_or([1, 1], |surface| surface.gpu.size());
        Viewport::screen(scale, size)
    }

    fn schedule(&mut self, delay: Duration) {
        self.elapsed_frames += 1;
        let loaded = self.session.scene().is_some();
        let streamed = self.info.view.resident >= self.info.view.demand;
        let settled = loaded
            && streamed
            && self.info.samples >= SAMPLE_LIMIT
            && !self.info.fading
            && !self.session.previewing();
        let occupied = self.streaming();
        if let Some(path) = self.capture.clone()
            && self.elapsed_frames > 30
            && (settled || (!occupied && !loaded) || self.start.elapsed() > CAPTURE_BUDGET)
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
        if self.session.scene().is_none() {
            if let Some(surface) = self.surface.as_mut() {
                surface
                    .window
                    .set_cursor(interface_cursor(surface.ui.cursor()));
            }
            return;
        }
        let (interaction, hovered) = {
            let App {
                surface,
                session,
                camera,
                stage,
                workspace,
                ..
            } = self;
            let Some(surface) = surface.as_mut() else {
                return;
            };
            let tool = workspace
                .annotation_tool
                .filter(|_| workspace.seed.is_none() && workspace.mask.is_none());
            let paint = workspace.mask.is_some();
            let slice = camera.slice().is_some();
            let blocked = surface.ui.pointer_over_chrome()
                || surface.ui.pointer_over_widget()
                || surface.ui.pointer_captured();
            let hovered = (!blocked && !slice && tool.is_none() && workspace.seed.is_none())
                .then(|| annotation_hit(surface, session, camera, stage))
                .flatten();
            let pressed = surface.ui.input().button(UiButton::Primary).pressed;
            let grabbed = hovered.is_some() && pressed;
            let interaction = stage.input(
                &mut surface.ui,
                camera,
                grana_editor::Intent {
                    tool,
                    paint,
                    grabbed,
                },
            );
            if interaction.camera {
                if let Some(scene) = session.scene() {
                    camera.clamp_focus(0.5 * scene.dataset.world_size());
                }
                surface.window.request_redraw();
            }
            if let Some(steps) = interaction.slice
                && let Some(scene) = session.scene()
            {
                grana_editor::slice_step(scene, camera, steps);
                surface.window.request_redraw();
            }
            let cursor = if paint || workspace.seed.is_some() || tool.is_some() {
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
        if let Some(tool) = interaction.tool
            && self.camera.slice().is_none()
        {
            self.use_tool(tool);
        }
        if let Some(grab) = interaction.grab {
            self.drag_annotation(grab);
        }
        if let Some(drag) = interaction.paint
            && self.camera.slice().is_none()
        {
            self.paint_mask(drag);
        }
        if let Some(point) = interaction.pick
            && self.camera.slice().is_none()
        {
            match self.workspace.seed {
                Some(set) => self.place_seed(set, point, interaction.modifiers.alt),
                None => {
                    self.pick_toggle = interaction.modifiers.shift || interaction.modifiers.control;
                    self.pick(point);
                }
            }
            self.request_redraw();
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
                self.place_annotation(tool, drag.position);
            }
        }
    }

    fn draw_region(&mut self, drag: ToolDrag) {
        if drag.finished {
            self.default_region(drag.start);
        }
    }

    fn region_axes(&self) -> [Vec3; 3] {
        [Vec3::X, Vec3::Y, Vec3::Z]
    }

    fn region_depth(&self, scene: &grana_scene::Scene, normal: Vec3) -> f32 {
        let world = scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size().min_element();
        (0.5 * world.abs().dot(normal.abs())).max(voxel)
    }

    fn default_region(&mut self, point: Vec2) {
        let Some(centre) = self.annotation_point(point) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let half = 0.5 * scene.dataset.world_size();
        let voxel = scene.dataset.voxel_size();
        let axes = self.region_axes();
        let reach = (0.06 * half.min_element()).max(voxel.min_element());
        let extent = Vec3::new(reach, reach, self.region_depth(scene, axes[2]));
        let centre = Vec3::from(centre).clamp(-half, half);
        self.apply(vec![Command::SetRegion(grana_volume::Region::new(
            centre, axes, extent,
        ))]);
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
        let Some(offset) = self.annotation_offset(index, drag.delta) else {
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

    fn annotation_offset(&self, index: usize, delta: Vec2) -> Option<Vec3> {
        let scene = self.session.scene()?;
        let anchor = scene.annotations.get(index)?.kind().anchor();
        let rect = self.stage.viewport();
        if rect.height() <= 0.0 {
            return None;
        }
        let camera = &self.camera;
        let depth = (Vec3::from(anchor) - camera.eye()).dot(camera.forward());
        let per_point = camera.world_per_point(rect.height(), depth);
        Some(camera.right() * delta.x * per_point - camera.up() * delta.y * per_point)
    }

    fn paint_mask(&mut self, drag: PaintDrag) {
        let Some(brush) = self.workspace.mask else {
            return;
        };
        let Some(point) = self.annotation_point(drag.position) else {
            self.mask_path.clear();
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let voxel = scene.dataset.voxel_size().min_element();
        let moved = self
            .mask_path
            .last()
            .is_some_and(|last| (Vec3::from(point) - Vec3::from(*last)).length() > 0.5 * voxel);
        if moved || self.mask_path.is_empty() {
            self.mask_path.push(point);
        }
        if !drag.finished {
            return;
        }
        let points = std::mem::take(&mut self.mask_path);
        if points.is_empty() {
            return;
        }
        let Some(anchor) = self.mask_anchor(brush.set, points[0]) else {
            self.set_output_message("paint inside an object of the object set");
            return;
        };
        let (tool, mode) = match brush.tool {
            grana_editor::MaskBrushTool::Brush => (MaskTool::Brush, MaskMode::Add),
            grana_editor::MaskBrushTool::Erase => (MaskTool::Brush, MaskMode::Remove),
            grana_editor::MaskBrushTool::Wand => (MaskTool::Wand, MaskMode::Add),
        };
        let points = match tool {
            MaskTool::Brush => points,
            MaskTool::Wand => vec![point],
        };
        let time = scene.time();
        self.apply(vec![Command::PaintMask {
            set: brush.set,
            frame: time,
            anchor,
            tool,
            mode,
            radius: brush.radius,
            tolerance: brush.tolerance,
            points,
        }]);
        self.retarget_mask();
    }

    fn mask_anchor(&self, set: ObjectSetId, point: [f32; 3]) -> Option<ObjectAnchor> {
        let scene = self.session.scene()?;
        let source = scene
            .objects
            .get(set)
            .and_then(|object| object.provenance().source())
            .unwrap_or(set);
        let object = scene.objects.get(source)?;
        let ObjectFrame::Surfaces(surfaces) = object.frame(scene.time())? else {
            return None;
        };
        let anchors = surfaces
            .objects()
            .iter()
            .map(|statistics| ObjectAnchor::new(statistics.centroid(), statistics.bounds()))
            .collect::<Vec<_>>();
        let voxel = scene.dataset.voxel_size().min_element();
        grana_session::match_anchor(&anchors, point, voxel).map(|index| anchors[index])
    }

    fn retarget_mask(&mut self) {
        let Some(brush) = self.workspace.mask else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        if scene
            .objects
            .get(brush.set)
            .is_some_and(|object| object.provenance().masking().is_some())
        {
            return;
        }
        let Some(masked) = scene.objects.iter().find_map(|object| {
            object
                .provenance()
                .masking()
                .and_then(|(source, _)| (source == brush.set).then_some(object.id()))
        }) else {
            return;
        };
        self.workspace.mask = Some(MaskBrush {
            set: masked,
            ..brush
        });
    }

    fn place_seed(&mut self, set: ObjectSetId, point: Vec2, remove: bool) {
        if self
            .session
            .scene()
            .is_none_or(|scene| scene.seed_source(set).is_some())
        {
            return;
        }
        let Some((origin, direction)) = self.volume_ray(point) else {
            return;
        };
        let Some(scene) = self.session.scene() else {
            return;
        };
        let channel = scene.objects.get(set).map_or(0, |object| object.channel());
        let position = match scene.surface_ray(set, origin, direction) {
            Ok(Some(hit)) => hit.to_array(),
            Ok(None) => match self.session.seed_peak(channel, origin, direction) {
                Some(peak) => peak,
                None => match self.annotation_point(point) {
                    Some(position) => position,
                    None => return,
                },
            },
            Err(detail) => {
                self.output = Some(detail);
                return;
            }
        };
        let voxel = scene.dataset.voxel_size().min_element();
        let radius = 3.0 * voxel;
        if remove {
            let mut nearest: Option<(f32, usize)> = None;
            for (index, seed) in scene.seeds(set).iter().enumerate() {
                let distance = (Vec3::from(*seed) - Vec3::from(position)).length();
                if distance <= radius && nearest.is_none_or(|(best, _)| distance < best) {
                    nearest = Some((distance, index));
                }
            }
            if let Some((_, index)) = nearest {
                self.apply(vec![Command::RemoveSeed { set, index }]);
            }
            return;
        }
        self.apply(vec![Command::AddSeed { set, position }]);
    }

    fn volume_ray(&self, point: Vec2) -> Option<(Vec3, Vec3)> {
        let surface = self.surface.as_ref()?;
        let region = self.stage.region();
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
        let ray = self.camera.ray(ndc, aspect);
        Some((ray.origin, ray.direction))
    }

    fn annotation_point(&self, point: Vec2) -> Option<[f32; 3]> {
        let scene = self.session.scene()?;
        let (origin, direction) = self.volume_ray(point)?;
        if let Ok(Some(hit)) = scene.surface_hit(origin, direction) {
            return Some(hit.to_array());
        }
        let half = 0.5 * scene.dataset.world_size();
        let clip = scene.clip;
        let (normal, offset) = if clip.enabled() {
            (clip.normal(), clip.offset())
        } else {
            let normal = self.camera.forward();
            (normal, normal.dot(self.camera.pivot()))
        };
        let component = direction.dot(normal);
        if component.abs() <= 1.0e-6 {
            return None;
        }
        let distance = (offset - normal.dot(origin)) / component;
        if distance <= 0.0 {
            return None;
        }
        let position = (origin + direction * distance).clamp(-half, half);
        Some(position.to_array())
    }

    fn place_annotation(&mut self, tool: AnnotationTool, point: Vec2) {
        let Some(position) = self.annotation_point(point) else {
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
            if command.replaces_document()
                && !self.release_document(Resume::Command(command.clone()))
            {
                continue;
            }
            let effects = self.session.execute(command);
            if self.react(effects) {
                return true;
            }
        }
        self.validate_mask();
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
        if effects.snapshot {
            self.begin_snapshot();
        }
        if let Some(transparent) = effects.movie {
            self.begin_movie(transparent);
        }
        if effects.sequence {
            self.begin_sequence();
        }
        if effects.cancel_export {
            self.cancel_export();
        }
        if effects.keyframe {
            self.set_keyframe();
        }
        false
    }

    fn validate_mask(&mut self) {
        let Some(brush) = self.workspace.mask else {
            return;
        };
        let held = self
            .session
            .scene()
            .is_some_and(|scene| scene.objects.get(brush.set).is_some());
        if !held {
            self.workspace.mask = None;
            self.mask_path.clear();
        }
    }

    fn run_ui(&mut self) -> (Vec<Command>, bool) {
        let App {
            surface,
            session,
            camera,
            stage,
            workspace,
            export,
            output,
            ..
        } = self;
        let Some(surface) = surface.as_mut() else {
            return (Vec::new(), false);
        };
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let points = UiVec2::new(size[0] as f32 / scale, size[1] as f32 / scale);
        surface.ui.resize(size);
        let actions = surface
            .ui
            .update(points, scale, self.start.elapsed().as_secs_f64());
        let exporting = export.progress();
        let export_message = output.clone();
        let view = SessionView {
            scene: session.scene(),
            measure: session.measure(),
            timeline: session.timeline(),
            jobs: session.progress(),
            io: session.io(exporting, export_message),
            batch: session.batch(),
            history: session.history(),
            series: session.series(),
            plate: session.plate(),
            labels: session.label_images(),
            views: session.views(),
            recent: session.recent(),
            import: session.import(),
            recovery: session.recovery(),
            export: session.export(),
        };
        let commands =
            grana_editor::build(&mut surface.ui, actions, &view, camera, stage, workspace);
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
        let viewport = self.viewport();
        let playing = self.workspace.playback.playing;
        let cap = self.level_cap();
        let App {
            surface,
            engine,
            session,
            camera,
            output,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_mut()) else {
            return None;
        };
        if let Some(scene) = session.scene() {
            let info = match engine.draw_viewport(
                &surface.gpu.device,
                &surface.gpu.queue,
                grana_engine::ViewportFrame {
                    target: surface.ui.scene_view(),
                    scene,
                    camera,
                    size: viewport.size,
                    viewport: viewport.region,
                    revision: session.revision().raw(),
                    prefetch: playing,
                    level_cap: cap,
                },
            ) {
                Ok(info) => info,
                Err(detail) => {
                    *output = Some(detail);
                    return Some(Duration::ZERO);
                }
            };
            self.info = info;
        } else {
            self.info = FrameInfo::default();
        }
        self.present_ui();
        Some(Duration::MAX)
    }

    fn level_cap(&mut self) -> usize {
        let Some((levels, brick_bytes)) = self.session.scene().map(|scene| {
            (
                scene.dataset.level_count(),
                scene.dataset.pyramid().brick_bytes(),
            )
        }) else {
            return 0;
        };
        let view = StreamView {
            remote: self.session.remote(),
            levels,
            rate: self.session.throughput(),
            bricks: self.info.view.demand,
            rendered: self.info.view.level,
            brick_bytes,
            now: Instant::now(),
        };
        self.stream.cap(view)
    }

    fn output_size(&self, width: u32, even: bool) -> grana_display::OutputSize {
        let viewport = self.viewport();
        let height = (f64::from(width) * f64::from(viewport.region[3])
            / f64::from(viewport.region[2]).max(1.0))
        .round()
        .max(1.0) as u32;
        let size = grana_display::OutputSize::of(width, height);
        if even { size.even() } else { size }
    }

    fn native_width(&self) -> u32 {
        self.viewport().size[0].max(1)
    }

    fn pose(&self) -> Pose {
        Pose {
            time: self.session.scene().map_or(0, |scene| scene.time()),
            camera: self.camera.state(),
        }
    }

    fn export_ready(&mut self) -> bool {
        if self.export.running() {
            self.set_output_message("wait for the running export to finish");
            return false;
        }
        if self.session.loading().is_some() {
            self.set_output_message("the dataset is still opening");
            return false;
        }
        if self.session.analysing() {
            self.set_output_message("wait for the running analysis before exporting");
            return false;
        }
        true
    }

    fn start_export(&mut self, plan: Plan) {
        let Some(scene) = self.session.scene() else {
            self.set_output_message("open a dataset before exporting");
            return;
        };
        let Some(image) = self.session.image() else {
            self.set_output_message("the dataset is still opening");
            return;
        };
        match self.export.start(plan, image, scene) {
            Ok(()) => {
                self.output = None;
                self.repaint_at = Some(Instant::now());
            }
            Err(detail) => self.output = Some(detail),
        }
    }

    fn begin_snapshot(&mut self) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let export = self.session.export();
        let slug = grana_session::slug(scene.dataset.name());
        let width = if export.image == 0 {
            self.native_width()
        } else {
            export.image
        };
        let suggested = format!("{slug}-image.png");
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save an image")
            .set_file_name(suggested)
            .add_filter("PNG (8-bit)", &["png"])
            .add_filter("OME-TIFF (16-bit)", &["tif", "tiff"])
            .save_file()
        else {
            return;
        };
        let size = self.output_size(width, false);
        let poses = vec![self.pose()];
        self.start_export(Plan {
            destination: Destination::Image { path },
            size,
            transparent: export.transparent,
            poses,
            fps: 0,
            region: scene.region,
        });
    }

    fn begin_movie(&mut self, transparent: bool) {
        if !self.export_ready() {
            return;
        }
        if self.session.timeline().is_empty() {
            self.set_output_message("set a keyframe before exporting");
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let width = self.session.export().movie;
        let slug = grana_session::slug(scene.dataset.name());
        let (suggested, title, filter, extensions) = if transparent {
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
        let poses = grana_offline::poses(self.session.timeline());
        let fps = self.session.timeline().fps();
        let size = self.output_size(width, true);
        self.start_export(Plan {
            destination: Destination::Movie { path },
            size,
            transparent,
            poses,
            fps,
            region: scene.region,
        });
    }

    fn begin_sequence(&mut self) {
        if !self.export_ready() {
            return;
        }
        let Some(scene) = self.session.scene() else {
            return;
        };
        let export = self.session.export();
        let frames = scene.frame_count();
        if frames == 0 {
            self.set_output_message("the dataset has no time point to export");
            return;
        }
        let stem = grana_session::slug(scene.dataset.name());
        let camera = self.camera.state();
        let Some(parent) = rfd::FileDialog::new()
            .set_title("Choose a folder for the frame sequence")
            .pick_folder()
        else {
            return;
        };
        let width = if export.image == 0 {
            self.native_width()
        } else {
            export.image
        };
        let directory = parent.join(format!("{stem}-sequence"));
        let poses = (0..frames).map(|time| Pose { time, camera }).collect();
        let size = self.output_size(width, false);
        self.start_export(Plan {
            destination: Destination::Sequence { directory, stem },
            size,
            transparent: export.transparent,
            poses,
            fps: 0,
            region: scene.region,
        });
    }

    fn cancel_export(&mut self) {
        self.export.cancel();
        self.repaint_at = Some(Instant::now());
    }

    fn set_output_message(&mut self, message: &str) {
        self.output = Some(message.to_string());
    }

    fn present_ui(&mut self) {
        let revision = self.frame_revision();
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
        surface.ui.set_scene_revision(revision);
        surface.ui.render(&mut encoder, &view);
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
    }

    fn frame_revision(&self) -> u64 {
        let mut stamp = Stamp::new();
        stamp.u64(self.session.revision().raw());
        stamp.u64(self.uploads);
        let camera = self.camera.state();
        for value in camera.pivot.into_iter().chain([
            camera.yaw,
            camera.pitch,
            camera.distance,
            camera.fov_y,
            camera.aperture,
            camera.focus,
        ]) {
            stamp.f32(value);
        }
        stamp.u64(camera.projection.index() as u64);
        for value in self.stage.region() {
            stamp.u64(u64::from(value));
        }
        let info = self.info;
        stamp.u64(u64::from(info.samples));
        stamp.u64(u64::from(u8::from(info.settled)) << 1 | u64::from(u8::from(info.fading)));
        for value in [
            info.view.level,
            info.view.visible,
            info.view.demand,
            info.view.resident,
        ] {
            stamp.u64(value as u64);
        }
        if let Some(scene) = self.session.scene() {
            stamp.u64(scene.time() as u64);
        }
        stamp.0
    }

    fn streaming(&self) -> bool {
        let engine = self.engine.as_ref();
        self.session.batch().is_some_and(|batch| batch.running)
            || self.workspace.playback.playing
            || self.session.loading().is_some()
            || self.session.previewing()
            || self.session.transferring()
            || self.session.writing()
            || !self.session.progress().is_empty()
            || self.session.pending() > 0
            || engine.is_some_and(|engine| engine.requests_pending() || engine.pick_pending())
            || (self.session.scene().is_some() && self.info.view.resident < self.info.view.demand)
    }

    fn view_state(&self) -> ViewState {
        ViewState {
            camera: self.camera.state(),
            timeline: self.session.timeline().clone(),
            export: self.session.export(),
        }
    }

    fn apply_view(&mut self, view: &ViewState) {
        self.camera.restore(view.camera);
        self.session.restore_timeline(view.timeline.clone());
        self.workspace.playback.selected = None;
        self.workspace.playback.position = 0.0;
        self.needs_fit = false;
    }

    fn dirty(&self) -> bool {
        self.session.dirty()
    }

    fn track_changes(&mut self) {
        let revision = self.session.revision();
        if revision != self.revision {
            self.revision = revision;
            self.changed_at = Some(Instant::now());
        }
    }

    fn autosave(&mut self) {
        if self.capture.is_some() {
            return;
        }
        let now = Instant::now();
        let idle = self
            .changed_at
            .is_none_or(|at| now.duration_since(at) >= AUTOSAVE_IDLE);
        let due = self
            .autosave_at
            .is_none_or(|at| now.duration_since(at) >= AUTOSAVE_INTERVAL);
        if !idle || !due || !self.session.dirty() || self.session.analysing() {
            return;
        }
        self.autosave_at = Some(now);
        let view = self.view_state();
        self.session.autosave(view);
    }

    fn autosave_wait(&self) -> Duration {
        let held = self
            .autosave_at
            .map_or(Duration::ZERO, |at| Instant::now().duration_since(at));
        AUTOSAVE_INTERVAL.saturating_sub(held).max(AUTOSAVE_IDLE)
    }

    fn release_document(&mut self, next: Resume) -> bool {
        if self.resume.is_some() {
            return false;
        }
        if self.session.transferring() {
            self.resume = Some(next);
            return false;
        }
        if !self.session.dirty() {
            return true;
        }
        let answer = rfd::MessageDialog::new()
            .set_title("Unsaved analysis")
            .set_description("The analysis has unsaved changes. Save them before continuing?")
            .set_level(rfd::MessageLevel::Warning)
            .set_buttons(rfd::MessageButtons::YesNoCancel)
            .show();
        match answer {
            rfd::MessageDialogResult::No => {
                self.session.discard_recovery();
                true
            }
            rfd::MessageDialogResult::Yes => {
                let view = self.view_state();
                let Some(path) = self.project_path(false) else {
                    return false;
                };
                self.session.save_project(path, view);
                self.resume = Some(next);
                false
            }
            _ => false,
        }
    }

    fn follow_resume(&mut self) -> bool {
        if self.resume.is_none() || self.session.transferring() {
            return false;
        }
        if self.session.dirty() {
            self.resume = None;
            return false;
        }
        match self.resume.take() {
            Some(Resume::Exit) => {
                self.exiting = true;
                true
            }
            Some(Resume::Command(command)) => {
                self.apply(vec![command]);
                false
            }
            None => false,
        }
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

    fn pick(&mut self, point: Vec2) {
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
        engine.request_pick(&surface.gpu.device, &surface.gpu.queue, point);
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
            camera: self.camera.state(),
        };
        let index = self.session.insert_keyframe(key);
        self.workspace.playback.position = at;
        self.workspace.playback.selected = Some(index);
        self.output = None;
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
    }

    fn pose_animation(&mut self) {
        let position = self.workspace.playback.position;
        if let Some(sample) = self.session.timeline().sample(position) {
            self.camera.restore(sample.camera);
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

    fn step_slice(&mut self, steps: i32) {
        let Some(scene) = self.session.scene() else {
            return;
        };
        grana_editor::slice_step(scene, &mut self.camera, steps);
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
                &[
                    "czi", "nd2", "lif", "ims", "ome.tif", "ome.tiff", "tif", "tiff",
                ],
            )
            .add_filter("ZEISS CZI", &["czi"])
            .add_filter("Nikon ND2", &["nd2"])
            .add_filter("Leica LIF", &["lif"])
            .add_filter("Imaris IMS", &["ims"])
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
            if let Key::Character(text) = &event.logical_key
                && let Some(key) = text.chars().next()
                && let Some(message) = grana_editor::shortcut(key, self.modifiers.shift_key())
            {
                self.dispatch(message);
            }
            self.request_redraw();
            return;
        }
        if self.workspace.palette.is_some() {
            if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                self.dispatch(grana_editor::Message::ClosePalette);
            }
            self.request_redraw();
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_play(),
            Key::Named(NamedKey::ArrowRight) => self.step_time(1),
            Key::Named(NamedKey::ArrowLeft) => self.step_time(-1),
            Key::Named(NamedKey::ArrowUp) if self.camera.slice().is_some() => self.step_slice(1),
            Key::Named(NamedKey::ArrowDown) if self.camera.slice().is_some() => self.step_slice(-1),
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) => return,
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
                } else if self.session.delete_selected() {
                    self.output = None;
                }
            }
            Key::Character(ref text) if text.as_str().eq_ignore_ascii_case("h") => {
                self.session.hide_selected();
            }
            _ => return,
        }
        self.request_redraw();
    }

    fn dispatch(&mut self, message: grana_editor::Message) -> bool {
        let commands = {
            let App {
                surface,
                session,
                camera,
                stage,
                workspace,
                export,
                output,
                ..
            } = self;
            let Some(surface) = surface.as_mut() else {
                return false;
            };
            let view = SessionView {
                scene: session.scene(),
                measure: session.measure(),
                timeline: session.timeline(),
                jobs: session.progress(),
                io: session.io(export.progress(), output.clone()),
                batch: session.batch(),
                history: session.history(),
                series: session.series(),
                plate: session.plate(),
                labels: session.label_images(),
                views: session.views(),
                recent: session.recent(),
                import: session.import(),
                recovery: session.recovery(),
                export: session.export(),
            };
            let mut commands = Vec::new();
            grana_editor::apply(
                &mut surface.ui,
                vec![message],
                &view,
                camera,
                stage,
                workspace,
                &mut commands,
            );
            commands
        };
        self.apply(commands)
    }

    fn confirm_export_close(&self) {
        rfd::MessageDialog::new()
            .set_title("Export running")
            .set_description("An export is still writing its file. Cancel it before closing grana.")
            .set_level(rfd::MessageLevel::Info)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
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
    camera: &Camera,
    stage: &Stage,
) -> Option<(usize, grana_figure::AnnotationPart)> {
    let pointer = surface.ui.input().position();
    if !stage.contains(pointer) {
        return None;
    }
    let scene = session.scene()?;
    let items = grana_figure::annotations(scene, scene.time());
    if items.is_empty() {
        return None;
    }
    let rect = stage.viewport();
    if rect.height() <= 0.0 {
        return None;
    }
    let world = scene.dataset.world_size();
    let aspect = rect.width() / rect.height();
    let view_projection = camera.view_projection(aspect, 0.5 * world.length());
    let placed = grana_figure::volume_annotations(&items, view_projection, rect);
    grana_figure::pick_annotation(&placed, pointer)
}

struct Stamp(u64);

impl Stamp {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn u64(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn f32(&mut self, value: f32) {
        self.u64(u64::from(value.to_bits()));
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
                if self.capture.is_some() {
                    event_loop.exit();
                    return;
                }
                if self.export.running() {
                    self.confirm_export_close();
                    return;
                }
                if self.release_document(Resume::Exit) {
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
            WindowEvent::Moved(_) => {}
            WindowEvent::ThemeChanged(theme) => {
                if self.appearance == Appearance::System {
                    self.system_dark = theme == winit::window::Theme::Dark;
                    self.themed = None;
                    self.sync_theme();
                }
            }
            WindowEvent::DroppedFile(path) => {
                self.apply(vec![Command::OpenPath(path)]);
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
            } else if self.export.running() {
                self.repaint_at = Some(Instant::now() + EXPORT_POLL);
            } else if self.capture.is_none() && self.session.dirty() {
                self.repaint_at = Some(Instant::now() + self.autosave_wait());
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
