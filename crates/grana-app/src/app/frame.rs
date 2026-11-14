use std::time::{Duration, Instant};

use grana_engine::{Display, SAMPLE_LIMIT, Viewport};
use grana_render::FrameInfo;

use super::App;
use crate::stream::View as StreamView;
impl App {
    pub(super) fn exchange_bricks(&mut self) {
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_mut()) else {
            return;
        };
        let time = session.scene().map_or(0, |scene| scene.time());
        if let Some(requests) = engine.poll_requests(&surface.gpu.device, time) {
            let current = session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time);
            if current {
                let mut bricks =
                    Vec::with_capacity(requests.volume.len() + requests.prefetch.len());
                bricks.extend_from_slice(&requests.volume);
                bricks.extend_from_slice(&requests.prefetch);
                session.request(requests.time, &bricks);
            }
        }
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            engine.upload(&surface.gpu.queue, &bricks);
        }
        let replaced = session.replaced() != self.replaced;
        let reloaded = session.content() != self.content;
        if replaced || reloaded {
            self.replaced = session.replaced();
            self.content = session.content();
            if let Some(scene) = session.scene() {
                engine.set_dataset(&surface.gpu.device, scene.dataset.pyramid());
            }
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

    pub(super) fn fit_view(&mut self) {
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

    pub(super) fn viewport(&self) -> Viewport {
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

    pub(super) fn render(&mut self, display: Display) -> Option<Duration> {
        let viewport = self.viewport();
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
        let overlays = super::overlay::overlays(session);
        let outcome = match session.scene() {
            Some(scene) => engine.draw(
                &surface.gpu.device,
                &surface.gpu.queue,
                grana_engine::View {
                    scene,
                    camera,
                    size: [viewport.region[2], viewport.region[3]],
                    display,
                    level_cap: cap,
                    overlays: &overlays,
                },
            ),
            None => Ok(()),
        };
        if let Err(detail) = outcome {
            *output = Some(detail);
            self.present_ui();
            return None;
        }
        self.present_ui();
        Some(Duration::MAX)
    }

    pub(super) fn level_cap(&mut self) -> usize {
        let Some((levels, brick_bytes)) = self.session.scene().map(|scene| {
            (
                scene.dataset.level_count(),
                scene.dataset.pyramid().brick_bytes(),
            )
        }) else {
            return 0;
        };
        let info = self.frame_info();
        let view = StreamView {
            remote: self.session.remote(),
            levels,
            rate: self.session.throughput(),
            bricks: info.view.demand,
            rendered: info.view.level,
            brick_bytes,
            now: Instant::now(),
        };
        self.stream.cap(view)
    }

    pub(super) fn present_ui(&mut self) {
        let App {
            surface, engine, ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_ref()) else {
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
        surface.ui.set_scene_revision(engine.image_revision());
        surface.ui.render(&mut encoder, &view, engine.view());
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
        surface.gpu.pipelines().save();
    }

    pub(super) fn frame_info(&self) -> FrameInfo {
        self.engine
            .as_ref()
            .map_or_else(FrameInfo::default, grana_engine::Engine::info)
    }

    pub(super) fn converging(&self) -> bool {
        let info = self.frame_info();
        (info.samples > 0 && info.samples < SAMPLE_LIMIT) || info.fading
    }

    pub(super) fn streaming(&self) -> bool {
        let engine = self.engine.as_ref();
        let info = self.frame_info();
        self.session.batch().is_some_and(|batch| batch.running)
            || self.workspace.playback.playing
            || self.session.loading().is_some()
            || self.session.previewing()
            || self.session.transferring()
            || self.session.writing()
            || !self.session.progress().is_empty()
            || self.session.pending() > 0
            || self.session.measuring()
            || self.session.listing()
            || self.session.materializing()
            || self.session.plotting()
            || engine.is_some_and(|engine| engine.requests_pending() || engine.pick_pending())
            || (self.session.scene().is_some() && info.view.resident < info.view.demand)
    }
}
