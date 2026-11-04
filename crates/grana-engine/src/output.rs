use std::path::{Path, PathBuf};

use grana_display::{CameraState, SectionState};
use grana_raster::Raster;
use grana_render::Renderer;
use grana_scene::Scene;
use grana_ui::Ui;
use grana_video::{Recorder, Sequence};
use wgpu::{
    CommandEncoder, CommandEncoderDescriptor, Device, Extent3d, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, StoreOp, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::frame::{self, View};
use crate::overlay;
use crate::panels::{Panels, Panes};

pub const IMAGE_SAMPLES: u32 = 8;
pub const MOVIE_SAMPLES: u32 = 16;
const IMAGE_ATTEMPTS: u32 = 512;
const MOVIE_ATTEMPTS: u32 = 4096;

#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub time: u32,
    pub camera: CameraState,
    pub section: SectionState,
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
}

pub enum Outcome {
    Running,
    Complete { path: PathBuf },
    Failed { detail: String },
}

enum Sink {
    Image { path: PathBuf },
    Movie { path: PathBuf, recorder: Recorder },
    Sequence { sequence: Sequence },
}

impl Sink {
    fn push(&mut self, time: u32, size: [u32; 2], pixels: &[u8]) -> Result<(), String> {
        let raster = || Raster::new(size[0], size[1], pixels.to_vec());
        match self {
            Self::Image { path } => grana_raster::write(path, &raster()).map_err(String::from),
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
            Self::Image { path } => Ok(path),
            Self::Movie { path, recorder } => {
                recorder.finish().map_err(|error| error.to_string())?;
                Ok(path)
            }
            Self::Sequence { sequence } => sequence.finish().map_err(|error| error.to_string()),
        }
    }
}

pub struct Export {
    sink: Option<Sink>,
    poses: Vec<Pose>,
    frame: usize,
    attempts: u32,
    samples: u32,
    panes: Panes,
    transparent: bool,
    format: TextureFormat,
    texture: Texture,
    view: TextureView,
    layer: Option<(Texture, TextureView)>,
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
        if transparent && movie {
            return Err("the movie encoder carries no alpha channel".to_string());
        }
        let panes = panels.export(width, movie);
        let limit = device.limits().max_texture_dimension_2d;
        if panes.size[0] > limit || panes.size[1] > limit {
            return Err(format!(
                "a {} × {} export exceeds the {limit} pixel limit of this device",
                panes.size[0], panes.size[1]
            ));
        }
        let texture = create_texture(device, "grana.export", format, panes.size);
        let view = texture.create_view(&TextureViewDescriptor::default());
        let layer = transparent.then(|| {
            let layer = create_texture(device, "grana.export.overlay", format, panes.size);
            let view = layer.create_view(&TextureViewDescriptor::default());
            (layer, view)
        });
        let sink = match destination {
            Destination::Image { path } => Sink::Image { path },
            Destination::Movie { path } => Sink::Movie {
                recorder: Recorder::start(&path, panes.size, fps)
                    .map_err(|error| error.to_string())?,
                path,
            },
            Destination::Sequence { directory, stem } => Sink::Sequence {
                sequence: Sequence::start(&directory, &stem, poses.len() as u32)
                    .map_err(|error| error.to_string())?,
            },
        };
        let samples = if movie { MOVIE_SAMPLES } else { IMAGE_SAMPLES };
        Ok(Self {
            sink: Some(sink),
            poses,
            frame: 0,
            attempts: 0,
            samples,
            panes,
            transparent,
            format,
            texture,
            view,
            layer,
        })
    }

    pub fn pose(&self) -> Pose {
        self.poses[self.frame]
    }

    pub fn progress(&self) -> (u32, u32) {
        (self.frame as u32, self.poses.len() as u32)
    }

    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui,
        scene: &Scene,
    ) -> Outcome {
        let pose = self.poses[self.frame];
        let camera = frame::restored_camera(&pose.camera);
        let section = frame::restored_section(&pose.section);
        let rendered = match crate::view::frame(scene) {
            Ok(rendered) => rendered,
            Err(detail) => return Outcome::Failed { detail },
        };
        let info = frame::draw(
            renderer,
            device,
            queue,
            View {
                target: &self.view,
                scene,
                camera: &camera,
                section: &section,
                size: self.panes.size,
                volume_region: self.panes.volume,
                section_region: self.panes.section,
                samples: self.samples,
                prefetch: false,
                transparent: self.transparent,
            },
            &rendered,
        );
        self.attempts += 1;
        if info.samples < self.samples {
            if self.attempts > self.attempts_limit() {
                return Outcome::Failed {
                    detail: "the export could not resolve the volume being streamed".to_string(),
                };
            }
            return Outcome::Running;
        }
        match self.store(device, queue, ui, scene, pose) {
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

    fn store(
        &mut self,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui,
        scene: &Scene,
        pose: Pose,
    ) -> Result<Option<PathBuf>, String> {
        let legends = grana_figure::legends(scene);
        let caption = grana_figure::caption(scene, pose.time);
        let instances = overlay::instances(
            ui,
            &self.panes,
            &pose.camera,
            &pose.section,
            &legends,
            Some(&caption),
            !self.transparent,
        );
        let pixels = self.capture(device, queue, ui, &instances)?;
        let size = self.panes.size;
        let mut sink = self.sink.take().expect("the export is alive");
        sink.push(pose.time, size, &pixels)?;
        self.frame += 1;
        self.attempts = 0;
        if self.frame < self.poses.len() {
            self.sink = Some(sink);
            return Ok(None);
        }
        sink.finish().map(Some)
    }

    fn capture(
        &mut self,
        device: &Device,
        queue: &Queue,
        ui: &mut Ui,
        instances: &[grana_ui::Instance],
    ) -> Result<Vec<u8>, String> {
        let size = self.panes.size;
        let scale = self.panes.pixels_per_point;
        ui.flush(queue);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grana.export.figure"),
        });
        if self.transparent {
            let (texture, view) = self.layer.as_ref().expect("the export carries a layer");
            clear(&mut encoder, view);
            let (renderer, atlas) = ui.renderer_and_atlas();
            renderer.paint_figure(&mut encoder, view, size, scale, atlas, instances);
            queue.submit([encoder.finish()]);
            let below = grana_render::read_image(device, queue, &self.texture, size, self.format);
            let above = grana_render::read_image(device, queue, texture, size, self.format);
            let below = Raster::new(size[0], size[1], below.pixels().to_vec());
            let above = Raster::new(size[0], size[1], above.pixels().to_vec());
            return Ok(grana_raster::composite_premultiplied(&below, &above)
                .pixels()
                .to_vec());
        }
        let (renderer, atlas) = ui.renderer_and_atlas();
        renderer.paint_figure(&mut encoder, &self.view, size, scale, atlas, instances);
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
