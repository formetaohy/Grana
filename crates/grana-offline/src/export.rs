use std::collections::VecDeque;
use std::path::PathBuf;

use grana_display::Camera;
use grana_gpu::Readback;
use grana_render::{Delivery, FrameInfo, Renderer};
use grana_scene::Scene;
use grana_ui::Ui;
use wgpu::{CommandEncoderDescriptor, Device, Queue, Texture, TextureView, TextureViewDescriptor};

use crate::capture;
use crate::compose::Composer;
use crate::figure::{self, Figure};
use crate::plan::{Destination, Plan, Pose, Still, camera, detail_lens, still};
use crate::sink::Sink;
use grana_page::Format;

const SAMPLES: u32 = 32;
const ATTEMPTS: u32 = 4096;
const READBACK_SLOTS: usize = 2;

pub(crate) enum Step {
    Running,
    Complete(PathBuf),
    Failed(String),
}

pub(crate) struct Export {
    sink: Option<Sink>,
    poses: Vec<Pose>,
    frame: usize,
    written: usize,
    attempts: u32,
    frames: u32,
    drawn: u32,
    viewport: grana_engine::Viewport,
    lens: Option<(Camera, u32)>,
    transparent: bool,
    figure: Option<Format>,
    draw: (Texture, TextureView),
    overlay: (Texture, TextureView),
    picture: (Texture, TextureView),
    readback: Readback,
    staged: VecDeque<[u32; 2]>,
    stride: u64,
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
        let picture = grana_render::picture_target(device, viewport.size);
        let picture_view = picture.create_view(&TextureViewDescriptor::default());
        let stride = grana_render::picture_stride(viewport.size[0]);
        let readback_bytes = stride * u64::from(viewport.size[1]);
        let buffers = device.limits().max_buffer_size;
        if readback_bytes > buffers {
            return Err(format!(
                "a {} × {} export reads {} MiB back at a time and this device holds at most {} MiB per buffer; lower the export size",
                viewport.size[0],
                viewport.size[1],
                readback_bytes >> 20,
                buffers >> 20
            ));
        }
        let readback = Readback::new(
            device,
            "grana.export.readback",
            readback_bytes,
            READBACK_SLOTS,
        );
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
        let lens = detail_lens(&poses, viewport.size[1]);
        Ok(Self {
            sink: Some(sink),
            poses,
            frame: 0,
            written: 0,
            attempts: 0,
            frames: SAMPLES,
            drawn: 0,
            viewport,
            lens,
            transparent,
            figure,
            draw: (draw, draw_view),
            overlay: (overlay, overlay_view),
            picture: (picture, picture_view),
            readback,
            staged: VecDeque::new(),
            stride,
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
        self.poses[self.frame.min(self.poses.len() - 1)]
    }

    pub(crate) fn progress(&self) -> (u32, u32) {
        (self.written as u32, self.poses.len() as u32)
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
        if let Some(step) = self.harvest(device, scene, false) {
            return step;
        }
        if self.frame == self.poses.len() {
            return self.harvest(device, scene, true).unwrap_or(Step::Running);
        }
        let pose = self.poses[self.frame];
        if self.drawn == 0 && self.attempts == 0 {
            renderer.restart();
        }
        let camera = camera(&pose.camera);
        renderer.set_transparent(self.transparent);
        let overlays = grana_engine::overlays(scene);
        let frame = rendered.frame(&overlays);
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
                scene: &frame,
                delivery: Delivery::Assured,
                level_cap: usize::MAX,
            },
        ) {
            Ok(info) => info,
            Err(detail) => return self.fail(detail),
        };
        self.attempts += 1;
        if info.truncated {
            return self.fail(truncation(&info));
        }
        if info.drawn {
            self.drawn = info.age;
        }
        if self.drawn < self.frames {
            if self.attempts > ATTEMPTS {
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
            Ok(step) => step,
            Err(detail) => Step::Failed(detail),
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

    fn harvest(&mut self, device: &Device, scene: &Scene, wait: bool) -> Option<Step> {
        let size = *self.staged.front()?;
        let stride = self.stride as usize;
        match self.readback.ready(device, wait) {
            Ok(true) => {}
            Ok(false) => return Some(Step::Running),
            Err(detail) => return Some(self.fail(detail)),
        }
        let pose = self.poses[self.written.min(self.poses.len() - 1)];
        let camera = camera(&pose.camera);
        let physical = camera.world_per_point(self.viewport.size[1] as f32, camera.distance());
        let transparent = self.transparent;
        let mut sink = self.sink.take().expect("the export is alive");
        let readback = &mut self.readback;
        let written = readback.take_ready(|_, bytes| {
            let picture = grana_render::Pixels::new(bytes, size, stride);
            sink.push(pose.time, picture, scene, physical, transparent)
        });
        self.staged.pop_front();
        if let Err(detail) = written {
            sink.discard();
            return Some(Step::Failed(detail));
        }
        self.written += 1;
        if self.written == self.poses.len() {
            return Some(match sink.finish() {
                Ok(path) => Step::Complete(path),
                Err(detail) => Step::Failed(detail),
            });
        }
        self.sink = Some(sink);
        Some(Step::Running)
    }
    fn store(
        &mut self,
        ui: &mut Ui<()>,
        mut drawn: Drawn<'_>,
        composer: &mut Composer,
    ) -> Result<Step, String> {
        if let Some(format) = self.figure {
            return match self.write_figure(ui, &mut drawn, format) {
                Ok(path) => Ok(Step::Complete(path)),
                Err(detail) => Err(detail),
            };
        }
        let free = self.readback.reserve();
        let slot = match free {
            Some(slot) => slot,
            None => match self.harvest(drawn.device, drawn.scene, true) {
                Some(Step::Running) => self
                    .readback
                    .reserve()
                    .ok_or_else(|| "the export readback ring stalled".to_string())?,
                Some(step) => return Ok(step),
                None => return Err("the export readback ring stalled".to_string()),
            },
        };
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
        drawn.renderer.present_picture(
            drawn.device,
            drawn.queue,
            &self.picture.1,
            size,
            drawn.scene.exposure,
            self.transparent,
        );
        let composed = composer.compose(
            drawn.device,
            drawn.queue,
            &self.picture.1,
            overlay,
            size,
            self.transparent,
        );
        let mut encoder = drawn
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("grana.export.readback"),
            });
        self.readback
            .stage_image(&mut encoder, slot, composed, size, self.stride);
        drawn.queue.submit([encoder.finish()]);
        self.readback.register();
        self.staged.push_back(size);
        self.frame += 1;
        self.attempts = 0;
        self.drawn = 0;
        Ok(Step::Running)
    }

    fn write_figure(
        &mut self,
        ui: &mut Ui<()>,
        drawn: &mut Drawn<'_>,
        format: Format,
    ) -> Result<PathBuf, String> {
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
        let marks = figure::marks(ui, figure);
        let transparent = self.transparent;
        let image = drawn.renderer.picture(
            drawn.device,
            drawn.queue,
            drawn.scene.exposure,
            transparent,
            |pixels| crate::quantize::rgba8(pixels, transparent),
        )?;
        let size = self.viewport.size;
        let sink = self.sink.take().expect("the export is alive");
        let Sink::Figure { path } = sink else {
            unreachable!("a vector figure writes a figure sink")
        };
        let mut page = grana_vector::Page::new(size[0] as f32, size[1] as f32);
        page.set_background(grana_vector::Image::new(size, image));
        grana_page::push(&mut page, &marks)?;
        grana_page::write_document(&path, format, &grana_vector::Document::of(page))?;
        self.frame += 1;
        self.written += 1;
        self.attempts = 0;
        self.drawn = 0;
        Ok(path)
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
