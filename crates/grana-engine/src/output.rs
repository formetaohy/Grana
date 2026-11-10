use std::path::{Path, PathBuf};

use grana_display::{
    Camera, PANE_COUNT, PaneKind, PaneView, SectionCamera, linear_to_srgb, srgb_component_to_linear,
};
use grana_raster::Raster;
use grana_render::{FrameInfo, Lens, PaneFrame, Renderer};
use grana_scene::Scene;
use grana_ui::Ui;
use grana_video::{Recorder, Sequence};
use grana_volume::{Pyramid, Region};
use wgpu::{
    CommandEncoder, CommandEncoderDescriptor, Device, Extent3d, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, StoreOp, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::frame::{self, View};
use crate::overlay;
use crate::panels::{Panels, Regions};

const IMAGE_SAMPLE_LIMIT: u32 = 64;
const MOVIE_SAMPLE_LIMIT: u32 = 32;
const IMAGE_SAMPLE_FLOOR: u32 = 16;
const MOVIE_SAMPLE_FLOOR: u32 = 8;
const IMAGE_ERROR: f32 = 0.0035;
const MOVIE_ERROR: f32 = 0.005;
const DEVIATION_TO_SIGMA: f32 = 1.2533;
const DEVIATION_WINDOW: usize = 4;
const WIDE_BYTES: u64 = 16;
const WIDE_CEILING: u64 = 1 << 30;
const IMAGE_ATTEMPTS: u32 = 4096;
const MOVIE_ATTEMPTS: u32 = 4096;

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub time: u32,
    pub views: [PaneView; PANE_COUNT],
}

pub enum Destination {
    Image { path: PathBuf },
    Movie { path: PathBuf },
    Sequence { directory: PathBuf, stem: String },
}

pub struct Plan {
    pub destination: Destination,
    pub panels: Panels,
    pub width: u32,
    pub transparent: bool,
    pub poses: Vec<Pose>,
    pub fps: u32,
    pub region: Region,
}

pub enum Outcome {
    Running,
    Complete { path: PathBuf },
    Failed { detail: String },
}

enum Sink {
    Png { path: PathBuf },
    Tiff { path: PathBuf },
    Movie { path: PathBuf, recorder: Recorder },
    Sequence { sequence: Sequence },
}

impl Sink {
    fn push(&mut self, time: u32, size: [u32; 2], pixels: &[u8]) -> Result<(), String> {
        let raster = || Raster::new(size[0], size[1], pixels.to_vec());
        match self {
            Self::Png { path } => grana_raster::write(path, &raster()).map_err(String::from),
            Self::Tiff { .. } => {
                unreachable!("a wide picture is written from the rendered frame")
            }
            Self::Movie { recorder, .. } => {
                recorder.push(pixels).map_err(|error| error.to_string())
            }
            Self::Sequence { sequence } => {
                let png = grana_raster::encode(&raster()).map_err(String::from)?;
                sequence.push(time, &png).map_err(|error| error.to_string())
            }
        }
    }

    fn finish(self) -> Result<PathBuf, String> {
        match self {
            Self::Png { path } => Ok(path),
            Self::Tiff { path } => Ok(path),
            Self::Movie { path, recorder } => {
                recorder.finish().map_err(|error| error.to_string())?;
                Ok(path)
            }
            Self::Sequence { sequence } => sequence.finish().map_err(|error| error.to_string()),
        }
    }
}

pub(crate) struct ExportResidency {
    pub(crate) slots: usize,
    pub(crate) level: usize,
}

pub(crate) fn validate(plan: &Plan) -> Result<(), String> {
    if plan.poses.is_empty() {
        return Err("an export holds at least one pose".to_string());
    }
    if let Destination::Image { path } = &plan.destination
        && still(path)? == Still::Tiff
    {
        let size = plan.panels.export(plan.width, false).size;
        let bytes = u64::from(size[0]) * u64::from(size[1]) * WIDE_BYTES;
        if bytes > WIDE_CEILING {
            return Err(format!(
                "a sixteen bit still at {} × {} needs more than {} GiB of texture memory; lower the export width",
                size[0],
                size[1],
                WIDE_CEILING >> 30
            ));
        }
    }
    Ok(())
}

pub(crate) fn residency(plan: &Plan, pyramid: &Pyramid) -> Result<ExportResidency, String> {
    if plan.poses.is_empty() {
        return Err("an export holds at least one pose".to_string());
    }
    let movie = matches!(plan.destination, Destination::Movie { .. });
    let regions = plan.panels.export(plan.width, movie);
    let mut slots = 1usize;
    let mut level = pyramid.level_count() - 1;
    for pose in &plan.poses {
        let cameras = pose.views.map(|view| frame::restored_camera(&view.camera));
        let sections = pose
            .views
            .map(|view| frame::restored_section(&view.section));
        let panes = std::array::from_fn(|index| {
            let pane = regions.panes[index]?;
            let aspect = pane.region[2].max(1) as f32 / pane.region[3].max(1) as f32;
            Some(match pane.kind {
                PaneKind::Volume => PaneFrame::Volume {
                    camera: &cameras[index],
                    lens: Lens {
                        camera: &cameras[index],
                        aspect,
                    },
                    region: pane.region,
                },
                PaneKind::Section => PaneFrame::Section {
                    section: &sections[index],
                    region: pane.region,
                },
            })
        });
        let held = grana_render::residency(pyramid, &panes, plan.region);
        slots = slots.max(held.slots);
        for detail in held.panes.iter().flatten() {
            level = level.min(detail.level);
        }
    }
    Ok(ExportResidency { slots, level })
}

fn truncation(info: &FrameInfo, regions: &Regions) -> String {
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

pub struct Export {
    sink: Option<Sink>,
    poses: Vec<Pose>,
    frame: usize,
    attempts: u32,
    samples: u32,
    floor: u32,
    error: f32,
    converged: bool,
    deviations: std::collections::VecDeque<f32>,
    regions: Regions,
    lens: Option<(Camera, u32)>,
    transparent: bool,
    format: TextureFormat,
    texture: Texture,
    view: TextureView,
    layer: Option<(Texture, TextureView)>,
    figure: Option<(Texture, TextureView)>,
}

impl Export {
    pub fn start(device: &Device, format: TextureFormat, plan: Plan) -> Result<Self, String> {
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
            TextureFormat::Rgba8Unorm
                | TextureFormat::Rgba8UnormSrgb
                | TextureFormat::Bgra8Unorm
                | TextureFormat::Bgra8UnormSrgb
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
        let texture = create_texture(device, "grana.export", format, regions.size);
        let view = texture.create_view(&TextureViewDescriptor::default());
        let layer = transparent.then(|| {
            let layer = create_texture(device, "grana.export.overlay", format, regions.size);
            let view = layer.create_view(&TextureViewDescriptor::default());
            (layer, view)
        });
        let sink = match destination {
            Destination::Image { path } => match kind.expect("the still was classified") {
                Still::Png => Sink::Png { path },
                Still::Tiff => Sink::Tiff { path },
            },
            Destination::Movie { path } => Sink::Movie {
                recorder: Recorder::start(&path, regions.size, fps, transparent)
                    .map_err(|error| error.to_string())?,
                path,
            },
            Destination::Sequence { directory, stem } => Sink::Sequence {
                sequence: Sequence::start(&directory, &stem, poses.len() as u32)
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

    pub fn lens(&self) -> Option<&(Camera, u32)> {
        self.lens.as_ref()
    }

    pub fn pose(&self) -> Pose {
        self.poses[self.frame]
    }

    pub fn progress(&self) -> (u32, u32) {
        (self.frame as u32, self.poses.len() as u32)
    }

    pub fn render<A: Clone>(
        &mut self,
        renderer: &mut Renderer,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui<A>,
        scene: &Scene,
        rendered: &crate::view::RenderedScene,
    ) -> Outcome {
        let pose = self.poses[self.frame];
        let cameras = pose.views.map(|view| frame::restored_camera(&view.camera));
        let sections = pose
            .views
            .map(|view| frame::restored_section(&view.section));
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
        let info = frame::draw(
            renderer,
            device,
            queue,
            View {
                target: &self.view,
                scene,
                panes,
                size: self.regions.size,
                samples: self.samples,
                prefetch: false,
                transparent: self.transparent,
                level_cap: usize::MAX,
                assured: true,
            },
            rendered,
        );
        self.attempts += 1;
        if info.truncated {
            return Outcome::Failed {
                detail: truncation(&info, &self.regions),
            };
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
                return Outcome::Failed {
                    detail: "the export could not resolve the volume being streamed".to_string(),
                };
            }
            return Outcome::Running;
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
            Ok(Some(path)) => Outcome::Complete { path },
            Ok(None) => Outcome::Running,
            Err(detail) => Outcome::Failed { detail },
        }
    }

    fn attempts_limit(&self) -> u32 {
        match self.sink {
            Some(Sink::Movie { .. }) => MOVIE_ATTEMPTS,
            _ => IMAGE_ATTEMPTS,
        }
    }

    fn store<A: Clone>(
        &mut self,
        ui: &mut Ui<A>,
        mut drawn: Drawn<'_>,
    ) -> Result<Option<PathBuf>, String> {
        let legends = grana_figure::legends(drawn.scene);
        let caption = grana_figure::caption(drawn.scene, drawn.pose.time);
        let labels = grana_figure::labels(drawn.scene, drawn.pose.time);
        let annotations = grana_figure::annotations(drawn.scene, drawn.pose.time);
        let instances = overlay::instances(
            ui,
            overlay::Figure {
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
        match captured {
            Capture::Png(pixels) => sink.push(drawn.pose.time, size, &pixels)?,
            Capture::Tiff(pixels) => match &sink {
                Sink::Tiff { path } => write_picture(
                    path,
                    size,
                    &pixels,
                    drawn.scene,
                    drawn.pose.time,
                    self.transparent,
                )?,
                _ => unreachable!("a wide picture comes from a wide sink"),
            },
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

    fn wide_pixels<A: Clone>(
        &mut self,
        ui: &mut Ui<A>,
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
            let texture = create_texture(device, "grana.export.figure", format, size);
            let view = texture.create_view(&TextureViewDescriptor::default());
            self.figure = Some((texture, view));
        }
        let (texture, view) = self.figure.as_ref().expect("the figure layer is ready");
        ui.flush(queue);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grana.export.figure"),
        });
        clear(&mut encoder, view);
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
        Ok(compose(&picture, &overlay, self.transparent))
    }

    fn capture<A: Clone>(
        &mut self,
        ui: &mut Ui<A>,
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
            clear(&mut encoder, view);
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
            let below = Raster::new(size[0], size[1], below.pixels().to_vec());
            let above = Raster::new(size[0], size[1], above.pixels().to_vec());
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

pub fn name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("the export")
        .to_string()
}

struct Drawn<'a> {
    renderer: &'a mut Renderer,
    device: &'a Device,
    queue: &'a Queue,
    scene: &'a Scene,
    pose: Pose,
    samples: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Still {
    Png,
    Tiff,
}

enum Capture {
    Png(Vec<u8>),
    Tiff(Vec<u16>),
}

fn still(path: &Path) -> Result<Still, String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("png") => Ok(Still::Png),
        Some("tif" | "tiff") => Ok(Still::Tiff),
        other => Err(format!(
            "a still image is saved as PNG or OME-TIFF, not `{}`",
            other.unwrap_or("a nameless file")
        )),
    }
}

fn compose(
    picture: &grana_render::Image16,
    overlay: &grana_render::Image,
    transparent: bool,
) -> Vec<u16> {
    assert_eq!(
        picture.size(),
        overlay.size(),
        "the figure layer covers its scene"
    );
    let mut composed = Vec::with_capacity(picture.pixels().len());
    for (below, above) in picture
        .pixels()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(overlay.pixels().as_chunks::<4>().0)
    {
        if transparent {
            over_transparent(below, above, &mut composed);
        } else {
            over_opaque(below, above, &mut composed);
        }
    }
    composed
}

fn over_opaque(below: &[u16], above: &[u8], out: &mut Vec<u16>) {
    let keep = 1.0 - f32::from(above[3]) / 255.0;
    for (index, value) in below.iter().take(3).enumerate() {
        let mixed = f32::from(above[index]) * 257.0 + f32::from(*value) * keep;
        out.push(mixed.round().clamp(0.0, 65535.0) as u16);
    }
    out.push(u16::MAX);
}

fn over_transparent(below: &[u16], above: &[u8], out: &mut Vec<u16>) {
    let front = f32::from(above[3]) / 255.0;
    let back = f32::from(below[3]) / 65535.0;
    let covered = back * (1.0 - front);
    let alpha = front + covered;
    if alpha <= 0.0 {
        out.extend_from_slice(&[0; 4]);
        return;
    }
    for index in 0..3 {
        let premultiplied = if front > 0.0 {
            srgb_component_to_linear(f32::from(above[index]) / 255.0 / front) * front
        } else {
            0.0
        };
        let value = (premultiplied
            + srgb_component_to_linear(f32::from(below[index]) / 65535.0) * covered)
            / alpha;
        out.push(quantize(linear_to_srgb(value)));
    }
    out.push((alpha * 65535.0).round().clamp(0.0, 65535.0) as u16);
}

fn quantize(encoded: f32) -> u16 {
    (encoded.clamp(0.0, 1.0) * 65535.0).round() as u16
}

fn write_picture(
    path: &Path,
    size: [u32; 2],
    pixels: &[u16],
    scene: &Scene,
    time: u32,
    transparent: bool,
) -> Result<(), String> {
    let name = if scene.dataset.frame_count() > 1 {
        format!("{} · t={time}", scene.dataset.name())
    } else {
        scene.dataset.name().to_string()
    };
    let rgb;
    let picture = if transparent {
        grana_tiff::Picture {
            size,
            samples: 4,
            pixels,
            name: &name,
        }
    } else {
        rgb = strip_alpha(pixels);
        grana_tiff::Picture {
            size,
            samples: 3,
            pixels: &rgb,
            name: &name,
        }
    };
    grana_tiff::write(path, &picture).map_err(|error| error.to_string())
}

fn strip_alpha(pixels: &[u16]) -> Vec<u16> {
    let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
    for pixel in pixels.as_chunks::<4>().0 {
        rgb.extend_from_slice(&pixel[..3]);
    }
    rgb
}

fn create_texture(
    device: &Device,
    label: &'static str,
    format: TextureFormat,
    size: [u32; 2],
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn clear(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.export.overlay.clear"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

pub(crate) fn restored_camera(view: &PaneView) -> Camera {
    frame::restored_camera(&view.camera)
}

fn detail_lens(poses: &[Pose], regions: &Regions) -> Option<(Camera, u32)> {
    let mut best: Option<(Camera, u32, f32)> = None;
    for pose in poses {
        for (index, pane) in regions.panes.iter().enumerate() {
            let Some(pane) = pane else {
                continue;
            };
            if pane.kind != PaneKind::Volume {
                continue;
            }
            let height = pane.region[3].max(1);
            let camera = restored_camera(&pose.views[index]);
            let scale = camera.pixels_per_world(height);
            if best.as_ref().is_none_or(|(_, _, held)| scale > *held) {
                best = Some((camera, height, scale));
            }
        }
    }
    best.map(|(camera, height, _)| (camera, height))
}

pub(crate) fn restored_section(view: &PaneView) -> SectionCamera {
    frame::restored_section(&view.section)
}
