use std::time::{Duration, Instant};

use grana_engine::{Display, Viewport};
use grana_render::FrameInfo;

use super::{App, CONVERGENCE_BUDGET, RETRY_INTERVAL};
use crate::stream::{Stream, View as StreamView};

impl App {
    pub(super) fn exchange_bricks(&mut self) {
        let replaced = self.session.replaced() != self.replaced;
        let reloaded = self.session.content() != self.content;
        if replaced || reloaded {
            self.replaced = self.session.replaced();
            self.content = self.session.content();
            self.sync_dataset(replaced);
            if replaced {
                self.needs_fit = true;
                self.camera_touched = false;
                self.workspace.reset_document();
                self.mask_path.clear();
                self.pick_toggle = None;
                self.plate = None;
                self.autosave_at = None;
                self.changed_at = None;
                self.last_advance = None;
                self.last_animation = None;
                self.stream = Stream::new();
            }
        }
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_mut()) else {
            return;
        };
        let damages = session.take_damage();
        if !damages.is_empty() {
            engine.damage(&damages);
        }
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
            engine.upload(&bricks);
        }
        let view = session.take_view();
        if let Some(view) = view {
            self.apply_view(&view);
        }
    }

    pub(super) fn sync_dataset(&mut self, replaced: bool) {
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_ref(), engine.as_mut()) else {
            return;
        };
        let pyramid = session.scene().map(|scene| scene.dataset.pyramid().clone());
        let Some(pyramid) = pyramid else {
            return;
        };
        if replaced || !engine.holds(&pyramid) {
            engine.set_dataset(&surface.gpu.device, &pyramid);
        } else {
            engine.invalidate(&pyramid);
        }
    }

    pub(super) fn fit_view(&mut self) {
        let scale = self
            .surface
            .as_ref()
            .map_or(1.0, |surface| surface.window.scale_factor() as f32);
        self.stage.set_pixels_per_point(scale);
        let viewport = self.viewport();
        let sized = viewport.region[2] > 64 && viewport.region[3] > 64;
        if self.needs_fit
            && sized
            && let Some(world) = self.session.scene().map(|scene| scene.dataset.world_size())
        {
            self.camera.fit(world, viewport.aspect());
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
        Viewport::screen(scale, size, self.stage.region())
    }

    pub(super) fn render(&mut self, display: Display) -> Option<Duration> {
        let viewport = self.viewport();
        let cap = self.level_cap();
        let budget = self.capture.is_none().then_some(CONVERGENCE_BUDGET);
        let overlays = super::overlay::overlays(&self.session);
        let scene = self.session.scene();
        let outcome = {
            let App {
                surface,
                engine,
                camera,
                ..
            } = self;
            match (surface.as_ref(), engine.as_mut(), scene) {
                (Some(surface), Some(engine), Some(scene)) => engine.draw(
                    &surface.gpu.device,
                    &surface.gpu.queue,
                    grana_engine::View {
                        scene,
                        camera,
                        size: [viewport.region[2], viewport.region[3]],
                        display,
                        budget,
                        level_cap: cap,
                        overlays: &overlays,
                    },
                ),
                _ => Ok(()),
            }
        };
        self.present_ui();
        match outcome {
            Ok(()) => Some(Duration::MAX),
            Err(_) => Some(RETRY_INTERVAL),
        }
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
        let Some(surface) = surface.as_mut() else {
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
        if let Some(engine) = engine.as_ref() {
            surface.ui.set_scene_revision(engine.image_revision());
        }
        surface.ui.render(
            &mut encoder,
            &view,
            engine.as_ref().and_then(grana_engine::Engine::view),
        );
        surface.gpu.queue.submit([encoder.finish()]);
        surface.gpu.queue.present(frame);
    }

    pub(super) fn frame_info(&self) -> FrameInfo {
        self.engine
            .as_ref()
            .map_or_else(FrameInfo::default, grana_engine::Engine::info)
    }

    pub(super) fn converging(&self) -> bool {
        let info = self.frame_info();
        (info.samples > 0 && !info.converged) || info.fading
    }

    pub(super) fn streaming(&self) -> bool {
        let engine = self.engine.as_ref();
        let info = self.frame_info();
        self.pending_engine.is_some()
            || self.session.batch().is_some_and(|batch| batch.running)
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
