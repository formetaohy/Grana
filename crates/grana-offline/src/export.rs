use std::path::PathBuf;

use grana_display::Camera;
use grana_render::{FrameInfo, Renderer};
use grana_scene::Scene;
use grana_ui::Ui;
use wgpu::{CommandEncoderDescriptor, Device, Queue, Texture, TextureView, TextureViewDescriptor};

use crate::capture;
use crate::compose::Composer;
use crate::figure::{self, Figure};
use crate::plan::{Destination, Plan, Pose, Still, camera, detail_lens, still};
use crate::sink::{self, Sink};
use grana_page::Format;

const DEVIATION_TO_SIGMA: f32 = 1.2533;
const DEVIATION_WINDOW: usize = 4;
const IMAGE_SAMPLE_LIMIT: u32 = 64;
const MOVIE_SAMPLE_LIMIT: u32 = 32;
const IMAGE_SAMPLE_FLOOR: u32 = 16;
const MOVIE_SAMPLE_FLOOR: u32 = 8;
const TEMPORAL_HISTORY: f32 = 16.0;
const TEMPORAL_SAMPLE_LIMIT: u32 = 8;
const TEMPORAL_SAMPLE_FLOOR: u32 = 2;
const IMAGE_ERROR: f32 = 0.0035;
const MOVIE_ERROR: f32 = 0.005;
const IMAGE_ATTEMPTS: u32 = 4096;
const MOVIE_ATTEMPTS: u32 = 4096;

pub(crate) enum Step {
    Running,
    Complete(PathBuf),
    Failed(String),
}

pub(crate) struct Export {
    sink: Option<Sink>,
    poses: Vec<Pose>,
    frame: usize,
    attempts: u32,
    samples: u32,
    floor: u32,
    base_samples: u32,
    base_floor: u32,
    error: f32,
    converged: bool,
    history: bool,
    temporal: Option<grana_render::Temporal>,
    deviations: std::collections::VecDeque<f32>,
    viewport: grana_engine::Viewport,
    lens: Option<(Camera, u32)>,
    transparent: bool,
    figure: Option<Format>,
    draw: (Texture, TextureView),
    overlay: (Texture, TextureView),
}

impl Export {
    pub(crate) fn start(
        device: &Device,
        format: wgpu::TextureFormat,
        plan: Plan,
    ) -> Result<Self, String> {
        let Plan {
            destination,
            size,
            transparent,
            poses,
            fps,
            region: _,
        } = plan;
        assert!(!poses.is_empty(), "an export holds at least one pose");
        if matches!(
            destination,
            Destination::Lineage { .. } | Destination::Chart { .. }
        ) {
            return Err("a vector page is written without a renderer".to_string());
        }
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err("an export reads back an eight bit colour target".to_string());
        }
        let movie = matches!(destination, Destination::Movie { .. });
        assert!(!movie || fps > 0, "a movie holds a positive frame rate");
        assert!(
            !movie || size.height().is_multiple_of(2) && size.width().is_multiple_of(2),
            "a movie frame holds an even number of pixels"
        );
        let figure = match &destination {
            Destination::Figure { path } => Some(
                Format::of(path)
                    .ok_or_else(|| grana_page::Format::error(path, "a vector figure"))?,
            ),
            _ => None,
        };
        if figure.is_some() && poses.len() != 1 {
            return Err("a vector figure renders exactly one frame".to_string());
        }
        let viewport = grana_engine::Viewport::export(size);
        let limit = device.limits().max_texture_dimension_2d;
        if viewport.size[0] > limit || viewport.size[1] > limit {
            return Err(format!(
                "a {} × {} export exceeds the {limit} pixel limit of this device",
                viewport.size[0], viewport.size[1]
            ));
        }
        let kind = match &destination {
            Destination::Image { path } => Some(still(path)?),
            _ => None,
        };
        let draw = capture::texture(device, "grana.export", format, viewport.size);
        let draw_view = draw.create_view(&TextureViewDescriptor::default());
        let overlay = capture::texture(device, "grana.export.overlay", format, viewport.size);
        let overlay_view = overlay.create_view(&TextureViewDescriptor::default());
        let sink = match destination {
            Destination::Image { path } => match kind.expect("the still was classified") {
                Still::Png => Sink::Png { path },
                Still::Tiff => Sink::Tiff { path },
            },
            Destination::Figure { path } => Sink::Figure { path },
            Destination::Lineage { .. } | Destination::Chart { .. } => {
                unreachable!("a vector page never reaches the renderer")
            }
            Destination::Movie { path } => Sink::Movie {
                recorder: grana_video::Recorder::start(&path, viewport.size, fps, transparent)
                    .map_err(|error| error.to_string())?,
                path,
            },
            Destination::Sequence { directory, stem } => Sink::Sequence {
                sequence: grana_video::Sequence::start(&directory, &stem, poses.len() as u32)
                    .map_err(|error| error.to_string())?,
            },
        };
        let samples = if movie {
            MOVIE_SAMPLE_LIMIT
        } else {
            IMAGE_SAMPLE_LIMIT
        };
        let floor = if movie {
            MOVIE_SAMPLE_FLOOR
        } else {
            IMAGE_SAMPLE_FLOOR
        };
        let error = if movie { MOVIE_ERROR } else { IMAGE_ERROR };
        let lens = detail_lens(&poses, viewport.size[1]);
        let temporal = movie.then(|| grana_render::Temporal::new(TEMPORAL_HISTORY));
        Ok(Self {
            sink: Some(sink),
            poses,
            frame: 0,
            attempts: 0,
            samples,
            floor,
            base_samples: samples,
            base_floor: floor,
            error,
            converged: false,
            history: false,
            temporal,
            deviations: std::collections::VecDeque::new(),
            viewport,
            lens,
            transparent,
            figure,
            draw: (draw, draw_view),
            overlay: (overlay, overlay_view),
        })
    }

    pub(crate) fn lens(&self) -> Option<grana_engine::Lens<'_>> {
        self.lens
            .as_ref()
            .map(|(camera, height)| grana_engine::Lens {
                camera,
                height: *height,
            })
    }

    pub(crate) fn pose(&self) -> Pose {
        self.poses[self.frame]
    }

    pub(crate) fn progress(&self) -> (u32, u32) {
        (self.frame as u32, self.poses.len() as u32)
    }

    pub(crate) fn size(&self) -> [u32; 2] {
        self.viewport.size
    }

    pub(crate) fn render(
        &mut self,
        renderer: &mut Renderer,
        context: Context<'_>,
        scene: &Scene,
        rendered: &grana_engine::RenderedScene,
    ) -> Step {
        let Context {
            device,
            queue,
            ui,
            composer,
        } = context;
        let pose = self.poses[self.frame];
        let camera = camera(&pose.camera);
        renderer.set_transparent(self.transparent);
        let frame = rendered.frame(&[]);
        let info = match renderer.draw(
            device,
            queue,
            grana_render::Frame {
                target: &self.draw.1,
                size: self.viewport.size,
                camera: &camera,
                viewport: self.viewport.region,
                time: scene.time(),
                region: scene.region,
                clip: scene.clip,
                exposure: scene.exposure,
                samples: self.samples,
                scene: &frame,
                display: grana_render::Display::Export,
                level_cap: usize::MAX,
                temporal: self.temporal,
            },
        ) {
            Ok(info) => info,
            Err(detail) => return self.fail(detail),
        };
        self.attempts += 1;
        self.history = info.history;
        if self.history {
            self.samples = TEMPORAL_SAMPLE_LIMIT;
            self.floor = TEMPORAL_SAMPLE_FLOOR;
        } else {
            self.samples = self.base_samples;
            self.floor = self.base_floor;
        }
        if info.truncated {
            return self.fail(truncation(&info));
        }
        if let Some((samples, deviation)) = renderer.convergence(device) {
            self.deviations.push_back(deviation);
            if self.deviations.len() > DEVIATION_WINDOW {
                self.deviations.pop_front();
            }
            if self.deviations.len() == DEVIATION_WINDOW {
                let smoothed = self.deviations.iter().sum::<f32>() / DEVIATION_WINDOW as f32;
                let standard_error = DEVIATION_TO_SIGMA * smoothed / (samples as f32).sqrt();
                self.converged = samples >= self.floor && standard_error <= self.error;
            }
        }
        let resolved =
            info.samples >= self.samples || (self.converged && info.samples >= self.floor);
        if !resolved || !info.settled {
            if self.attempts > self.attempts_limit() {
                return self
                    .fail("the export could not resolve the volume being streamed".to_string());
            }
            return Step::Running;
        }
        let drawn = Drawn {
            renderer,
            device,
            queue,
            scene,
            pose,
        };
        match self.store(ui, drawn, composer) {
            Ok(Some(path)) => Step::Complete(path),
            Ok(None) => Step::Running,
            Err(detail) => Step::Failed(detail),
        }
    }

    fn attempts_limit(&self) -> u32 {
        match self.sink {
            Some(Sink::Movie { .. }) => MOVIE_ATTEMPTS,
            _ => IMAGE_ATTEMPTS,
        }
    }

    pub(crate) fn discard(mut self) {
        if let Some(sink) = self.sink.take() {
            sink.discard();
        }
    }

    fn fail(&mut self, detail: String) -> Step {
        if let Some(sink) = self.sink.take() {
            sink.discard();
        }
        Step::Failed(detail)
    }

    fn store(
        &mut self,
        ui: &mut Ui<()>,
        drawn: Drawn<'_>,
        composer: &mut Composer,
    ) -> Result<Option<PathBuf>, String> {
        let legends = grana_figure::legends(drawn.scene);
        let caption = grana_figure::caption(drawn.scene, drawn.pose.time);
        let labels = grana_figure::labels(drawn.scene, drawn.pose.time);
        let annotations = grana_figure::annotations(drawn.scene, drawn.pose.time);
        let camera = camera(&drawn.pose.camera);
        let figure = Figure {
            viewport: &self.viewport,
            camera: &camera,
            world: drawn.scene.dataset.world_size(),
            labels: &labels,
            annotations: &annotations,
            legends: &legends,
            caption: Some(&caption),
            backdrop: !self.transparent,
        };
        if let Some(format) = self.figure {
            let marks = figure::marks(ui, figure);
            let image = drawn.renderer.picture(
                drawn.device,
                drawn.queue,
                drawn.scene.exposure,
                self.transparent,
            );
            let size = image.size();
            assert_eq!(size, self.viewport.size, "the picture covers the export");
            let sink = self.sink.take().expect("the export is alive");
            let Sink::Figure { path } = sink else {
                unreachable!("a vector figure writes a figure sink")
            };
            let mut page = grana_vector::Page::new(size[0] as f32, size[1] as f32);
            page.set_background(grana_vector::Image::new(
                size,
                sink::rgba8(size, image.pixels(), self.transparent),
            ));
            grana_page::push(&mut page, &marks)?;
            grana_page::write_document(&path, format, &grana_vector::Document::of(page))?;
            self.frame += 1;
            self.attempts = 0;
            self.converged = false;
            self.deviations.clear();
            return Ok(Some(path));
        }
        let instances = figure::instances(ui, figure);
        let size = self.viewport.size;
        let scale = self.viewport.pixels_per_point;
        let overlay = &self.overlay.1;
        ui.flush(drawn.queue);
        let mut encoder = drawn
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("grana.export.figure"),
            });
        capture::clear(&mut encoder, overlay);
        let (figure, atlas, images) = ui.renderer_and_atlas();
        figure.paint_figure(
            &mut encoder,
            grana_ui::Figure {
                target: overlay,
                size,
                scale,
                instances: &instances,
            },
            atlas,
            images,
        );
        drawn.queue.submit([encoder.finish()]);
        let (picture, picture_size) = drawn.renderer.wide_picture(
            drawn.device,
            drawn.queue,
            drawn.scene.exposure,
            self.transparent,
        );
        assert_eq!(picture_size, size, "the picture covers the export");
        let picture_view = picture.create_view(&TextureViewDescriptor::default());
        let image = composer.compose(
            drawn.device,
            drawn.queue,
            &picture_view,
            overlay,
            size,
            self.transparent,
        );
        let mut sink = self.sink.take().expect("the export is alive");
        let written = match &sink {
            Sink::Tiff { path } => {
                let physical =
                    camera.world_per_point(self.viewport.size[1] as f32, camera.distance());
                crate::sink::write_picture(
                    path,
                    image.size(),
                    image.pixels(),
                    drawn.scene,
                    drawn.pose.time,
                    physical,
                    self.transparent,
                )
            }
            _ => sink.push(drawn.pose.time, image.size(), &image, self.transparent),
        };
        if let Err(detail) = written {
            sink.discard();
            return Err(detail);
        }
        self.frame += 1;
        self.attempts = 0;
        self.converged = false;
        self.deviations.clear();
        if self.frame < self.poses.len() {
            self.sink = Some(sink);
            return Ok(None);
        }
        sink.finish().map(Some)
    }
}

pub(crate) struct Context<'a> {
    pub(crate) device: &'a Device,
    pub(crate) queue: &'a Queue,
    pub(crate) ui: &'a mut Ui<()>,
    pub(crate) composer: &'a mut Composer,
}

struct Drawn<'a> {
    renderer: &'a mut Renderer,
    device: &'a Device,
    queue: &'a Queue,
    scene: &'a Scene,
    pose: Pose,
}

fn truncation(info: &FrameInfo) -> String {
    let held = info.view;
    format!(
        "an export at this size needs {} bricks at level {} but only {} fit in the residency budget; lower the export width or zoom in",
        held.visible, held.level, held.demand
    )
}
