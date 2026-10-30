use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::Vec2;
use grana_object::{SpotSet, SurfaceSet};
use grana_render::{BrickRequest, Frame, FrameInfo, Marker, Renderer, SurfaceFrame, SurfaceStyle};
use grana_volume::Spacing;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::analysis::{self, Analysis, Event, Kind, Outcome, Poll, Progress};
use crate::dataset::Dataset;
use crate::demo;
use crate::egui_layer::EguiLayer;
use crate::gpu::Gpu;
use crate::loader::{Event as LoaderEvent, Loader};
use crate::scene::Scene;
use crate::ui::{self, Message, UiState};
use crate::viewport::{Pane, Viewport};

const TEXTURE_BUDGET: u64 = 512 << 20;
const UPLOAD_BATCH: usize = 32;
const MAX_IN_FLIGHT: usize = 64;
const STREAM_POLL: Duration = Duration::from_millis(16);
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
    styles: Vec<SurfaceStyle>,
    surface_dirty: bool,
    volume_requests: Vec<BrickRequest>,
    section_requests: Vec<BrickRequest>,
    in_flight: HashSet<BrickRequest>,
    frame: FrameInfo,
    loading: Option<String>,
    message: Option<Message>,
    repaint_at: Option<Instant>,
    needs_fit: bool,
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
            styles: Vec::new(),
            surface_dirty: true,
            volume_requests: Vec::new(),
            section_requests: Vec::new(),
            in_flight: HashSet::new(),
            frame: FrameInfo::default(),
            loading: None,
            message: None,
            repaint_at: None,
            needs_fit: true,
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
        let volume = demo::volume();
        let (source, pyramid) = Loader::memory(volume);
        let mut scene = Scene::new(Dataset::synthetic("synthetic fluorescence", pyramid));
        demo::stage(&mut scene);
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
        self.start_spots();
        self.start_surfaces(demo::SURFACE_CHANNEL);
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
            if actions.detect_spots {
                self.start_spots();
            }
            if actions.cancel_spots {
                self.cancel(Kind::Spots);
            }
            if actions.clear_spots
                && let Some(scene) = self.scene.as_mut()
            {
                scene.clear_spots();
            }
            if actions.export_spots {
                self.export_spots();
            }
            if actions.detect_surfaces {
                let channel = self.scene.as_ref().expect("the scene is alive").selected;
                self.start_surfaces(channel);
            }
            if actions.cancel_surfaces {
                self.cancel(Kind::Surfaces);
            }
            if actions.clear_surfaces
                && let Some(scene) = self.scene.as_mut()
            {
                scene.clear_surfaces();
                self.surface_dirty = true;
            }
            if actions.export_surfaces {
                self.export_surfaces();
            }
        }
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

    fn streaming(&self) -> bool {
        self.loading.is_some()
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
        scene.surface_styles(&mut self.styles);
        if self.surface_dirty {
            renderer.set_surface(&gpu.device, scene.surfaces().map(SurfaceSet::mesh));
            self.surface_dirty = false;
        }
        let surface_view = (!self.styles.is_empty()).then_some(SurfaceFrame {
            styles: &self.styles,
        });
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
                camera,
                section,
                params: &params,
                markers: &self.markers,
                surface: surface_view,
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
                    self.surface_dirty = true;
                    self.loading = None;
                    self.message = None;
                    self.in_flight.clear();
                    self.volume_requests.clear();
                    self.section_requests.clear();
                    self.frame = FrameInfo::default();
                    self.needs_fit = true;
                }
                LoaderEvent::Brick { request, data } => {
                    self.in_flight.remove(&request);
                    if let Some(surface) = self.surface.as_mut() {
                        surface.renderer.upload(
                            &surface.gpu.device,
                            &surface.gpu.queue,
                            request.level,
                            request.coord,
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
            Progress(usize, usize),
            Finished(Outcome),
            Failed(String),
        }
        let mut updates: Vec<(usize, Kind, usize, Update)> = Vec::new();
        for (index, job) in self.analysis.iter().enumerate() {
            loop {
                match job.poll() {
                    Poll::Event(Event::Started { total })
                    | Poll::Event(Event::Progress { done: 0, total }) => {
                        updates.push((
                            index,
                            job.kind(),
                            job.channel(),
                            Update::Progress(0, total),
                        ));
                    }
                    Poll::Event(Event::Progress { done, total }) => {
                        updates.push((
                            index,
                            job.kind(),
                            job.channel(),
                            Update::Progress(done, total),
                        ));
                    }
                    Poll::Event(Event::Finished(outcome)) => {
                        updates.push((index, job.kind(), job.channel(), Update::Finished(outcome)));
                        break;
                    }
                    Poll::Event(Event::Failed(detail)) => {
                        updates.push((index, job.kind(), job.channel(), Update::Failed(detail)));
                        break;
                    }
                    Poll::Empty => break,
                    Poll::Closed => {
                        updates.push((
                            index,
                            job.kind(),
                            job.channel(),
                            Update::Failed("the analysis worker stopped".to_string()),
                        ));
                        break;
                    }
                }
            }
        }
        let mut finished = Vec::new();
        let mut removed = Vec::new();
        for (index, kind, channel, update) in updates {
            match update {
                Update::Progress(done, total) => {
                    self.record_progress(kind, channel, done, total);
                }
                Update::Finished(outcome) => {
                    self.progress.retain(|progress| progress.kind != kind);
                    removed.push(index);
                    finished.push(outcome);
                }
                Update::Failed(detail) => {
                    self.progress.retain(|progress| progress.kind != kind);
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
        for outcome in finished {
            match outcome {
                Outcome::Spots(set) => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.install_spots(set);
                    }
                }
                Outcome::Surfaces(set) => {
                    if let Some(scene) = self.scene.as_mut() {
                        scene.install_surfaces(set);
                    }
                    self.surface_dirty = true;
                }
            }
        }
    }

    fn record_progress(&mut self, kind: Kind, channel: usize, done: usize, total: usize) {
        let progress = Progress {
            kind,
            channel,
            done,
            total,
        };
        if let Some(existing) = self
            .progress
            .iter_mut()
            .find(|progress| progress.kind == kind)
        {
            *existing = progress;
        } else {
            self.progress.push(progress);
        }
    }

    fn cancel(&mut self, kind: Kind) {
        self.analysis.retain(|job| job.kind() != kind);
        self.progress.retain(|progress| progress.kind != kind);
    }

    fn start_spots(&mut self) {
        if self.loading.is_some() {
            return;
        }
        if self.analysis.iter().any(|job| job.kind() == Kind::Spots) {
            return;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return;
        };
        let (channel, params, span) = {
            let Some(scene) = self.scene.as_ref() else {
                return;
            };
            let channel = scene.selected;
            (
                channel,
                scene.spot_params,
                scene.dataset.channels()[channel].intensity().span(),
            )
        };
        if span <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to detect",
            ));
            return;
        }
        self.message = None;
        self.record_progress(Kind::Spots, channel, 0, 0);
        self.analysis
            .push(analysis::start_spots(source, channel, params));
    }

    fn start_surfaces(&mut self, channel: usize) {
        if self.loading.is_some() {
            return;
        }
        if self.analysis.iter().any(|job| job.kind() == Kind::Surfaces) {
            return;
        }
        let Some(source) = self.source.as_ref().map(Loader::source) else {
            return;
        };
        let (params, span) = {
            let Some(scene) = self.scene.as_ref() else {
                return;
            };
            assert!(
                channel < scene.dataset.channels().len(),
                "the surface channel exists"
            );
            (
                scene.surface_params,
                scene.dataset.channels()[channel].intensity().span(),
            )
        };
        if span <= 0.0 {
            self.message = Some(Message::error(
                "the selected channel has no intensity range to extract",
            ));
            return;
        }
        self.message = None;
        self.record_progress(Kind::Surfaces, channel, 0, 0);
        self.analysis
            .push(analysis::start_surfaces(source, channel, params));
    }

    fn export_spots(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(spots) = scene.spots() else {
            return;
        };
        let channel = scene.dataset.channels()[spots.channel()].name();
        let file_name = format!("{}-{}-spots.csv", slug(scene.dataset.name()), slug(channel));
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export spots")
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, spots_csv(spots)) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export spots to {}: {error}",
                    path.display()
                )));
            }
        }
    }

    fn export_surfaces(&mut self) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let Some(surfaces) = scene.surfaces() else {
            return;
        };
        let channel = scene.dataset.channels()[surfaces.channel()].name();
        let file_name = format!(
            "{}-{}-surfaces.csv",
            slug(scene.dataset.name()),
            slug(channel)
        );
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export surfaces")
            .set_file_name(file_name)
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, surfaces_csv(surfaces)) {
            Ok(()) => self.message = None,
            Err(error) => {
                self.message = Some(Message::error(format!(
                    "failed to export surfaces to {}: {error}",
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
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("OME-Zarr dataset")
        .to_string()
}

fn spots_csv(spots: &SpotSet) -> String {
    let mut csv = String::from("id,x_um,y_um,z_um,diameter_um,quality\n");
    for (index, spot) in spots.spots().iter().enumerate() {
        let center = spot.center();
        csv.push_str(&format!(
            "{},{:.4},{:.4},{:.4},{:.4},{:.4}\n",
            index + 1,
            center[0],
            center[1],
            center[2],
            spot.radius() * 2.0,
            spot.quality()
        ));
    }
    csv
}

fn surfaces_csv(surfaces: &SurfaceSet) -> String {
    let mut csv = String::from(
        "id,voxels,volume_um3,area_um2,sphericity,centroid_x_um,centroid_y_um,centroid_z_um,min_x_um,min_y_um,min_z_um,max_x_um,max_y_um,max_z_um,mean_intensity,min_intensity,max_intensity,intensity_std\n",
    );
    for (index, object) in surfaces.objects().iter().enumerate() {
        let centroid = object.centroid();
        let bounds = object.bounds();
        let min = bounds.min();
        let max = bounds.max();
        let intensity = object.intensity();
        csv.push_str(&format!(
            "{},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.2},{:.2},{:.2},{:.2}\n",
            index + 1,
            object.voxel_count(),
            object.volume(),
            object.area(),
            object.sphericity(),
            centroid[0],
            centroid[1],
            centroid[2],
            min[0],
            min[1],
            min[2],
            max[0],
            max[1],
            max[2],
            intensity.mean(),
            intensity.min(),
            intensity.max(),
            intensity.deviation(),
        ));
    }
    csv
}

fn slug(name: &str) -> String {
    let slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    if slug.is_empty() {
        "dataset".to_string()
    } else {
        slug
    }
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
        let camera_changed = {
            let surface = self.surface.as_mut().expect("the surface is alive");
            let response = surface.egui.event(&surface.window, &event);
            self.viewport
                .on_window_event(&event, response.consumed, spacing)
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
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.streaming() && self.repaint_at.is_none() {
            self.repaint_at = Some(Instant::now() + STREAM_POLL);
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
