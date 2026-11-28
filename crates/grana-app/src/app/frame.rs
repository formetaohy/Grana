use std::time::Duration;

use grana_engine::Viewport;
use grana_render::{Delivery, FrameInfo};

use super::{App, STREAM_POLL};

impl App {
    pub(super) fn exchange_bricks(&mut self) -> Result<(), String> {
        let replaced = self.session.replaced() != self.replaced;
        let reloaded = self.session.content() != self.content;
        if (replaced || reloaded) && self.stage.laid_out() {
            self.replaced = self.session.replaced();
            self.content = self.session.content();
            self.sync_dataset(replaced)?;
            if replaced {
                self.needs_fit = true;
                self.camera_touched = false;
                self.workspace.reset_document();
                self.workspace.calibration = self.session.scene().and_then(|scene| {
                    grana_editor::CalibrationSheet::after_import(
                        scene,
                        self.session.access().author(),
                    )
                });
                self.mask_path.clear();
                self.pick_toggle = None;
                self.plate = None;
                self.autosave_at = None;
                self.changed_at = None;
                self.last_advance = None;
                self.last_animation = None;
            }
        }
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_mut(), engine.as_mut()) else {
            return Ok(());
        };
        let damages = session.take_damage();
        if !damages.is_empty() {
            engine.damage(&damages);
        }
        let bricks = session.take_bricks();
        if !bricks.is_empty() {
            engine.upload(&bricks);
        }
        let abandoned = session.take_abandoned();
        if !abandoned.is_empty() {
            engine.abandon(&abandoned);
        }
        let retries = session.take_retries();
        if !retries.is_empty() {
            engine.retry(&retries);
        }
        if session.take_retry_view() {
            engine.retry_all();
        }
        let time = session.scene().map_or(0, |scene| scene.time());
        if let Some(requests) =
            engine.poll_requests(surface.device(), time, session.pending_requests())?
        {
            let current = session
                .scene()
                .is_some_and(|scene| scene.time() == requests.time);
            session.request(
                time,
                requests.epoch,
                if current { &requests.volume } else { &[] },
                &requests.prefetch,
            );
        }
        let view = session.take_view();
        if let Some(view) = view {
            self.apply_view(&view);
        }
        Ok(())
    }

    pub(super) fn sync_dataset(&mut self, replaced: bool) -> Result<(), String> {
        let viewport = self.viewport();
        let size = [viewport.region[2], viewport.region[3]];
        let App {
            surface,
            engine,
            session,
            ..
        } = self;
        let (Some(surface), Some(engine)) = (surface.as_ref(), engine.as_mut()) else {
            return Ok(());
        };
        let pyramid = session.scene().map(|scene| scene.dataset.pyramid().clone());
        let Some(pyramid) = pyramid else {
            return Ok(());
        };
        if replaced || !engine.holds(&pyramid) {
            let demand = session
                .scene()
                .map_or_else(grana_render::Demand::full, grana_engine::demand);
            engine.set_dataset(surface.device(), &pyramid, surface.budget(), size, demand)?;
        } else {
            engine.invalidate(&pyramid);
        }
        Ok(())
    }

    pub(super) fn fit_view(&mut self) {
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
        Viewport::screen(self.stage.surface(), self.stage.region())
    }

    pub(super) fn render(&mut self, delivery: Delivery) -> Option<Duration> {
        let viewport = self.viewport();
        let scene = self.session.scene();
        let overlays = scene.map_or_else(Vec::new, grana_engine::overlays);
        let surfaces = self
            .session
            .surfaces()
            .iter()
            .map(|surface| {
                grana_engine::ChannelSurface::new(
                    surface.channel(),
                    surface.mesh(),
                    surface.colors(),
                    surface.revision(),
                )
            })
            .collect::<Vec<_>>();
        let outcome = {
            let App {
                surface,
                engine,
                camera,
                ..
            } = self;
            match (surface.as_ref(), engine.as_mut(), scene) {
                (Some(surface), Some(engine), Some(scene)) => engine.draw(
                    surface.device(),
                    surface.queue(),
                    grana_engine::View {
                        scene,
                        camera,
                        size: [viewport.region[2], viewport.region[3]],
                        delivery,
                        overlays: &overlays,
                        surfaces: &surfaces,
                    },
                ),
                _ => Ok(()),
            }
        };
        self.present_ui();
        match outcome {
            Ok(()) => Some(Duration::MAX),
            Err(detail) => {
                match self.surface.as_ref().and_then(|surface| surface.lost()) {
                    Some(detail) => self.fail(detail),
                    None => self
                        .set_output_message(&format!("the viewport could not be drawn: {detail}")),
                }
                Some(Duration::MAX)
            }
        }
    }

    pub(super) fn present_ui(&mut self) {
        let App {
            surface, engine, ..
        } = self;
        let Some(surface) = surface.as_mut() else {
            return;
        };
        let Some(frame) = surface.acquire() else {
            return;
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            surface
                .device()
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
        surface.queue().submit([encoder.finish()]);
        surface.present(frame);
    }

    pub(super) fn frame_info(&self) -> FrameInfo {
        self.engine
            .as_ref()
            .map_or_else(FrameInfo::default, grana_engine::Engine::info)
    }

    pub(super) fn settling(&self) -> bool {
        let info = self.frame_info();
        (info.drawn && !info.refined()) || info.fading
    }

    pub(super) fn streaming(&self) -> bool {
        let engine = self.engine.as_ref();
        let info = self.frame_info();
        self.pending_engine.is_some()
            || self.workspace.playback.playing
            || self.session.working()
            || self.session.inquiry_pending()
            || self.session.retrying()
            || engine.is_some_and(|engine| engine.requests_pending() || engine.pick_pending())
            || (self.session.scene().is_some() && !info.view.complete())
    }

    pub(super) fn stream_poll(&self) -> Duration {
        if self.session.pending() > 0 {
            return STREAM_POLL;
        }
        self.session
            .retry_wait()
            .map_or(STREAM_POLL, |wait| wait.max(STREAM_POLL))
    }
}
