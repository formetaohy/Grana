use std::path::PathBuf;

use grana_display::{Camera, PaneKind};
use grana_render::{FrameInfo, Lens, PaneFrame, Renderer};
use grana_scene::Scene;
use grana_ui::Ui;
use wgpu::{CommandEncoderDescriptor, Device, Queue, Texture, TextureView, TextureViewDescriptor};

use crate::capture::{self, Capture};
use crate::figure::{self, Figure};
use crate::plan::{Destination, Plan, Pose, Still, camera, detail_lens, section, still};
use crate::sink::Sink;

const DEVIATION_TO_SIGMA: f32 = 1.2533;
const DEVIATION_WINDOW: usize = 4;
const IMAGE_SAMPLE_LIMIT: u32 = 64;
const MOVIE_SAMPLE_LIMIT: u32 = 32;
const IMAGE_SAMPLE_FLOOR: u32 = 16;
const MOVIE_SAMPLE_FLOOR: u32 = 8;
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
    error: f32,
    converged: bool,
    deviations: std::collections::VecDeque<f32>,
    regions: grana_engine::Regions,
    lens: Option<(Camera, u32)>,
    transparent: bool,
    format: wgpu::TextureFormat,
    texture: Texture,
    view: TextureView,
    layer: Option<(Texture, TextureView)>,
    figure: Option<(Texture, TextureView)>,
}

impl Export {
    pub(crate) fn start(
        device: &Device,
        format: wgpu::TextureFormat,
        plan: Plan,
    ) -> Result<Self, String> {
        let Plan {
            destination,
            panels,
            width,
            transparent,
            poses,
            fps,
            region: _,
        } = plan;
        assert!(!poses.is_empty(), "an export holds at least one pose");
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
        let regions = panels.export(width, movie);
        let limit = device.limits().max_texture_dimension_2d;
        if regions.size[0] > limit || regions.size[1] > limit {
            return Err(format!(
                "a {} × {} export exceeds the {limit} pixel limit of this device",
                regions.size[0], regions.size[1]
            ));
        }
        let kind = match &destination {
            Destination::Image { path } => Some(still(path)?),
            _ => None,
        };
        let texture = capture::texture(device, "grana.export", format, regions.size);
        let view = texture.create_view(&TextureViewDescriptor::default());
        let layer = transparent.then(|| {
            let layer = capture::texture(device, "grana.export.overlay", format, regions.size);
            let view = layer.create_view(&TextureViewDescriptor::default());
            (layer, view)
        });
        let sink = match destination {
            Destination::Image { path } => match kind.expect("the still was classified") {
                Still::Png => Sink::Png { path },
                Still::Tiff => Sink::Tiff { path },
            },
            Destination::Movie { path } => Sink::Movie {
                recorder: grana_video::Recorder::start(&path, regions.size, fps, transparent)
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
        let lens = detail_lens(&poses, &regions);
        Ok(Self {
            sink: Some(sink),
            poses,
            frame: 0,
            attempts: 0,
            samples,
            floor,
            error,
            converged: false,
            deviations: std::collections::VecDeque::new(),
            regions,
            lens,
            transparent,
            format,
            texture,
            view,
            layer,
            figure: None,
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
        self.regions.size
    }

    pub(crate) fn render(
        &mut self,
        renderer: &mut Renderer,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui<()>,
        scene: &Scene,
        rendered: &grana_engine::RenderedScene,
    ) -> Step {
        let pose = self.poses[self.frame];
        let cameras = pose.views.map(|view| camera(&view.camera));
        let sections = pose.views.map(|view| section(&view.section));
        let panes = std::array::from_fn(|index| {
            let pane = self.regions.panes[index]?;
            match pane.kind {
                PaneKind::Volume => Some(PaneFrame::Volume {
                    camera: &cameras[index],
                    lens: Lens {
                        camera: &cameras[index],
                        aspect: pane.region[2].max(1) as f32 / pane.region[3].max(1) as f32,
                    },
                    region: pane.region,
                }),
                PaneKind::Section => Some(PaneFrame::Section {
                    section: &sections[index],
                    region: pane.region,
                }),
            }
        });
        renderer.set_transparent(self.transparent);
        let frame = rendered.frame();
        let info = renderer.draw(
            device,
            queue,
            grana_render::Frame {
                target: &self.view,
                size: self.regions.size,
                panes,
                time: scene.time(),
                region: scene.region,
                clip: scene.clip,
                exposure: scene.exposure,
                samples: self.samples,
                scene: &frame,
                prefetch: false,
                level_cap: usize::MAX,
                assured: true,
            },
        );
        self.attempts += 1;
        if info.truncated {
            return self.fail(truncation(&info, &self.regions));
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
            samples: info.samples,
        };
        match self.store(ui, drawn) {
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

    fn store(&mut self, ui: &mut Ui<()>, mut drawn: Drawn<'_>) -> Result<Option<PathBuf>, String> {
        let legends = grana_figure::legends(drawn.scene);
        let caption = grana_figure::caption(drawn.scene, drawn.pose.time);
        let labels = grana_figure::labels(drawn.scene, drawn.pose.time);
        let annotations = grana_figure::annotations(drawn.scene, drawn.pose.time);
        let instances = figure::instances(
            ui,
            Figure {
                regions: &self.regions,
                views: &drawn.pose.views,
                world: drawn.scene.dataset.world_size(),
                labels: &labels,
                annotations: &annotations,
                voxel: drawn.scene.dataset.voxel_size(),
                legends: &legends,
                caption: Some(&caption),
                backdrop: !self.transparent,
            },
        );
        let tiff = matches!(self.sink, Some(Sink::Tiff { .. }));
        let captured = if tiff {
            Capture::Tiff(self.wide_pixels(ui, &mut drawn, &instances)?)
        } else {
            Capture::Png(self.capture(ui, &drawn, &instances)?)
        };
        let size = self.regions.size;
        let mut sink = self.sink.take().expect("the export is alive");
        let written = match captured {
            Capture::Png(pixels) => sink.push(drawn.pose.time, size, &pixels),
            Capture::Tiff(pixels) => match &sink {
                Sink::Tiff { path } => crate::sink::write_picture(
                    path,
                    size,
                    &pixels,
                    drawn.scene,
                    drawn.pose.time,
                    self.transparent,
                ),
                _ => unreachable!("a wide picture comes from a wide sink"),
            },
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

    fn wide_pixels(
        &mut self,
        ui: &mut Ui<()>,
        drawn: &mut Drawn<'_>,
        instances: &[grana_ui::Instance],
    ) -> Result<Vec<u16>, String> {
        let size = self.regions.size;
        let scale = self.regions.pixels_per_point;
        let device = drawn.device;
        let queue = drawn.queue;
        let picture = drawn.renderer.picture(
            device,
            queue,
            drawn.scene.exposure,
            drawn.samples,
            self.transparent,
        );
        let format = self.format;
        if self.figure.is_none() {
            let texture = capture::texture(device, "grana.export.figure", format, size);
            let view = texture.create_view(&TextureViewDescriptor::default());
            self.figure = Some((texture, view));
        }
        let (texture, view) = self.figure.as_ref().expect("the figure layer is ready");
        ui.flush(queue);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grana.export.figure"),
        });
        capture::clear(&mut encoder, view);
        let (figure, atlas, images) = ui.renderer_and_atlas();
        figure.paint_figure(
            &mut encoder,
            grana_ui::Figure {
                target: view,
                size,
                scale,
                instances,
            },
            atlas,
            images,
        );
        queue.submit([encoder.finish()]);
        let overlay = grana_render::read_image(device, queue, texture, size, format);
        Ok(capture::compose(&picture, &overlay, self.transparent))
    }

    fn capture(
        &mut self,
        ui: &mut Ui<()>,
        drawn: &Drawn<'_>,
        instances: &[grana_ui::Instance],
    ) -> Result<Vec<u8>, String> {
        let device = drawn.device;
        let queue = drawn.queue;
        let size = self.regions.size;
        let scale = self.regions.pixels_per_point;
        ui.flush(queue);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grana.export.figure"),
        });
        if self.transparent {
            let (texture, view) = self.layer.as_ref().expect("the export carries a layer");
            capture::clear(&mut encoder, view);
            let (renderer, atlas, images) = ui.renderer_and_atlas();
            renderer.paint_figure(
                &mut encoder,
                grana_ui::Figure {
                    target: view,
                    size,
                    scale,
                    instances,
                },
                atlas,
                images,
            );
            queue.submit([encoder.finish()]);
            let below = grana_render::read_image(device, queue, &self.texture, size, self.format);
            let above = grana_render::read_image(device, queue, texture, size, self.format);
            let below = grana_raster::Raster::new(size[0], size[1], below.pixels().to_vec());
            let above = grana_raster::Raster::new(size[0], size[1], above.pixels().to_vec());
            return Ok(grana_raster::composite_premultiplied(&below, &above)
                .pixels()
                .to_vec());
        }
        let (renderer, atlas, images) = ui.renderer_and_atlas();
        renderer.paint_figure(
            &mut encoder,
            grana_ui::Figure {
                target: &self.view,
                size,
                scale,
                instances,
            },
            atlas,
            images,
        );
        queue.submit([encoder.finish()]);
        let image = grana_render::read_image(device, queue, &self.texture, size, self.format);
        Ok(image.pixels().to_vec())
    }
}

struct Drawn<'a> {
    renderer: &'a mut Renderer,
    device: &'a Device,
    queue: &'a Queue,
    scene: &'a Scene,
    pose: Pose,
    samples: u32,
}

fn truncation(info: &FrameInfo, regions: &grana_engine::Regions) -> String {
    let held = info
        .panes
        .iter()
        .enumerate()
        .filter(|(index, _)| regions.panes[*index].is_some())
        .map(|(_, pane)| *pane)
        .max_by_key(|pane| pane.visible.saturating_sub(pane.demand))
        .unwrap_or_default();
    format!(
        "an export at this size needs {} bricks at level {} but only {} fit in the residency budget; lower the export width or zoom in",
        held.visible, held.level, held.demand
    )
}
