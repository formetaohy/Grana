use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_engine::RenderedScene;
use grana_gpu::Headless;
use grana_render::{EXPORT_CEILING, Renderer};
use grana_scene::Scene;
use grana_ui::Ui;
use wgpu::{Device, Queue, TextureFormat};

use crate::compose::Composer;
use crate::export::{Export, Step};
use crate::plan::{self, Destination};
use crate::task::{Bricks, Task};

const TEXTURE_BUDGET: u64 = 512 << 20;

pub enum Outcome {
    Complete { path: PathBuf },
    Failed { detail: String },
    Cancelled,
}

pub struct Offline {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    renderer: Renderer,
    composer: Composer,
}

impl Offline {
    pub fn new(device: Device, queue: Queue, format: TextureFormat) -> Self {
        let renderer = Renderer::new(&device, &queue, format, TEXTURE_BUDGET);
        let composer = Composer::new(&device);
        Self {
            device,
            queue,
            format,
            renderer,
            composer,
        }
    }

    pub fn headless() -> Result<Self, String> {
        let Headless { device, queue } = Headless::new()?;
        Ok(Self::new(device, queue, grana_gpu::HEADLESS_FORMAT))
    }

    pub fn run(
        &mut self,
        task: Task,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(u32, u32),
    ) -> Outcome {
        let Task {
            plan,
            mut scene,
            bricks,
        } = task;
        scene.region = plan.region;
        if let Err(detail) = plan::validate(&plan) {
            return Outcome::Failed { detail };
        }
        if let Destination::Lineage { path, set } = &plan.destination {
            progress(0, 1);
            let outcome = match crate::lineage::write(
                &self.device,
                &self.queue,
                self.format,
                &scene,
                *set,
                path,
                plan.size.to_array(),
            ) {
                Ok(path) => Outcome::Complete { path },
                Err(detail) => Outcome::Failed { detail },
            };
            if matches!(outcome, Outcome::Complete { .. }) {
                progress(1, 1);
            }
            return outcome;
        }
        let pyramid = scene.dataset.pyramid().clone();
        let needed = match plan::residency(&plan, &pyramid) {
            Ok(needed) => needed,
            Err(detail) => return Outcome::Failed { detail },
        };
        self.renderer.set_budget(TEXTURE_BUDGET);
        self.renderer.set_dataset(&self.device, &pyramid);
        if let Err(detail) =
            self.renderer
                .reserve(&self.device, &pyramid, needed.slots, EXPORT_CEILING)
        {
            return Outcome::Failed { detail };
        }
        let first = self.renderer.first_level();
        if first > needed.level {
            return Outcome::Failed {
                detail: format!(
                    "an export at this size needs level {} but the renderer addresses levels {first} and coarser",
                    needed.level
                ),
            };
        }
        self.renderer.begin_export();
        let mut export = match Export::start(&self.device, self.format, plan) {
            Ok(export) => export,
            Err(detail) => return Outcome::Failed { detail },
        };
        let mut ui: Ui<()> = Ui::new(&self.device, &self.queue, self.format);
        ui.resize(export.size());
        progress(0, export.progress().1);
        let mut prepared: Option<(u32, Vec<usize>, RenderedScene)> = None;
        let mut revision = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                export.discard();
                return Outcome::Cancelled;
            }
            let pose = export.pose();
            scene.set_time(pose.time);
            let detail = grana_engine::detail(&scene, export.lens());
            let stale = prepared
                .as_ref()
                .is_none_or(|(time, cached, _)| *time != pose.time || *cached != detail);
            if stale {
                revision += 1;
                match grana_engine::frame(&scene, revision, &detail) {
                    Ok(rendered) => prepared = Some((pose.time, detail, rendered)),
                    Err(detail) => {
                        export.discard();
                        return Outcome::Failed { detail };
                    }
                }
            }
            if let Err(detail) = self.pump(&bricks, &scene) {
                export.discard();
                return Outcome::Failed { detail };
            }
            let rendered = &prepared.as_ref().expect("the prepared scene is ready").2;
            match export.render(
                &mut self.renderer,
                crate::export::Context {
                    device: &self.device,
                    queue: &self.queue,
                    ui: &mut ui,
                    composer: &mut self.composer,
                },
                &scene,
                rendered,
            ) {
                Step::Running => {}
                Step::Complete(path) => {
                    let (frame, total) = export.progress();
                    progress(frame, total);
                    return Outcome::Complete { path };
                }
                Step::Failed(detail) => return Outcome::Failed { detail },
            }
            let (frame, total) = export.progress();
            progress(frame, total);
        }
    }

    pub fn render(&mut self, task: Task) -> Outcome {
        let cancel = AtomicBool::new(false);
        self.run(task, &cancel, &mut |_, _| {})
    }

    fn pump(&mut self, bricks: &Arc<dyn Bricks>, scene: &Scene) -> Result<(), String> {
        let Some(requests) = self.renderer.poll_requests(&self.device) else {
            return Ok(());
        };
        if requests.time != scene.time() {
            return Ok(());
        }
        let mut uploaded = Vec::with_capacity(requests.volume.len() + requests.prefetch.len());
        for request in requests.volume.iter().chain(&requests.prefetch) {
            let data = bricks.read(*request, scene.drift().offset(request.time))?;
            uploaded.push((*request, data));
        }
        self.renderer.upload(&self.queue, &uploaded);
        Ok(())
    }
}
