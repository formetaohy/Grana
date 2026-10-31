use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use grana_animation::{Keyframe, Sample, Timeline};
use grana_object::TrackSet;
use grana_pipeline::{
    self as pipeline, Analysis, BatchEvent, Event, Outcome, Plan, Poll, Protocol,
};
use grana_project::{Project, Source as ProjectSource, ViewState};
use grana_render::{BrickRequest, Camera, Frame, FrameInfo, Renderer, Requests, SectionCamera};
use grana_scene::{Dataset, Job, Measure, ObjectSetId, Origin, Progress, Scene};
use grana_ui::Target as FigureTarget;
use grana_ui::{
    Action, BatchDataset, Caption, EguiLayer, Figure, Message, Pane, Panel, State, Viewport,
};
use grana_volume::Spacing;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

use crate::demo;
use crate::gpu::Gpu;
use crate::loader::{Event as LoaderEvent, Loader};
use crate::movie::{self, Movie};
use crate::snapshot;

const TEXTURE_BUDGET: u64 = 512 << 20;
const SAMPLE_LIMIT: u32 = 32;
const SNAPSHOT_SCALE: u32 = 4;
const SNAPSHOT_SAMPLES: u32 = 8;
const SNAPSHOT_LIMIT: u32 = 8192;
const SNAPSHOT_ATTEMPTS: u32 = 512;
const MOVIE_SAMPLES: u32 = 16;
const MOVIE_ATTEMPTS: u32 = 4096;
const MOVIE_WIDTH: u32 = 1920;
const UPLOAD_BATCH: usize = 32;
const MAX_IN_FLIGHT: usize = 64;
const STREAM_POLL: Duration = Duration::from_millis(16);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const ANIMATION_INTERVAL: Duration = Duration::from_millis(16);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    egui: EguiLayer,
    figure: Figure,
    renderer: Renderer,
}

struct Pending {
    path: PathBuf,
    project: Project,
}

struct Snapshot {
    path: PathBuf,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    attempts: u32,
}

struct Animation {
    timeline: Timeline,
    position: f32,
    previewing: bool,
    last: Option<Instant>,
    selected: Option<usize>,
    width: u32,
    message: Option<String>,
}

impl Animation {
    fn new(frames: u32, interval: f32) -> Self {
        let duration = (frames as f32 * interval).clamp(1.0, Timeline::MAX_DURATION);
        Self {
            timeline: Timeline::new(30, duration),
            position: 0.0,
            previewing: false,
            last: None,
            selected: None,
            width: MOVIE_WIDTH,
            message: None,
        }
    }

    fn reset(&mut self, frames: u32, interval: f32) {
        let width = self.width;
        *self = Self::new(frames, interval);
        self.width = width;
    }

    fn stop(&mut self) {
        self.previewing = false;
        self.last = None;
    }
}

pub struct App {
    surface: Option<Surface>,
    scene: Option<Scene>,
    measure: Measure,
    panel: Option<Panel>,
    viewport: Viewport,
    animation: Animation,
    movie: Option<Movie>,
    source: Option<Loader>,
    analysis: Vec<Analysis>,
    progress: Vec<Progress>,
    in_flight: HashSet<BrickRequest>,
    frame: FrameInfo,
    loading: Option<String>,
    message: Option<Message>,
    repaint_at: Option<Instant>,
    needs_fit: bool,
    fit_section: bool,
    playing: bool,
    last_advance: Option<Instant>,
    project: Option<PathBuf>,
    pending: Option<Pending>,
    snapshot: Option<Snapshot>,
    saved: Option<u64>,
    title: Option<String>,
    modifiers: ModifiersState,
    protocol: Option<Protocol>,
    protocol_text: Option<String>,
    batch: Option<pipeline::Batch>,
    batch_datasets: Vec<PathBuf>,
    batch_rows: Vec<BatchDataset>,
    batch_output: Option<PathBuf>,
    batch_running: bool,
    batch_message: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface: None,
            scene: None,
            measure: Measure::new(),
            panel: Some(Panel::Statistics),
            viewport: Viewport::new(),
            animation: Animation::new(1, 0.0),
            movie: None,
            source: None,
            analysis: Vec::new(),
            progress: Vec::new(),
            in_flight: HashSet::new(),
            frame: FrameInfo::default(),
            loading: None,
            message: None,
            repaint_at: None,
            needs_fit: true,
            fit_section: false,
            playing: false,
            last_advance: None,
            project: None,
            pending: None,
            snapshot: None,
            saved: None,
            title: None,
            modifiers: ModifiersState::default(),
            protocol: None,
            protocol_text: None,
            batch: None,
            batch_datasets: Vec::new(),
            batch_rows: Vec::new(),
            batch_output: None,
            batch_running: false,
            batch_message: None,
        }
    }

    fn resume(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("grana")
                        .with_inner_size(LogicalSize::new(1440.0, 900.0)),
                )
                .expect("failed to create the window"),
        );
        let gpu = Gpu::new(window.clone());
        let egui = EguiLayer::new(&window, &gpu.device, gpu.format());
        let figure = Figure::new(&gpu.device, gpu.format());
        let (source, pyramid) = Loader::memory(demo::movie());
        let mut scene = Scene::new(Dataset::synthetic("synthetic fluorescence", pyramid));
        let staged = demo::stage(&mut scene);
        let mut renderer = Renderer::new(&gpu.device, &gpu.queue, gpu.format(), TEXTURE_BUDGET);
        renderer.set_dataset(&gpu.device, scene.dataset.pyramid());
        self.scene = Some(scene);
        self.source = Some(source);
        self.surface = Some(Surface {
            window,
            gpu,
            egui,
            figure,
            renderer,
        });
        if let Some(scene) = self.scene.as_ref() {
            self.animation
                .reset(scene.dataset.frame_count(), scene.dataset.frame_interval());
        }
        self.start_detect(staged.spots);
        self.start_detect(staged.surfaces);
        self.start_detect(staged.filaments);
    }

    fn redraw(&mut self) {
        self.poll_requests();
        self.poll_source();
        self.poll_analysis();
        self.poll_batch();
        if self.surface.is_none() || self.scene.is_none() {
            return;
        }
        if let Some(scene) = self.scene.as_ref() {
            let busy = self.measure.set().is_some_and(|set| {
                self.analysis
                    .iter()
                    .any(|analysis| analysis.job().set() == Some(set))
            });
            self.measure.sync(scene, busy);
        }
        let (output, actions) = self.run_ui();
        if self.apply(actions) {
            return;
        }
        if self.measure.take_dirty()
            && let Some(scene) = self.scene.as_mut()
        {
            self.measure.apply(scene);
        }
        self.sync_title();
        self.advance_playback();
        self.advance_animation();
        let surface = self.surface.as_ref().expect("the surface is alive");
        let scale = surface.window.scale_factor() as f32;
        let size = surface.gpu.size();
        let volume_region = physical_region(self.viewport.volume_rect(), scale, size);
        let section_region = physical_region(self.viewport.section_rect(), scale, size);
        self.viewport.set_regions(volume_region, section_region);
        let volume_aspect = volume_region[2] as f32 / volume_region[3] as f32;
        let section_aspect = section_region[2] as f32 / section_region[3] as f32;
        let (world, voxel) = {
            let scene = self.scene.as_ref().expect("the scene is alive");
            (scene.dataset.world_size(), scene.dataset.voxel_size())
        };
        if self.fit_section {
            self.viewport.fit_section(world, voxel, section_aspect);
            self.fit_section = false;
        }
        if self.needs_fit {
            self.viewport
                .fit(world, voxel, volume_aspect, section_aspect);
            self.needs_fit = false;
        }
        let mut delay = if self.movie.is_some() {
            self.record(output).unwrap_or(Duration::MAX)
        } else {
            self.render(output, volume_region, section_region)
                .unwrap_or(Duration::MAX)
        };
        if self.movie.is_some() {
            delay = Duration::ZERO;
        } else if self.animation.previewing {
            delay = delay.min(ANIMATION_INTERVAL);
        } else if self.frame.samples < SAMPLE_LIMIT {
            delay = Duration::ZERO;
        }
        if self.streaming() {
            delay = delay.min(STREAM_POLL);
        }
        if self.playing {
            delay = delay.min(PLAYBACK_INTERVAL);
        }
        if delay.is_zero() {
            self.repaint_at = None;
            if let Some(surface) = &self.surface {
                surface.window.request_redraw();
            }
        } else if delay < MAX_REPAINT_DELAY {
            self.repaint_at = Some(Instant::now() + delay);
        } else {
            self.repaint_at = None;
        }
    }

    fn apply(&mut self, actions: Vec<Action>) -> bool {
        for action in actions {
            match action {
                Action::OpenDataset => {
                    if let Some(path) = Self::pick_folder() {
                        self.begin_load(path);
                        return true;
                    }
                }
                Action::OpenProject => {
                    if let Some(path) = Self::pick_project() {
                        self.begin_project(path);
                        return true;
                    }
                }
                Action::SaveProject => self.save_project(false),
                Action::SaveProjectAs => self.save_project(true),
                Action::SaveSnapshot => self.begin_snapshot(),
                Action::Fit => self.needs_fit = true,
                Action::FitSection => self.fit_section = true,
                Action::SetSectionAxis(axis) => {
                    self.viewport.section_mut().set_axis(axis);
                    self.fit_section = true;
                }
                Action::SetSplit(split) => self.viewport.set_split(split),
                Action::SetTime(time) => self.scrub(time),
                Action::Select(selection) => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.select(selection);
                    }
                }
                Action::TogglePlay => self.toggle_play(),
                Action::AddObject(kind) => {
                    let channel = self.scene.as_ref().expect("the scene is alive").selected;
                    self.scene
                        .as_mut()
                        .expect("the scene is alive")
                        .add_object(kind, channel);
                }
                Action::RemoveObject(set) => self.remove_object(set),
                Action::Detect(set) => self.start_detect(set),
                Action::CancelDetect(set) => self.cancel(Job::Detect(set)),
                Action::ClearObject(set) => {
                    self.cancel(Job::Detect(set));
                    self.cancel(Job::Track(set));
                    if let Some(scene) = self.scene.as_mut() {
                        scene.clear_object(set);
                    }
                }
                Action::Track(set) => self.start_tracks(set),
                Action::CancelTrack(set) => self.cancel(Job::Track(set)),
                Action::ClearTracks(set) => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.clear_tracks(set);
                    }
                }
                Action::ExportMeasure => self.export_measure(),
                Action::SetPanel(panel) => self.panel = panel,
                Action::ComputeColoc => self.start_coloc(),
                Action::CancelColoc => self.cancel(Job::Coloc),
                Action::ExportColoc => self.export_coloc(),
                Action::CaptureProtocol => self.capture_protocol(),
                Action::LoadProtocol => self.load_protocol(),
                Action::SaveProtocol => self.save_protocol(),
                Action::AddBatchDatasets => self.add_batch_datasets(),
                Action::ClearBatchDatasets => {
                    self.batch_datasets.clear();
                    self.batch_rows.clear();
                }
                Action::ChooseBatchOutput => self.choose_batch_output(),
                Action::RunBatch => self.run_batch(),
                Action::CancelBatch => {
                    if let Some(batch) = &self.batch {
                        batch.cancel();
                    }
                }
                Action::ToggleAnimation => self.toggle_animation(),
                Action::ScrubAnimation(position) => self.scrub_animation(position),
                Action::SetKeyframe => self.set_keyframe(),
                Action::ClearKeyframes => {
                    self.animation.timeline.clear();
                    self.animation.selected = None;
                }
                Action::SelectKeyframe(index) => self.animation.selected = Some(index),
                Action::DeleteKeyframe(index) => self.delete_keyframe(index),
                Action::SetMovieWidth(width) => self.animation.width = width,
                Action::ExportMovie => self.begin_movie(),
                Action::CancelMovie => self.cancel_movie(),
            }
        }
        false
    }

    fn toggle_animation(&mut self) {
        if self.animation.previewing {
            self.animation.stop();
            self.request_redraw();
            return;
        }
        if self.animation.timeline.is_empty() {
            self.animation.message = Some("set a keyframe before previewing".to_string());
            return;
        }
        self.playing = false;
        self.last_advance = None;
        if self.animation.position >= self.animation.timeline.duration() {
            self.animation.position = 0.0;
        }
        self.animation.message = None;
        self.animation.previewing = true;
        self.animation.last = Some(Instant::now());
        self.pose_animation();
        self.request_redraw();
    }

    fn scrub_animation(&mut self, position: f32) {
        self.animation.stop();
        self.playing = false;
        self.last_advance = None;
        self.animation.position = position.clamp(0.0, self.animation.timeline.duration());
        self.pose_animation();
        self.request_redraw();
    }

    fn pose_animation(&mut self) {
        let position = self.animation.position;
        if let Some(sample) = self.animation.timeline.sample(position) {
            self.apply_sample(sample);
        }
    }

    fn apply_sample(&mut self, sample: Sample) {
        self.viewport.restore_view(sample.camera, sample.section);
        self.set_time(sample.time);
    }

    fn advance_animation(&mut self) {
        if !self.animation.previewing {
            return;
        }
        let now = Instant::now();
        let Some(last) = self.animation.last.replace(now) else {
            return;
        };
        let duration = self.animation.timeline.duration();
        let elapsed = now.duration_since(last).as_secs_f32().min(0.25);
        self.animation.position = (self.animation.position + elapsed) % duration;
        self.pose_animation();
    }

    fn set_keyframe(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let at = self
            .animation
            .position
            .clamp(0.0, self.animation.timeline.duration());
        let key = Keyframe {
            at,
            time: scene.time(),
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
        };
        let index = self.animation.timeline.insert(key);
        self.animation.position = at;
        self.animation.selected = Some(index);
        self.animation.message = None;
        self.request_redraw();
    }

    fn delete_keyframe(&mut self, index: usize) {
        if self.animation.timeline.remove(index).is_none() {
            return;
        }
        self.animation.selected = match self.animation.selected {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            selected => selected,
        };
        self.request_redraw();
    }

    fn toggle_play(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.frame_count() <= 1 {
            return;
        }
        self.playing = !self.playing;
        self.last_advance = self.playing.then(Instant::now);
        if self.playing {
            self.animation.stop();
        }
    }

    fn scrub(&mut self, time: u32) {
        self.playing = false;
        self.last_advance = None;
        self.animation.stop();
        self.set_time(time);
    }

    fn set_time(&mut self, time: u32) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        if time == scene.time() {
            return;
        }
        scene.set_time(time);
        self.in_flight.retain(|request| request.time >= time);
        if let Some(source) = &self.source {
            source.set_time(time);
        }
    }

    fn advance_playback(&mut self) {
        if !self.playing {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            self.playing = false;
            return;
        };
        if scene.frame_count() <= 1 {
            self.playing = false;
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
        self.set_time(next);
        self.last_advance = Some(now);
    }

    fn step_time(&mut self, delta: i64) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let frames = scene.frame_count();
        if frames <= 1 {
            return;
        }
        let time = (i64::from(scene.time()) + delta).rem_euclid(i64::from(frames)) as u32;
        self.scrub(time);
    }

    fn streaming(&self) -> bool {
        self.batch_running
            || self.playing
            || self.loading.is_some()
            || !self.analysis.is_empty()
            || !self.in_flight.is_empty()
            || self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.renderer.requests_pending())
            || (self.source.is_some()
                && (self.frame.volume.resident < self.frame.volume.demand
                    || self.frame.section.resident < self.frame.section.demand))
    }

    fn run_ui(&mut self) -> (egui::FullOutput, Vec<Action>) {
        let size = self
            .movie_layout()
            .map_or([MOVIE_WIDTH, MOVIE_WIDTH * 9 / 16], |layout| layout.0);
        let project = self.project.as_ref().map(|path| file_name(path));
        let dirty = self.dirty();
        let surface = self.surface.as_mut().expect("the surface is alive");
        let scene = self.scene.as_mut().expect("the scene is alive");
        let Surface {
            window,
            gpu,
            egui,
            figure: _,
            renderer: _,
        } = surface;
        let adapter = gpu.adapter_description.as_str();
        let loading = self.loading.as_deref();
        let message = self.message.as_ref();
        let frame = self.frame;
        let pending = self.in_flight.len();
        let analysis = self.progress.as_slice();
        let playing = self.playing;
        let scene_frames = scene.frame_count();
        let frame_interval = scene.dataset.frame_interval();
        let output_text = self
            .batch_output
            .as_ref()
            .map(|path| path.display().to_string());
        let movie = self.movie.as_ref().map(|movie| (movie.frame, movie.frames));
        let mut actions = Vec::new();
        let output = egui.run(window, |root| {
            actions = grana_ui::build(
                root,
                &mut State {
                    scene,
                    measure: &mut self.measure,
                    panel: &mut self.panel,
                    viewport: &mut self.viewport,
                    animation: grana_ui::Animation {
                        timeline: &mut self.animation.timeline,
                        position: self.animation.position,
                        previewing: self.animation.previewing,
                        selected: self.animation.selected,
                        width: self.animation.width,
                        size,
                        movie,
                        message: self.animation.message.as_deref(),
                        frames: scene_frames,
                        frame_interval,
                    },
                    adapter,
                    loading,
                    message,
                    project: project.as_deref(),
                    dirty,
                    frame,
                    pending,
                    playing,
                    analysis,
                    batch_protocol: self.protocol_text.as_deref(),
                    batch_datasets: &self.batch_rows,
                    batch_output: output_text.as_deref(),
                    batch_running: self.batch_running,
                    batch_message: self.batch_message.as_deref(),
                },
            );
        });
        (output, actions)
    }

    fn render(
        &mut self,
        output: egui::FullOutput,
        volume_region: [u32; 4],
        section_region: [u32; 4],
    ) -> Option<Duration> {
        if self.snapshot.is_some() {
            let delay = self.capture();
            self.present_ui(output);
            return delay;
        }
        let repaint_delay = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX);
        let info = {
            let surface = self.surface.as_mut()?;
            let scene = self.scene.as_mut()?;
            let size = surface.gpu.size();
            let time = scene.time();
            let region = scene.region;
            let exposure = scene.exposure;
            let rendered = scene.rendered();
            let frame = surface.gpu.acquire()?;
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let info = surface.renderer.draw(
                &surface.gpu.device,
                &surface.gpu.queue,
                Frame {
                    target: &view,
                    size,
                    volume_region,
                    section_region,
                    time,
                    camera: self.viewport.camera(),
                    section: self.viewport.section(),
                    region,
                    exposure,
                    samples: SAMPLE_LIMIT,
                    scene: &rendered.frame(),
                    prefetch: self.playing,
                },
            );
            let mut encoder =
                surface
                    .gpu
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("grana.egui"),
                    });
            surface.egui.paint(
                &surface.gpu.device,
                &surface.gpu.queue,
                &mut encoder,
                &view,
                size,
                output,
            );
            surface.gpu.queue.submit([encoder.finish()]);
            surface.gpu.queue.present(frame);
            info
        };
        self.frame = info;
        Some(repaint_delay)
    }

    fn capture(&mut self) -> Option<Duration> {
        let Some(surface) = self.surface.as_mut() else {
            self.snapshot = None;
            return None;
        };
        let Some(scene) = self.scene.as_mut() else {
            self.snapshot = None;
            return None;
        };
        let time = scene.time();
        let region = scene.region;
        let exposure = scene.exposure;
        let rendered = scene.rendered();
        let camera = self.viewport.camera();
        let section = self.viewport.section();
        let outcome =
            {
                let job = self.snapshot.as_mut()?;
                job.attempts += 1;
                let exhausted = job.attempts > SNAPSHOT_ATTEMPTS;
                let info = surface.renderer.draw(
                    &surface.gpu.device,
                    &surface.gpu.queue,
                    Frame {
                        target: &job.view,
                        size: job.size,
                        volume_region: [0, 0, job.size[0], job.size[1]],
                        section_region: [0, 0, 1, 1],
                        time,
                        camera,
                        section,
                        region,
                        exposure,
                        samples: SNAPSHOT_SAMPLES,
                        scene: &rendered.frame(),
                        prefetch: false,
                    },
                );
                if info.samples < SNAPSHOT_SAMPLES {
                    if exhausted {
                        self.message = Some(Message::error(
                            "the snapshot could not resolve the volume being streamed",
                        ));
                        self.snapshot = None;
                    }
                    None
                } else {
                    let mut encoder = surface.gpu.device.create_command_encoder(
                        &wgpu::CommandEncoderDescriptor {
                            label: Some("grana.snapshot.figure"),
                        },
                    );
                    let legends = grana_ui::legends(scene);
                    let pane = self.viewport.volume_rect();
                    let world_per_point = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan()
                        / pane.height().max(1.0);
                    surface.figure.paint(
                        &surface.gpu.device,
                        &surface.gpu.queue,
                        &mut encoder,
                        FigureTarget {
                            texture: &job.view,
                            size: job.size,
                        },
                        Caption {
                            world_per_point,
                            pixels_per_point: job.size[0] as f32 / pane.width().max(1.0),
                            legends: &legends,
                        },
                    );
                    surface.gpu.queue.submit([encoder.finish()]);
                    let image = grana_render::read_image(
                        &surface.gpu.device,
                        &surface.gpu.queue,
                        &job.texture,
                        job.size,
                        surface.gpu.format(),
                    );
                    Some((job.path.clone(), image))
                }
            };
        if let Some((path, image)) = outcome {
            self.snapshot = None;
            if let Err(detail) = snapshot::write(&path, &image) {
                self.message = Some(Message::error(detail));
            }
        }
        Some(Duration::ZERO)
    }

    fn begin_snapshot(&mut self) {
        if self.movie.is_some() {
            return;
        }
        let (Some(surface), Some(scene)) = (self.surface.as_ref(), self.scene.as_ref()) else {
            return;
        };
        let pixels = surface.window.scale_factor() as f32;
        let pane = physical_region(self.viewport.volume_rect(), pixels, surface.gpu.size());
        let longest = pane[2].max(pane[3]).max(1);
        let scale = SNAPSHOT_SCALE.min((SNAPSHOT_LIMIT / longest).max(1));
        let size = [(pane[2] * scale).max(1), (pane[3] * scale).max(1)];
        let suggested = format!("{}-snapshot.png", pipeline::slug(scene.dataset.name()));
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a snapshot")
            .set_file_name(suggested)
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        let texture = surface.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grana.snapshot"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: surface.gpu.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.snapshot = Some(Snapshot {
            path,
            texture,
            view,
            size,
            attempts: 0,
        });
        self.message = None;
    }

    fn begin_movie(&mut self) {
        if self.movie.is_some() || self.snapshot.is_some() || self.loading.is_some() {
            return;
        }
        if self.animation.timeline.is_empty() {
            self.animation.message = Some("set a keyframe before exporting".to_string());
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let suggested = format!("{}-movie.mp4", pipeline::slug(scene.dataset.name()));
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save a movie")
            .set_file_name(suggested)
            .add_filter("MP4", &["mp4"])
            .save_file()
        else {
            return;
        };
        self.start_movie(path);
    }

    fn start_movie(&mut self, path: PathBuf) {
        if self.movie.is_some() || self.snapshot.is_some() || self.loading.is_some() {
            return;
        }
        if self.animation.timeline.is_empty() {
            self.animation.message = Some("set a keyframe before exporting".to_string());
            return;
        }
        let (Some(surface), Some(scene)) = (self.surface.as_ref(), self.scene.as_ref()) else {
            return;
        };
        let Some((size, volume)) = self.movie_layout() else {
            return;
        };
        let limit = surface.gpu.device.limits().max_texture_dimension_2d;
        if size[0] > limit || size[1] > limit {
            self.animation.message = Some(format!(
                "a {} × {} movie exceeds the {limit} pixel limit of this device",
                size[0], size[1]
            ));
            return;
        }
        let plan = movie::Plan {
            path,
            size,
            volume,
            fps: self.animation.timeline.fps(),
            frames: self.animation.timeline.frames(),
            resume: scene.time(),
            position: self.animation.position,
        };
        match Movie::start(&surface.gpu.device, surface.gpu.format(), plan) {
            Ok(started) => {
                self.movie = Some(started);
                self.animation.stop();
                self.playing = false;
                self.last_advance = None;
                self.animation.message = None;
                self.message = None;
                self.repaint_at = Some(Instant::now());
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
    }

    fn cancel_movie(&mut self) {
        if self.movie.is_none() {
            return;
        }
        self.stop_movie(None);
        self.animation.message = Some("the movie was cancelled".to_string());
    }

    fn stop_movie(&mut self, detail: Option<String>) {
        let Some(movie) = self.movie.take() else {
            return;
        };
        let (resume, position) = movie.resume();
        drop(movie);
        self.restore_time(resume, position);
        if let Some(detail) = detail {
            self.message = Some(Message::error(detail));
        }
        self.repaint_at = Some(Instant::now());
    }

    fn finish_movie(&mut self) {
        let Some(movie) = self.movie.take() else {
            return;
        };
        let (resume, position) = movie.resume();
        match movie.finish() {
            Ok(path) => {
                self.animation.message = Some(format!("saved {}", movie::name(&path)));
                self.message = None;
            }
            Err(detail) => self.message = Some(Message::error(detail)),
        }
        self.restore_time(resume, position);
        self.repaint_at = Some(Instant::now());
    }

    fn restore_time(&mut self, time: u32, position: f32) {
        self.set_time(time);
        self.animation.position = position.clamp(0.0, self.animation.timeline.duration());
    }

    fn record(&mut self, output: egui::FullOutput) -> Option<Duration> {
        let Some(frame) = self.movie.as_ref().map(|movie| movie.frame) else {
            self.present_ui(output);
            return Some(Duration::ZERO);
        };
        let Some(sample) = self.animation.timeline.sample_frame(frame) else {
            self.stop_movie(Some("the timeline ended before the movie did".to_string()));
            self.present_ui(output);
            return Some(Duration::ZERO);
        };
        self.set_time(sample.time);
        let mut camera = Camera::new(35.0);
        camera.restore(sample.camera);
        let mut section = SectionCamera::new(sample.section.axis);
        section.restore(sample.section);
        let mut complete = false;
        let mut failure = None;
        {
            let (Some(surface), Some(scene), Some(movie)) = (
                self.surface.as_mut(),
                self.scene.as_mut(),
                self.movie.as_mut(),
            ) else {
                self.present_ui(output);
                return Some(Duration::ZERO);
            };
            let rendered = scene.rendered();
            let info = surface.renderer.draw(
                &surface.gpu.device,
                &surface.gpu.queue,
                Frame {
                    target: movie.target(),
                    size: movie.size,
                    volume_region: movie.volume,
                    section_region: [0, 0, 1, 1],
                    time: sample.time,
                    camera: &camera,
                    section: &section,
                    region: scene.region,
                    exposure: scene.exposure,
                    samples: MOVIE_SAMPLES,
                    scene: &rendered.frame(),
                    prefetch: false,
                },
            );
            movie.attempts += 1;
            if info.samples >= MOVIE_SAMPLES && info.settled {
                let image = movie.capture(
                    &surface.gpu.device,
                    &surface.gpu.queue,
                    surface.gpu.format(),
                );
                match movie.push(&image) {
                    Ok(()) => complete = movie.complete(),
                    Err(detail) => failure = Some(detail),
                }
            } else if movie.attempts > MOVIE_ATTEMPTS {
                failure = Some("the movie could not resolve the streamed volume".to_string());
            }
        }
        self.present_ui(output);
        if let Some(detail) = failure {
            self.stop_movie(Some(detail));
            return Some(Duration::ZERO);
        }
        if complete {
            self.finish_movie();
        }
        Some(Duration::ZERO)
    }

    fn present_ui(&mut self, output: egui::FullOutput) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(frame) = surface.gpu.acquire() else {
            surface
                .egui
                .discard(&surface.gpu.device, &surface.gpu.queue, output);
            return;
        };
        let size = surface.gpu.size();
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
        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("grana.ui.clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        surface.egui.paint(
            &surface.gpu.device,
            &surface.gpu.queue,
            &mut encoder,
            &view,
            size,
            output,
        );
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
    }

    fn movie_layout(&self) -> Option<([u32; 2], [u32; 4])> {
        let surface = self.surface.as_ref()?;
        let scale = surface.window.scale_factor() as f32;
        let surface_size = surface.gpu.size();
        let pane = physical_region(self.viewport.volume_rect(), scale, surface_size);
        let factor = self.animation.width as f32 / pane[2] as f32;
        let size = [
            even((pane[2] as f32 * factor).round() as u32),
            even((pane[3] as f32 * factor).round() as u32),
        ];
        Some((size, scale_region(pane, [pane[0], pane[1]], factor, size)))
    }

    fn poll_requests(&mut self) {
        let Some(requests) = self
            .surface
            .as_mut()
            .and_then(|surface| surface.renderer.poll_requests(&surface.gpu.device))
        else {
            return;
        };
        if self
            .scene
            .as_ref()
            .is_none_or(|scene| scene.time() != requests.time)
        {
            return;
        }
        self.frame.volume = requests.volume_view;
        self.frame.section = requests.section_view;
        self.service_requests(&requests);
    }

    fn poll_source(&mut self) {
        let mut processed = 0;
        while processed < UPLOAD_BATCH {
            let Some(event) = self.source.as_ref().and_then(Loader::poll) else {
                break;
            };
            processed += 1;
            match event {
                LoaderEvent::Opened { name, pyramid } => {
                    self.open_dataset(Dataset::zarr(name, pyramid));
                }
                LoaderEvent::Brick { request, data } => {
                    self.in_flight.remove(&request);
                    if let Some(surface) = self.surface.as_mut() {
                        surface.renderer.upload(
                            &surface.gpu.device,
                            &surface.gpu.queue,
                            request,
                            &data,
                        );
                    }
                }
                LoaderEvent::ReadFailed { request, detail } => {
                    self.in_flight.remove(&request);
                    self.message = Some(Message::error(detail));
                    self.analysis.clear();
                    self.progress.clear();
                    self.source = None;
                    self.in_flight.clear();
                    self.pending = None;
                    self.playing = false;
                }
                LoaderEvent::Failed(detail) => {
                    self.loading = None;
                    self.message = Some(Message::error(detail));
                    self.analysis.clear();
                    self.progress.clear();
                    self.source = None;
                    self.in_flight.clear();
                    self.pending = None;
                    self.project = None;
                    self.saved = None;
                    self.playing = false;
                }
            }
        }
    }

    fn service_requests(&mut self, requests: &Requests) {
        if self.loading.is_some() {
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let mut stopped = false;
        for request in requests
            .section
            .iter()
            .chain(&requests.volume)
            .chain(&requests.prefetch)
        {
            if self.in_flight.len() >= MAX_IN_FLIGHT {
                break;
            }
            if self.in_flight.contains(request) {
                continue;
            }
            if source.request(*request) {
                self.in_flight.insert(*request);
            } else {
                stopped = true;
                break;
            }
        }
        if stopped {
            self.source = None;
            self.in_flight.clear();
            self.message = Some(Message::error("the dataset reader stopped"));
        }
    }

    fn poll_analysis(&mut self) {
        enum Update {
            Progress(u32, usize, usize),
            Frame(u32, grana_scene::ObjectFrame),
            Tracks(TrackSet),
            Coloc(grana_object::ColocSeries, grana_volume::Region),
            Done,
            Failed(String),
        }
        let mut updates: Vec<(usize, Job, Update)> = Vec::new();
        for (index, analysis) in self.analysis.iter().enumerate() {
            let job = analysis.job();
            loop {
                match analysis.poll() {
                    Poll::Event(Event::Progress { time, done, total }) => {
                        updates.push((index, job, Update::Progress(time, done, total)));
                    }
                    Poll::Event(Event::Frame { time, frame }) => {
                        updates.push((index, job, Update::Frame(time, frame)));
                    }
                    Poll::Event(Event::Tracks(set)) => {
                        updates.push((index, job, Update::Tracks(set)));
                    }
                    Poll::Event(Event::Coloc { series, region }) => {
                        updates.push((index, job, Update::Coloc(*series, region)));
                    }
                    Poll::Event(Event::Done) => {
                        updates.push((index, job, Update::Done));
                        break;
                    }
                    Poll::Event(Event::Failed(detail)) => {
                        updates.push((index, job, Update::Failed(detail)));
                        break;
                    }
                    Poll::Empty => break,
                    Poll::Closed => {
                        updates.push((
                            index,
                            job,
                            Update::Failed("the analysis worker stopped".to_string()),
                        ));
                        break;
                    }
                }
            }
        }
        let mut removed = Vec::new();
        let mut detected = Vec::new();
        for (index, job, update) in updates {
            match update {
                Update::Progress(time, done, total) => {
                    self.record_progress(job, time, done, total);
                }
                Update::Frame(time, frame) => {
                    if let (Job::Detect(set), Some(scene)) = (job, self.scene.as_mut()) {
                        scene.install(set, time, frame);
                    }
                }
                Update::Tracks(tracks) => {
                    if let (Job::Track(set), Some(scene)) = (job, self.scene.as_mut()) {
                        scene.install_tracks(set, tracks);
                    }
                }
                Update::Coloc(series, region) => {
                    if let Some(scene) = self.scene.as_mut()
                        && scene.coloc.pair() == Some(series.pair())
                    {
                        let frames = scene.frame_count();
                        scene.coloc.install(series, region, frames);
                    }
                }
                Update::Done => {
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    if let Job::Detect(set) = job {
                        detected.push(set);
                    }
                }
                Update::Failed(detail) => {
                    self.progress.retain(|progress| progress.job != job);
                    removed.push(index);
                    self.message = Some(Message::error(detail));
                }
            }
        }
        removed.sort_unstable();
        removed.dedup();
        for index in removed.into_iter().rev() {
            self.analysis.remove(index);
        }
        for set in detected {
            self.start_tracks(set);
        }
    }

    fn record_progress(&mut self, job: Job, time: u32, done: usize, total: usize) {
        let progress = Progress {
            job,
            time,
            done,
            total,
        };
        if let Some(existing) = self
            .progress
            .iter_mut()
            .find(|progress| progress.job == job)
        {
            *existing = progress;
        } else {
            self.progress.push(progress);
        }
    }

    fn cancel(&mut self, job: Job) {
        self.analysis.retain(|analysis| analysis.job() != job);
        self.progress.retain(|progress| progress.job != job);
    }
    fn start_detect(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job() == Job::Detect(set))
        {
            return;
        }
        self.cancel(Job::Track(set));
        if let Some(scene) = self.scene.as_mut() {
            scene.clear_tracks(set);
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let channel = object.channel();
        let params = object.params();
        let region = scene.region;
        if scene.dataset.channels()[channel].intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to detect",
            ));
            return;
        }
        if region.enabled()
            && region
                .voxel_range(scene.dataset.pyramid().level(0))
                .is_none()
        {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        self.message = None;
        self.record_progress(Job::Detect(set), 0, 0, 0);
        self.analysis.push(pipeline::detect(
            Job::Detect(set),
            source,
            channel,
            params,
            region,
        ));
    }

    fn start_tracks(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|analysis| analysis.job().set() == Some(set))
        {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.frame_count() < 2 {
            return;
        }
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        if !object.kind().trackable() {
            return;
        }
        if object.detected() == 0 {
            return;
        }
        let mut detections = vec![Vec::new(); scene.frame_count() as usize];
        for (time, frame) in object.frames() {
            detections[time as usize] = frame.centroids();
        }
        let params = object
            .track_style()
            .expect("a trackable object carries track parameters")
            .params();
        let frame_interval = scene.dataset.frame_interval();
        self.message = None;
        self.record_progress(Job::Track(set), 0, 0, detections.len());
        self.analysis.push(pipeline::track(
            Job::Track(set),
            detections,
            params,
            frame_interval,
        ));
    }

    fn start_coloc(&mut self) {
        if self.loading.is_some()
            || self
                .analysis
                .iter()
                .any(|analysis| analysis.job() == Job::Coloc)
        {
            return;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return;
        };
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(pair) = scene.coloc.pair() else {
            self.message = Some(Message::error(
                "the dataset needs two channels to colocalize",
            ));
            return;
        };
        if [pair.a(), pair.b()]
            .iter()
            .any(|channel| scene.dataset.channels()[*channel].intensity().span() <= 0.0)
        {
            self.message = Some(Message::error(
                "the selected channels have no intensity range to colocalize",
            ));
            return;
        }
        let region = scene.region;
        if region.enabled()
            && region
                .voxel_range(scene.dataset.pyramid().level(0))
                .is_none()
        {
            self.message = Some(Message::error("the region does not overlap the dataset"));
            return;
        }
        self.message = None;
        self.record_progress(Job::Coloc, 0, 0, 0);
        self.analysis
            .push(pipeline::coloc(Job::Coloc, source, pair, region));
    }

    fn export_coloc(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(series) = scene.coloc.series() else {
            return;
        };
        let Some(pair) = scene.coloc.pair() else {
            return;
        };
        let csv = series.csv(scene.coloc.thresholds());
        let file_name = format!(
            "{}-{}-{}-colocalization.csv",
            pipeline::slug(scene.dataset.name()),
            pipeline::slug(scene.dataset.channels()[pair.a()].name()),
            pipeline::slug(scene.dataset.channels()[pair.b()].name())
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export colocalization statistics")
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, csv) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export colocalization to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    fn capture_protocol(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        match Protocol::capture(scene) {
            Ok(protocol) => {
                self.protocol_text = Some(protocol.summary());
                self.protocol = Some(protocol);
                self.reset_batch_rows();
                self.batch_message = None;
            }
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn load_protocol(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Open an analysis protocol")
            .add_filter("grana protocol", &["json"])
            .pick_file()
        else {
            return;
        };
        match Protocol::read(&path) {
            Ok(protocol) => {
                self.protocol_text = Some(protocol.summary());
                self.protocol = Some(protocol);
                self.reset_batch_rows();
                self.batch_message = None;
            }
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn save_protocol(&mut self) {
        if self.protocol.is_none() {
            self.capture_protocol();
        }
        let Some(protocol) = self.protocol.as_ref() else {
            return;
        };
        let suggested = format!(
            "{}-protocol.json",
            pipeline::slug(
                self.scene
                    .as_ref()
                    .map_or("grana", |scene| scene.dataset.name())
            )
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save the analysis protocol")
            .set_file_name(suggested)
            .add_filter("grana protocol", &["json"])
            .save_file()
        else {
            return;
        };
        match protocol.write(&path) {
            Ok(()) => self.batch_message = None,
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn add_batch_datasets(&mut self) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("Choose a folder of OME-Zarr datasets")
            .pick_folder()
        else {
            return;
        };
        match pipeline::discover(&root) {
            Ok(found) => {
                let mut added = 0;
                for path in found {
                    if !self.batch_datasets.contains(&path) {
                        self.batch_datasets.push(path);
                        added += 1;
                    }
                }
                if added == 0 {
                    self.batch_message =
                        Some("the folder holds no new OME-Zarr dataset".to_string());
                } else {
                    self.batch_message = None;
                }
                self.reset_batch_rows();
            }
            Err(error) => self.batch_message = Some(error.to_string()),
        }
    }

    fn choose_batch_output(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a folder for the batch statistics")
            .pick_folder()
        else {
            return;
        };
        self.batch_output = Some(path);
        self.batch_message = None;
    }

    fn run_batch(&mut self) {
        if self.batch_running {
            return;
        }
        let Some(protocol) = self.protocol.clone() else {
            self.batch_message = Some("capture or load a protocol first".to_string());
            return;
        };
        if self.batch_datasets.is_empty() {
            self.batch_message = Some("add a folder of datasets first".to_string());
            return;
        }
        let Some(output) = self.batch_output.clone() else {
            self.batch_message = Some("choose an output folder first".to_string());
            return;
        };
        self.reset_batch_rows();
        self.batch_running = true;
        self.batch_message = None;
        self.batch = Some(pipeline::Batch::start(Plan {
            protocol,
            datasets: self.batch_datasets.clone(),
            output,
        }));
        self.repaint_at = Some(Instant::now());
    }

    fn reset_batch_rows(&mut self) {
        self.batch_rows = self
            .batch_datasets
            .iter()
            .map(|path| BatchDataset {
                name: pipeline::dataset_name(path),
                state: "queued".to_string(),
                detail: String::new(),
            })
            .collect();
    }

    fn poll_batch(&mut self) {
        let Some(batch) = self.batch.take() else {
            return;
        };
        let mut finished = false;
        let mut fatal = false;
        loop {
            let event = match batch.poll() {
                pipeline::BatchPoll::Event(event) => event,
                pipeline::BatchPoll::Empty => break,
                pipeline::BatchPoll::Closed => {
                    if !finished {
                        self.batch_running = false;
                        self.batch_message = Some("the batch worker stopped".to_string());
                    }
                    return;
                }
            };
            match event {
                BatchEvent::Started { index, name } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        row.name = name;
                        row.state = "running".to_string();
                    }
                }
                BatchEvent::Step { index, label } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        row.state = label;
                    }
                }
                BatchEvent::Progress { index, done, total } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        row.detail = format!("{done}/{total}");
                    }
                }
                BatchEvent::Dataset { index, outcome } => {
                    if let Some(row) = self.batch_rows.get_mut(index) {
                        match outcome {
                            Outcome::Complete { objects, .. } => {
                                row.state = "done".to_string();
                                row.detail = format!("{objects} objects");
                            }
                            Outcome::Failed { detail } => {
                                row.state = "failed".to_string();
                                row.detail = detail;
                            }
                        }
                    }
                }
                BatchEvent::Failed(detail) => {
                    fatal = true;
                    self.batch_message = Some(detail);
                }
                BatchEvent::Done { cancelled } => {
                    finished = true;
                    self.batch_running = false;
                    if !fatal {
                        self.batch_message =
                            cancelled.then(|| "the batch was cancelled".to_string());
                    }
                }
            }
        }
        if !finished {
            self.batch = Some(batch);
        }
    }

    fn remove_object(&mut self, set: ObjectSetId) {
        self.cancel(Job::Detect(set));
        self.cancel(Job::Track(set));
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_object(set);
        }
    }

    fn export_measure(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(set) = self.measure.set() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some(csv) = self.measure.csv() else {
            return;
        };
        let channel = scene.dataset.channels()[object.channel()].name();
        let group = match self.measure.group() {
            grana_measure::Group::Object => "objects",
            grana_measure::Group::Track => "tracks",
            grana_measure::Group::Timepoint => "timepoints",
        };
        let file_name = format!(
            "{}-{}-{}-{group}.csv",
            pipeline::slug(scene.dataset.name()),
            pipeline::slug(channel),
            pipeline::slug(object.name())
        );
        let title = format!("Export {} statistics", object.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, csv) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export statistics to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    fn begin_load(&mut self, path: PathBuf) {
        if self.loading.is_some() {
            return;
        }
        self.loading = Some(file_name(&path));
        self.source = Some(Loader::file(path));
        self.pending = None;
        self.project = None;
        self.saved = None;
        self.reset_session();
    }

    fn begin_project(&mut self, path: PathBuf) {
        if self.loading.is_some() {
            return;
        }
        let project = match Project::read(&path) {
            Ok(project) => project,
            Err(error) => {
                self.message = Some(Message::error(error.to_string()));
                return;
            }
        };
        self.message = None;
        enum Target {
            Synthetic(String),
            Zarr(PathBuf),
        }
        let target = match &project.source {
            ProjectSource::Synthetic { name } => Target::Synthetic(name.clone()),
            ProjectSource::Zarr { path, .. } => Target::Zarr(path.clone()),
        };
        match target {
            Target::Synthetic(name) => {
                let (source, pyramid) = Loader::memory(demo::movie());
                self.source = Some(source);
                self.loading = None;
                self.pending = Some(Pending { path, project });
                self.project = None;
                self.saved = None;
                self.reset_session();
                self.open_dataset(Dataset::synthetic(name, pyramid));
            }
            Target::Zarr(data) => {
                if !data.is_dir() {
                    self.message = Some(Message::error(format!(
                        "{} is not an OME-Zarr folder",
                        data.display()
                    )));
                    return;
                }
                self.source = Some(Loader::file(data.clone()));
                self.loading = Some(file_name(&data));
                self.pending = Some(Pending { path, project });
                self.project = None;
                self.saved = None;
                self.reset_session();
            }
        }
    }

    fn reset_session(&mut self) {
        self.measure.reset();
        self.analysis.clear();
        self.progress.clear();
        self.message = None;
        self.in_flight.clear();
        self.snapshot = None;
        self.movie = None;
        self.animation.stop();
        self.animation.message = None;
        self.frame = FrameInfo::default();
        self.repaint_at = Some(Instant::now());
        self.playing = false;
        self.last_advance = None;
    }

    fn open_dataset(&mut self, dataset: Dataset) {
        if let Some(surface) = self.surface.as_mut() {
            surface
                .renderer
                .set_dataset(&surface.gpu.device, dataset.pyramid());
        }
        let mut scene = Scene::new(dataset);
        self.loading = None;
        self.message = None;
        self.analysis.clear();
        self.progress.clear();
        self.measure.reset();
        self.in_flight.clear();
        self.frame = FrameInfo::default();
        self.playing = false;
        self.last_advance = None;
        self.needs_fit = true;
        self.fit_section = false;
        self.animation
            .reset(scene.dataset.frame_count(), scene.dataset.frame_interval());
        if let Some(pending) = self.pending.take() {
            if pending.project.matches(&scene.dataset) {
                scene.restore(pending.project.scene);
                self.measure.restore(pending.project.measure, &scene);
                self.apply_view(pending.project.view);
                self.project = Some(pending.path);
                self.needs_fit = false;
                self.saved = Some(
                    scene.document_hash()
                        ^ self.measure.document_hash()
                        ^ self.animation.timeline.document(),
                );
            } else {
                self.message = Some(Message::error(
                    "the project was saved from a dataset with a different shape, channels or duration",
                ));
            }
        }
        if let Some(source) = &self.source {
            source.set_time(scene.time());
        }
        self.scene = Some(scene);
        self.repaint_at = Some(Instant::now());
    }

    fn save_project(&mut self, choose: bool) {
        if self.loading.is_some() {
            return;
        }
        let (Some(scene), Some(source)) = (self.scene.as_ref(), self.source_ref()) else {
            return;
        };
        let suggested = format!("{}.grana", pipeline::slug(scene.dataset.name()));
        let path = match (choose, self.project.clone()) {
            (false, Some(path)) => path,
            _ => match rfd::FileDialog::new()
                .set_title("Save a grana project")
                .set_file_name(suggested)
                .add_filter("grana project", &["grana"])
                .save_file()
            {
                Some(path) => path,
                None => return,
            },
        };
        let project = Project::capture(scene, &self.measure, source, self.view_state());
        let hash = self.document_hash();
        match project.write(&path) {
            Ok(()) => {
                self.project = Some(path);
                self.saved = hash;
                self.message = None;
            }
            Err(error) => self.message = Some(Message::error(error.to_string())),
        }
    }

    fn source_ref(&self) -> Option<ProjectSource> {
        let scene = self.scene.as_ref()?;
        let name = scene.dataset.name().to_string();
        match scene.dataset.origin() {
            Origin::Synthetic => Some(ProjectSource::Synthetic { name }),
            Origin::Zarr => match self.source.as_ref()?.source() {
                pipeline::Source::File(path) => Some(ProjectSource::Zarr { name, path }),
                pipeline::Source::Memory(_) => None,
            },
        }
    }

    fn view_state(&self) -> ViewState {
        ViewState {
            camera: self.viewport.camera_state(),
            section: self.viewport.section_state(),
            split: self.viewport.split(),
            timeline: self.animation.timeline.clone(),
        }
    }

    fn apply_view(&mut self, view: ViewState) {
        let ViewState {
            camera,
            section,
            split,
            timeline,
        } = view;
        self.viewport.restore_view(camera, section);
        self.viewport.set_split(split);
        self.animation.timeline = timeline;
        self.animation.selected = None;
        self.animation.position = 0.0;
    }

    fn document_hash(&self) -> Option<u64> {
        let scene = self.scene.as_ref()?;
        Some(
            scene.document_hash()
                ^ self.measure.document_hash()
                ^ self.animation.timeline.document(),
        )
    }

    fn dirty(&self) -> bool {
        self.project.is_some() && self.document_hash() != self.saved
    }

    fn sync_title(&mut self) {
        let name = self
            .project
            .as_ref()
            .map(|path| file_name(path))
            .unwrap_or_else(|| "untitled".to_string());
        let marker = if self.dirty() { " •" } else { "" };
        let title = format!("grana — {name}{marker}");
        if self.title.as_deref() == Some(title.as_str()) {
            return;
        }
        self.title = Some(title.clone());
        if let Some(surface) = &self.surface {
            surface.window.set_title(&title);
        }
    }

    fn pick(&mut self, pane: Pane, point: Vec2) {
        let Some(scene) = self.scene.as_mut() else {
            return;
        };
        let ndc = self.viewport.ndc(pane, point);
        let aspect = self.viewport.aspect(pane);
        let selection = match pane {
            Pane::Volume => {
                let ray = self.viewport.camera().ray(ndc, aspect);
                scene.pick_ray(&ray)
            }
            Pane::Section => {
                let section = self.viewport.section();
                let axis = section.axis();
                let focus = self.viewport.camera().focus();
                let mut target = section.point(focus, ndc, aspect);
                let extent = scene.dataset.extent();
                let world = scene.dataset.world_size()[axis.index()];
                let layers = axis.extent(extent) as f32;
                let layer = ((focus[axis.index()] / world + 0.5) * layers)
                    .floor()
                    .clamp(0.0, layers - 1.0);
                target[axis.index()] = ((layer + 0.5) / layers - 0.5) * world;
                scene.pick_slice(axis, target)
            }
        };
        scene.select(selection);
        if let Some(surface) = &self.surface {
            surface.window.request_redraw();
        }
    }

    fn pick_folder() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open an OME-Zarr dataset")
            .pick_folder()
    }

    fn pick_project() -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Open a grana project")
            .add_filter("grana project", &["grana"])
            .pick_file()
    }

    fn on_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        if self.modifiers.control_key() {
            if let Key::Character(text) = &event.logical_key {
                if text.as_str().eq_ignore_ascii_case("s") {
                    self.save_project(self.modifiers.shift_key());
                } else if text.as_str().eq_ignore_ascii_case("o")
                    && let Some(path) = Self::pick_project()
                {
                    self.begin_project(path);
                }
            }
            self.request_redraw();
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_play(),
            Key::Named(NamedKey::ArrowRight) => self.step_time(1),
            Key::Named(NamedKey::ArrowLeft) => self.step_time(-1),
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
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("OME-Zarr dataset")
        .to_string()
}

fn physical_region(rect: egui::Rect, scale: f32, surface: [u32; 2]) -> [u32; 4] {
    let x = ((rect.left() * scale).round() as i64).clamp(0, surface[0] as i64 - 1) as u32;
    let y = ((rect.top() * scale).round() as i64).clamp(0, surface[1] as i64 - 1) as u32;
    let width = ((rect.width() * scale).round() as u32)
        .min(surface[0] - x)
        .max(1);
    let height = ((rect.height() * scale).round() as u32)
        .min(surface[1] - y)
        .max(1);
    [x, y, width, height]
}

fn even(value: u32) -> u32 {
    (value.max(2) / 2) * 2
}

fn scale_region(region: [u32; 4], origin: [u32; 2], factor: f32, size: [u32; 2]) -> [u32; 4] {
    let x = ((region[0].saturating_sub(origin[0]) as f32 * factor).round() as u32).min(size[0] - 1);
    let y = ((region[1].saturating_sub(origin[1]) as f32 * factor).round() as u32).min(size[1] - 1);
    let width = ((region[2] as f32 * factor).round() as u32).clamp(1, size[0] - x);
    let height = ((region[3] as f32 * factor).round() as u32).clamp(1, size[1] - y);
    [x, y, width, height]
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
        let spacing = self
            .scene
            .as_ref()
            .map_or(Spacing::new(1.0, 1.0, 1.0), |scene| scene.dataset.spacing());
        let (camera_changed, consumed) = {
            let surface = self.surface.as_mut().expect("the surface is alive");
            let response = surface.egui.event(&surface.window, &event);
            (
                self.viewport
                    .on_window_event(&event, response.consumed, spacing),
                response.consumed,
            )
        };
        if camera_changed.camera {
            if let Some(scene) = &self.scene {
                self.viewport.clamp_focus(0.5 * scene.dataset.world_size());
            }
            if let Some(surface) = &self.surface {
                surface.window.request_redraw();
            }
        }
        if let Some((pane, point)) = camera_changed.pick {
            self.pick(pane, point);
        }

        match event {
            WindowEvent::CloseRequested => {
                if !self.dirty() || self.confirm_close() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(surface) = self.surface.as_mut() {
                    surface.gpu.resize(size);
                    surface.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
            }
            WindowEvent::DroppedFile(path) => {
                if path.is_dir() {
                    self.begin_load(path);
                } else if path
                    .extension()
                    .is_some_and(|extension| extension == "grana")
                {
                    self.begin_project(path);
                } else {
                    self.message = Some(Message::error(format!(
                        "{} is neither an OME-Zarr folder nor a grana project",
                        path.display()
                    )));
                }
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } if !consumed => {
                self.on_key(&event);
            }
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.repaint_at.is_none() {
            if self.playing {
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
