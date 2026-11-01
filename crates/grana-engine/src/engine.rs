use std::path::{Path, PathBuf};

use grana_display::{Camera, CameraState, SectionCamera, SectionState, Viewport};
use grana_gpu::Gpu;
use grana_render::{Frame, FrameInfo, Image, Renderer, Requests};
use grana_scene::Scene;
use grana_ui::Ui;
use grana_volume::{BrickData, BrickRequest, Pyramid};
use wgpu::{TextureView, TextureViewDescriptor};

use crate::movie::{self, Movie, Plan};
use crate::snapshot;
use crate::view;

const TEXTURE_BUDGET: u64 = 512 << 20;
const SNAPSHOT_SAMPLES: u32 = 8;
const SNAPSHOT_ATTEMPTS: u32 = 512;
pub const SAMPLE_LIMIT: u32 = 32;
pub const MOVIE_SAMPLES: u32 = 16;
const MOVIE_ATTEMPTS: u32 = 4096;

struct Snapshot {
    path: PathBuf,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    attempts: u32,
}

pub struct View<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub samples: u32,
    pub prefetch: bool,
}

pub struct ViewportFrame<'a> {
    pub target: &'a TextureView,
    pub scene: &'a Scene,
    pub viewport: &'a Viewport,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub prefetch: bool,
}

pub struct ProjectedView {
    pub camera: CameraState,
    pub section: SectionState,
    pub aspect: f32,
    pub world_per_point: f32,
    pub pixels_per_point: f32,
}

pub struct Pose {
    pub time: u32,
    pub position: f32,
    pub camera: CameraState,
    pub section: SectionState,
}

pub enum Recording {
    Running,
    Complete { path: PathBuf },
    Failed { detail: String },
}

pub struct Engine {
    renderer: Renderer,
    snapshot: Option<Snapshot>,
    movie: Option<Movie>,
    message: Option<String>,
    movie_message: Option<String>,
}

impl Engine {
    pub fn new(gpu: &Gpu) -> Self {
        Self {
            renderer: Renderer::new(&gpu.device, &gpu.queue, gpu.format(), TEXTURE_BUDGET),
            snapshot: None,
            movie: None,
            message: None,
            movie_message: None,
        }
    }

    pub fn set_dataset(&mut self, gpu: &Gpu, pyramid: &Pyramid) {
        self.renderer.set_dataset(&gpu.device, pyramid);
    }

    pub fn requests_pending(&self) -> bool {
        self.renderer.requests_pending()
    }

    pub fn poll_requests(&mut self, gpu: &Gpu) -> Option<Requests> {
        self.renderer.poll_requests(&gpu.device)
    }

    pub fn upload(&mut self, gpu: &Gpu, request: BrickRequest, data: &BrickData) {
        self.renderer.upload(&gpu.device, &gpu.queue, request, data);
    }

    pub fn movie(&self) -> Option<(u32, u32)> {
        self.movie.as_ref().map(|movie| (movie.frame, movie.frames))
    }

    pub fn movie_message(&self) -> Option<String> {
        self.movie_message.clone()
    }

    pub fn set_movie_message(&mut self, message: Option<String>) {
        self.movie_message = message;
    }

    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    pub fn recording(&self) -> bool {
        self.movie.is_some()
    }

    pub fn snapshot_pending(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn draw_viewport(&mut self, gpu: &Gpu, frame: ViewportFrame<'_>) -> FrameInfo {
        draw(
            &mut self.renderer,
            gpu,
            View {
                target: frame.target,
                scene: frame.scene,
                camera: frame.viewport.camera(),
                section: frame.viewport.section(),
                size: frame.size,
                volume_region: frame.volume_region,
                section_region: frame.section_region,
                samples: SAMPLE_LIMIT,
                prefetch: frame.prefetch,
            },
        )
    }

    pub fn begin_snapshot(&mut self, gpu: &Gpu, path: PathBuf, size: [u32; 2]) {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grana.snapshot"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.format(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.snapshot = Some(Snapshot {
            path,
            texture,
            view,
            size,
            attempts: 0,
        });
        self.message = None;
    }

    pub fn capture(
        &mut self,
        gpu: &Gpu,
        ui: &mut Ui,
        scene: &Scene,
        projected: &ProjectedView,
    ) -> bool {
        let Some(job) = self.snapshot.as_mut() else {
            return false;
        };
        job.attempts += 1;
        let exhausted = job.attempts > SNAPSHOT_ATTEMPTS;
        let camera = restored_camera(&projected.camera);
        let section = restored_section(&projected.section);
        let size = job.size;
        let target = job.view.clone();
        let info = draw(
            &mut self.renderer,
            gpu,
            View {
                target: &target,
                scene,
                camera: &camera,
                section: &section,
                size,
                volume_region: [0, 0, size[0], size[1]],
                section_region: [0, 0, 1, 1],
                samples: SNAPSHOT_SAMPLES,
                prefetch: false,
            },
        );
        if info.samples < SNAPSHOT_SAMPLES {
            if exhausted {
                self.snapshot = None;
                self.message =
                    Some("the snapshot could not resolve the volume being streamed".to_string());
            }
            return false;
        }
        let job = self.snapshot.as_ref().expect("the snapshot is alive");
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.snapshot.figure"),
            });
        let legends = grana_figure::legends(scene);
        let instances = grana_figure::instances(
            ui,
            size,
            projected.pixels_per_point,
            projected.world_per_point,
            &legends,
        );
        ui.renderer().paint_figure(
            &mut encoder,
            &job.view,
            size,
            projected.pixels_per_point,
            ui.atlas(),
            &instances,
        );
        gpu.queue.submit([encoder.finish()]);
        let image: Image =
            grana_render::read_image(&gpu.device, &gpu.queue, &job.texture, size, gpu.format());
        let path = job.path.clone();
        self.snapshot = None;
        self.message = Some(match snapshot::write(&path, &image) {
            Ok(()) => format!("saved {}", path.display()),
            Err(detail) => detail,
        });
        true
    }

    pub fn start_movie(&mut self, gpu: &Gpu, plan: Plan) -> Result<(), String> {
        match Movie::start(&gpu.device, gpu.format(), plan) {
            Ok(started) => {
                self.movie = Some(started);
                self.movie_message = None;
                Ok(())
            }
            Err(detail) => Err(detail),
        }
    }

    pub fn record(&mut self, gpu: &Gpu, scene: &Scene, pose: &Pose) -> Recording {
        let camera = restored_camera(&pose.camera);
        let section = restored_section(&pose.section);
        let Some(movie) = self.movie.as_mut() else {
            return Recording::Failed {
                detail: "the movie disappeared".to_string(),
            };
        };
        let info = draw(
            &mut self.renderer,
            gpu,
            View {
                target: movie.target(),
                scene,
                camera: &camera,
                section: &section,
                size: movie.size,
                volume_region: movie.volume,
                section_region: [0, 0, 1, 1],
                samples: MOVIE_SAMPLES,
                prefetch: false,
            },
        );
        movie.attempts += 1;
        if info.samples < MOVIE_SAMPLES || !info.settled {
            if movie.attempts > MOVIE_ATTEMPTS {
                self.movie = None;
                return Recording::Failed {
                    detail: "the movie could not resolve the streamed volume".to_string(),
                };
            }
            return Recording::Running;
        }
        let image: Image = movie.capture(&gpu.device, &gpu.queue, gpu.format());
        if let Err(detail) = movie.push(&image) {
            self.movie = None;
            return Recording::Failed { detail };
        }
        if !movie.complete() {
            return Recording::Running;
        }
        let movie = self.movie.take().expect("the movie is alive");
        match movie.finish() {
            Ok(path) => Recording::Complete { path },
            Err(detail) => Recording::Failed { detail },
        }
    }

    pub fn resume(&self) -> Option<(u32, f32)> {
        self.movie.as_ref().map(|movie| movie.resume())
    }

    pub fn cancel_movie(&mut self) -> Option<(u32, f32)> {
        let movie = self.movie.take()?;
        self.movie_message = Some("the movie was cancelled".to_string());
        Some(movie.resume())
    }
}

fn draw(renderer: &mut Renderer, gpu: &Gpu, frame: View<'_>) -> FrameInfo {
    let rendered = view::frame(frame.scene);
    renderer.draw(
        &gpu.device,
        &gpu.queue,
        Frame {
            target: frame.target,
            size: frame.size,
            volume_region: frame.volume_region,
            section_region: frame.section_region,
            time: frame.scene.time(),
            camera: frame.camera,
            section: frame.section,
            region: frame.scene.region,
            exposure: frame.scene.exposure,
            samples: frame.samples,
            scene: &rendered.frame(),
            prefetch: frame.prefetch,
        },
    )
}

fn restored_camera(state: &CameraState) -> Camera {
    let mut camera = Camera::new(35.0);
    camera.restore(*state);
    camera
}

fn restored_section(state: &SectionState) -> SectionCamera {
    let mut section = SectionCamera::new(state.axis);
    section.restore(*state);
    section
}

pub fn name(path: &Path) -> String {
    movie::name(path)
}
