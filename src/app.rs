use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use grana_object::TrackSet;
use grana_render::{BrickRequest, Frame, FrameInfo, Marker, MeshFrame, MeshUpload, Renderer};
use grana_volume::Spacing;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::analysis::{self, Analysis, Event, Job, Outcome, Poll, Progress};
use crate::dataset::Dataset;
use crate::demo;
use crate::egui_layer::EguiLayer;
use crate::export;
use crate::gpu::Gpu;
use crate::loader::{Event as LoaderEvent, Loader};
use crate::objects::ObjectSetId;
use crate::scene::Scene;
use crate::ui::{self, Message, UiState};
use crate::viewport::{Pane, Viewport};

const TEXTURE_BUDGET: u64 = 512 << 20;
const UPLOAD_BATCH: usize = 32;
const MAX_IN_FLIGHT: usize = 64;
const STREAM_POLL: Duration = Duration::from_millis(16);
const PLAYBACK_INTERVAL: Duration = Duration::from_millis(90);
const MAX_REPAINT_DELAY: Duration = Duration::from_secs(3600);

struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    egui: EguiLayer,
    renderer: Renderer,
}

pub struct App {
    surface: Option<Surface>,
    viewport: Viewport,
    scene: Option<Scene>,
    source: Option<Loader>,
    analysis: Vec<Analysis>,
    progress: Vec<Progress>,
    markers: Vec<Marker>,
    volume_requests: Vec<BrickRequest>,
    section_requests: Vec<BrickRequest>,
    in_flight: HashSet<BrickRequest>,
    frame: FrameInfo,
    loading: Option<String>,
    message: Option<Message>,
    repaint_at: Option<Instant>,
    needs_fit: bool,
    playing: bool,
    last_advance: Option<Instant>,
}

impl App {
    pub fn new() -> Self {
        Self {
            surface: None,
            viewport: Viewport::new(),
            scene: None,
            source: None,
            analysis: Vec::new(),
            progress: Vec::new(),
            markers: Vec::new(),
            volume_requests: Vec::new(),
            section_requests: Vec::new(),
            in_flight: HashSet::new(),
            frame: FrameInfo::default(),
            loading: None,
            message: None,
            repaint_at: None,
            needs_fit: true,
            playing: false,
            last_advance: None,
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
        let (source, pyramid) = Loader::memory(demo::movie());
        let mut scene = Scene::new(Dataset::synthetic("synthetic fluorescence", pyramid));
        let staged = demo::stage(&mut scene);
        let mut renderer = Renderer::new(&gpu.device, &gpu.queue, gpu.format(), TEXTURE_BUDGET);
        renderer.set_pyramid(&gpu.device, scene.dataset.pyramid());
        self.scene = Some(scene);
        self.source = Some(source);
        self.surface = Some(Surface {
            window,
            gpu,
            egui,
            renderer,
        });
        self.start_detect(staged.spots);
        self.start_detect(staged.surfaces);
    }

    fn redraw(&mut self) {
        self.poll_source();
        self.poll_analysis();
        if self.surface.is_none() || self.scene.is_none() {
            return;
        }
        let (output, actions) = self.run_ui();
        let mut opened = false;
        if actions.open
            && let Some(path) = Self::pick_folder()
        {
            self.begin_load(path);
            opened = true;
        }
        if !opened {
            if actions.toggle_play {
                self.toggle_play();
            }
            if let Some(time) = actions.time {
                self.scrub(time);
            }
            if let Some(set) = actions.remove_object {
                self.remove_object(set);
            }
            if let Some(kind) = actions.add_object {
                let channel = self.scene.as_ref().expect("the scene is alive").selected;
                self.scene
                    .as_mut()
                    .expect("the scene is alive")
                    .add_object(kind, channel);
            }
            if let Some(set) = actions.detect {
                self.start_detect(set);
            }
            if let Some(set) = actions.cancel_detect {
                self.cancel(set, Job::Detect);
            }
            if let Some(set) = actions.clear_object {
                self.cancel(set, Job::Track);
                if let Some(scene) = self.scene.as_mut() {
                    scene.clear_object(set);
                }
            }
            if let Some(set) = actions.export_object {
                self.export_object(set);
            }
            if let Some(set) = actions.track {
                self.start_tracks(set);
            }
            if let Some(set) = actions.cancel_track {
                self.cancel(set, Job::Track);
            }
            if let Some(set) = actions.clear_tracks
                && let Some(scene) = self.scene.as_mut()
            {
                scene.clear_tracks(set);
            }
            if let Some(set) = actions.export_tracks {
                self.export_tracks(set);
            }
        }
        self.advance_playback();
        let scale = self
            .surface
            .as_ref()
            .expect("the surface is alive")
            .window
            .scale_factor() as f32;
        let size = self
            .surface
            .as_ref()
            .expect("the surface is alive")
            .gpu
            .size();
        let volume_region = physical_region(actions.volume, scale, size);
        let section_region = physical_region(actions.section, scale, size);
        self.viewport.set_regions(volume_region, section_region);
        if let Some(split) = actions.split {
            self.viewport.set_split(split);
        }
        let volume_aspect = volume_region[2] as f32 / volume_region[3] as f32;
        let section_aspect = section_region[2] as f32 / section_region[3] as f32;
        let (world, voxel) = {
            let scene = self.scene.as_ref().expect("the scene is alive");
            (scene.dataset.world_size(), scene.dataset.voxel_size())
        };
        if let Some(axis) = actions.section_axis {
            self.viewport.section_mut().set_axis(axis);
            self.viewport.fit_section(world, voxel, section_aspect);
        }
        if actions.fit_section {
            self.viewport.fit_section(world, voxel, section_aspect);
        }
        if self.needs_fit || actions.fit {
            self.viewport
                .fit(world, voxel, volume_aspect, section_aspect);
            self.needs_fit = false;
        }
        let mut delay = self
            .render(output, volume_region, section_region)
            .unwrap_or(Duration::MAX);
        self.service_requests();
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

    fn toggle_play(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        if scene.frame_count() <= 1 {
            return;
        }
        self.playing = !self.playing;
        self.last_advance = self.playing.then(Instant::now);
    }

    fn scrub(&mut self, time: u32) {
        self.playing = false;
        self.last_advance = None;
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
        self.in_flight.clear();
        self.volume_requests.clear();
        self.section_requests.clear();
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
        self.playing
            || self.loading.is_some()
            || !self.analysis.is_empty()
            || !self.in_flight.is_empty()
            || (self.source.is_some()
                && (!self.volume_requests.is_empty() || !self.section_requests.is_empty()))
            || (self.source.is_some()
                && (self.frame.volume.resident < self.frame.volume.visible
                    || self.frame.section.resident < self.frame.section.visible))
    }

    fn run_ui(&mut self) -> (egui::FullOutput, ui::Actions) {
        let surface = self.surface.as_mut().expect("the surface is alive");
        let scene = self.scene.as_mut().expect("the scene is alive");
        let Surface {
            window,
            gpu,
            egui,
            renderer: _,
        } = surface;
        let adapter = gpu.adapter_description.as_str();
        let loading = self.loading.as_deref();
        let message = self.message.as_ref();
        let frame = self.frame;
        let pending = self.in_flight.len();
        let analysis = self.progress.as_slice();
        let playing = self.playing;
        let viewport = &mut self.viewport;
        let mut actions = ui::Actions::new();
        let output = egui.run(window, |root| {
            actions = ui::build(
                root,
                &mut UiState {
                    scene,
                    view: viewport,
                    adapter,
                    loading,
                    message,
                    frame,
                    pending,
                    playing,
                    analysis,
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
        let surface = self.surface.as_mut()?;
        let scene = self.scene.as_ref()?;
        let Surface {
            gpu,
            egui,
            renderer,
            ..
        } = surface;
        let size = gpu.size();
        let repaint_delay = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX);
        let params = scene.params();
        scene.markers(&mut self.markers);
        let layers = scene.layers();
        let uploads = layers
            .iter()
            .map(|layer| MeshUpload {
                key: layer.key,
                mesh: layer.mesh,
            })
            .collect::<Vec<_>>();
        renderer.set_meshes(&gpu.device, &uploads);
        let frames = layers
            .iter()
            .map(|layer| MeshFrame {
                styles: &layer.styles,
            })
            .collect::<Vec<_>>();
        let frame = gpu.acquire()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.frame"),
            });
        self.volume_requests.clear();
        self.section_requests.clear();
        let camera = self.viewport.camera();
        let section = self.viewport.section();
        self.frame = renderer.draw(
            &gpu.device,
            &gpu.queue,
            Frame {
                encoder: &mut encoder,
                target: &view,
                size,
                volume_region,
                section_region,
                time: scene.time(),
                camera,
                section,
                params: &params,
                markers: &self.markers,
                meshes: &frames,
                volume_requests: &mut self.volume_requests,
                section_requests: &mut self.section_requests,
            },
        );
        egui.paint(&gpu.device, &gpu.queue, &mut encoder, &view, size, output);
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
        Some(repaint_delay)
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
                    let dataset = Dataset::zarr(name, pyramid);
                    if let Some(surface) = self.surface.as_mut() {
                        surface
                            .renderer
                            .set_pyramid(&surface.gpu.device, dataset.pyramid());
                    }
                    self.scene = Some(Scene::new(dataset));
                    self.analysis.clear();
                    self.progress.clear();
                    self.loading = None;
                    self.message = None;
                    self.in_flight.clear();
                    self.volume_requests.clear();
                    self.section_requests.clear();
                    self.frame = FrameInfo::default();
                    self.needs_fit = true;
                    self.playing = false;
                    self.last_advance = None;
                    if let Some(source) = &self.source {
                        source.set_time(0);
                    }
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
                    self.volume_requests.clear();
                    self.section_requests.clear();
                    self.playing = false;
                }
                LoaderEvent::Failed(detail) => {
                    self.loading = None;
                    self.message = Some(Message::error(detail));
                    self.analysis.clear();
                    self.progress.clear();
                    self.source = None;
                    self.in_flight.clear();
                    self.volume_requests.clear();
                    self.section_requests.clear();
                    self.playing = false;
                }
            }
        }
    }

    fn service_requests(&mut self) {
        if self.loading.is_some() {
            return;
        }
        let Some(source) = self.source.as_ref() else {
            return;
        };
        let mut stopped = false;
        for request in self
            .section_requests
            .iter()
            .chain(self.volume_requests.iter())
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
            Frame(u32, Outcome),
            Tracks(TrackSet),
            Done,
            Failed(String),
        }
        let mut updates: Vec<(usize, ObjectSetId, Job, Update)> = Vec::new();
        for (index, job) in self.analysis.iter().enumerate() {
            loop {
                match job.poll() {
                    Poll::Event(Event::Progress { time, done, total }) => {
                        updates.push((
                            index,
                            job.set(),
                            job.job(),
                            Update::Progress(time, done, total),
                        ));
                    }
                    Poll::Event(Event::Frame { time, outcome }) => {
                        updates.push((index, job.set(), job.job(), Update::Frame(time, outcome)));
                    }
                    Poll::Event(Event::Tracks(set)) => {
                        updates.push((index, job.set(), job.job(), Update::Tracks(set)));
                    }
                    Poll::Event(Event::Done) => {
                        updates.push((index, job.set(), job.job(), Update::Done));
                        break;
                    }
                    Poll::Event(Event::Failed(detail)) => {
                        updates.push((index, job.set(), job.job(), Update::Failed(detail)));
                        break;
                    }
                    Poll::Empty => break,
                    Poll::Closed => {
                        updates.push((
                            index,
                            job.set(),
                            job.job(),
                            Update::Failed("the analysis worker stopped".to_string()),
                        ));
                        break;
                    }
                }
            }
        }
        let mut removed = Vec::new();
        let mut detected = Vec::new();
        for (index, set, job, update) in updates {
            match update {
                Update::Progress(time, done, total) => {
                    self.record_progress(set, job, time, done, total);
                }
                Update::Frame(time, outcome) => {
                    if let Some(scene) = self.scene.as_mut() {
                        let frame = match outcome {
                            Outcome::Spots(spots) => crate::objects::ObjectFrame::Spots(spots),
                            Outcome::Surfaces(surfaces) => {
                                crate::objects::ObjectFrame::Surfaces(surfaces)
                            }
                        };
                        scene.install(set, time, frame);
                    }
                }
                Update::Tracks(tracks) => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.install_tracks(set, tracks);
                    }
                }
                Update::Done => {
                    self.progress
                        .retain(|progress| progress.set != set || progress.job != job);
                    removed.push(index);
                    if job == Job::Detect {
                        detected.push(set);
                    }
                }
                Update::Failed(detail) => {
                    self.progress
                        .retain(|progress| progress.set != set || progress.job != job);
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

    fn record_progress(
        &mut self,
        set: ObjectSetId,
        job: Job,
        time: u32,
        done: usize,
        total: usize,
    ) {
        let progress = Progress {
            set,
            job,
            time,
            done,
            total,
        };
        if let Some(existing) = self
            .progress
            .iter_mut()
            .find(|progress| progress.set == set && progress.job == job)
        {
            *existing = progress;
        } else {
            self.progress.push(progress);
        }
    }

    fn cancel(&mut self, set: ObjectSetId, job: Job) {
        self.analysis
            .retain(|analysis| analysis.set() != set || analysis.job() != job);
        self.progress
            .retain(|progress| progress.set != set || progress.job != job);
    }

    fn start_detect(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self
            .analysis
            .iter()
            .any(|job| job.set() == set && job.job() == Job::Detect)
        {
            return;
        }
        self.cancel(set, Job::Track);
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
        if scene.dataset.channels()[channel].intensity().span() <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to detect",
            ));
            return;
        }
        self.message = None;
        self.record_progress(set, Job::Detect, 0, 0, 0);
        self.analysis
            .push(analysis::detect(source, set, channel, params));
    }

    fn start_tracks(&mut self, set: ObjectSetId) {
        if self.loading.is_some() {
            return;
        }
        if self.analysis.iter().any(|job| job.set() == set) {
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
        if object.detected() == 0 {
            return;
        }
        let mut detections = vec![Vec::new(); scene.frame_count() as usize];
        for (time, frame) in object.frames() {
            detections[time as usize] = frame.centroids();
        }
        let params = object.track_params();
        let frame_interval = scene.dataset.frame_interval();
        self.message = None;
        self.record_progress(set, Job::Track, 0, 0, detections.len());
        self.analysis
            .push(analysis::track(set, detections, params, frame_interval));
    }

    fn remove_object(&mut self, set: ObjectSetId) {
        self.cancel(set, Job::Detect);
        self.cancel(set, Job::Track);
        if let Some(scene) = self.scene.as_mut() {
            scene.remove_object(set);
        }
    }

    fn export_object(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some((_, first)) = object.frames().next() else {
            return;
        };
        let channel = scene.dataset.channels()[object.channel()].name();
        let file_name = format!(
            "{}-{}-{}.csv",
            export::slug(scene.dataset.name()),
            export::slug(channel),
            export::slug(object.name())
        );
        let title = format!("Export {}", object.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        let csv = match first {
            crate::objects::ObjectFrame::Spots(_) => {
                export::spots(scene.dataset.frame_interval(), object)
            }
            crate::objects::ObjectFrame::Surfaces(_) => {
                export::surfaces(scene.dataset.frame_interval(), object)
            }
        };
        match std::fs::write(&path, csv) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export objects to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    fn export_tracks(&mut self, set: ObjectSetId) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(object) = scene.objects.get(set) else {
            return;
        };
        let Some(layer) = object.tracks() else {
            return;
        };
        let channel = scene.dataset.channels()[object.channel()].name();
        let file_name = format!(
            "{}-{}-{}-tracks.csv",
            export::slug(scene.dataset.name()),
            export::slug(channel),
            export::slug(object.name())
        );
        let title = format!("Export {} tracks", object.name());
        let Some(path) = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, export::tracks(layer.set())) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export tracks to {}: {error}",
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
        self.analysis.clear();
        self.progress.clear();
        self.message = None;
        self.in_flight.clear();
        self.volume_requests.clear();
        self.section_requests.clear();
        self.frame = FrameInfo::default();
        self.repaint_at = Some(Instant::now());
        self.playing = false;
        self.last_advance = None;
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

    fn on_key(&mut self, event: &winit::event::KeyEvent) {
        if event.state != ElementState::Pressed || event.repeat {
            return;
        }
        match event.logical_key {
            Key::Named(NamedKey::Space) => self.toggle_play(),
            Key::Named(NamedKey::ArrowRight) => self.step_time(1),
            Key::Named(NamedKey::ArrowLeft) => self.step_time(-1),
            _ => return,
        }
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
            WindowEvent::CloseRequested => event_loop.exit(),
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
                } else {
                    self.message = Some(Message::error(format!(
                        "{} is not an OME-Zarr folder",
                        path.display()
                    )));
                }
                if let Some(surface) = &self.surface {
                    surface.window.request_redraw();
                }
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
