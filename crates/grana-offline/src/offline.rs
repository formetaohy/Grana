use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grana_engine::RenderedScene;
use grana_gpu::{Consumer, Headless, Pipelines};
use grana_render::{EXPORT_CEILING, FrameBudget, Renderer};
use grana_scene::Scene;
use grana_ui::Ui;
use grana_volume::PackedBrick;
use wgpu::{Device, Queue, TextureFormat};

use crate::compose::Composer;
use crate::export::{Export, Footprint, Step};
use crate::plan::{self, Destination};
use crate::probe;
use crate::task::{Bricks, Task};

const READ_BYTES: u64 = 128 << 20;

pub enum Outcome {
    Complete { path: PathBuf },
    Failed { detail: String },
    Cancelled,
}

pub struct Offline {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    ledger: Arc<grana_gpu::Ledger>,
    budget: u64,
    renderer: Renderer,
    composer: Composer,
    pipelines: Pipelines,
}

impl Offline {
    pub fn new(
        device: Device,
        queue: Queue,
        format: TextureFormat,
        ledger: &Arc<grana_gpu::Ledger>,
        budget: u64,
        pipelines: &Pipelines,
    ) -> Self {
        let renderer = Renderer::new(&device, &queue, format, ledger, budget, pipelines);
        let composer = Composer::new(&device, pipelines);
        Self {
            device,
            queue,
            format,
            ledger: Arc::clone(ledger),
            budget,
            renderer,
            composer,
            pipelines: pipelines.clone(),
        }
    }

    pub fn headless() -> Result<Self, String> {
        let headless = Headless::shared()?;
        let budget = grana_render::texture_budget(headless.memory());
        let pipelines = headless.pipelines().clone();
        let ledger = Arc::clone(headless.graphics().ledger());
        Ok(Self::new(
            headless.device,
            headless.queue,
            grana_gpu::HEADLESS_FORMAT,
            &ledger,
            budget,
            &pipelines,
        ))
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
            surfaces,
        } = task;
        scene.region = plan.region;
        if let Err(detail) = plan::validate(&plan) {
            return Outcome::Failed { detail };
        }
        if let Some(written) = crate::page::write(&plan) {
            progress(0, 1);
            let outcome = match written {
                Ok(path) => {
                    progress(1, 1);
                    Outcome::Complete { path }
                }
                Err(detail) => Outcome::Failed { detail },
            };
            return outcome;
        }
        if let Destination::Lineage { path, set } = &plan.destination {
            progress(0, 1);
            let surface = crate::surface::Surface {
                device: &self.device,
                queue: &self.queue,
                format: self.format,
                pipelines: &self.pipelines,
            };
            let outcome =
                match crate::lineage::write(&surface, &scene, *set, path, plan.size.to_array()) {
                    Ok(path) => Outcome::Complete { path },
                    Err(detail) => Outcome::Failed { detail },
                };
            if matches!(outcome, Outcome::Complete { .. }) {
                progress(1, 1);
            }
            return outcome;
        }
        let pyramid = scene.dataset.pyramid().clone();
        let size = plan.size.to_array();
        let needed = match plan::residency(&plan, &pyramid) {
            Ok(needed) => needed,
            Err(detail) => return Outcome::Failed { detail },
        };
        let footprint = match Footprint::of(&self.device, self.format, size) {
            Ok(footprint) => footprint,
            Err(detail) => return Outcome::Failed { detail },
        };
        let claim = match self.ledger.claim(Consumer::Export, footprint.bytes()) {
            Some(claim) => claim,
            None => {
                return Outcome::Failed {
                    detail: format!(
                        "a {} × {} export needs {:.1} MiB of export textures and the device allowance holds {:.1} MiB; lower the export size",
                        size[0],
                        size[1],
                        footprint.bytes() as f64 / (1u64 << 20) as f64,
                        self.ledger.total() as f64 / (1u64 << 20) as f64
                    ),
                };
            }
        };
        let ceiling =
            EXPORT_CEILING.min(self.ledger.spare().saturating_add(self.renderer.claimed()));
        let mut demand = grana_engine::demand(&scene);
        demand.transparent |= plan.transparent;
        let samples = match demand.volume {
            true => crate::export::SAMPLES,
            false => 1,
        };
        let frame = FrameBudget { size, demand };
        let room = match self.renderer.capacity(
            &self.device,
            &pyramid,
            self.budget,
            grana_render::Detail {
                level: needed.detail.level,
                bricks: usize::MAX / 4,
            },
            frame,
            ceiling,
        ) {
            Ok(room) => room,
            Err(detail) => return Outcome::Failed { detail },
        };
        let mut detail = needed.detail;
        let mut hollow = Vec::new();
        let mut bricks_held = probe::Cache::empty();
        if room < detail.bricks {
            let scanned = match probe::scan(probe::Scan {
                plan: &plan,
                pyramid: &pyramid,
                scene: &scene,
                bricks: bricks.as_ref(),
                limit: room,
                cancel,
                progress,
            }) {
                Ok(Some(scanned)) => scanned,
                Ok(None) => return Outcome::Cancelled,
                Err(detail) => return Outcome::Failed { detail },
            };
            detail = scanned.detail;
            detail.bricks = detail.bricks.min(needed.detail.bricks);
            hollow = scanned.hollow;
            bricks_held = scanned.cache;
        }
        if let Err(detail) =
            self.renderer
                .reserve(&self.device, &pyramid, self.budget, detail, frame, ceiling)
        {
            return Outcome::Failed { detail };
        }
        self.renderer.seed_hollow(hollow);
        let first = self.renderer.first_level();
        if first > needed.detail.level {
            return Outcome::Failed {
                detail: format!(
                    "an export at this size needs level {} but the renderer addresses levels {first} and coarser",
                    needed.detail.level
                ),
            };
        }
        self.renderer.begin_export();
        let mut export = match Export::start(
            &self.device,
            self.format,
            plan,
            needed.levels,
            samples,
            claim,
            footprint,
        ) {
            Ok(export) => export,
            Err(detail) => return Outcome::Failed { detail },
        };
        let mut ui: Ui<()> = Ui::new(&self.device, &self.queue, self.format, &self.pipelines);
        ui.resize(export.size());
        progress(0, export.progress().1);
        let mut prepared: Option<(u32, grana_engine::Targets, RenderedScene)> = None;
        let mut cache = grana_engine::Cache::new();
        loop {
            if cancel.load(Ordering::Relaxed) {
                export.discard();
                return Outcome::Cancelled;
            }
            let pose = export.pose();
            scene.set_time(pose.time);
            let targets = grana_engine::plan_with(&scene, export.lens(), &mut cache);
            let stale = prepared
                .as_ref()
                .is_none_or(|(time, cached, _)| *time != pose.time || *cached != targets);
            if stale {
                let budget = self.renderer.splat_budget();
                let views = surfaces
                    .iter()
                    .map(grana_engine::Surface::view)
                    .collect::<Vec<_>>();
                match grana_engine::frame(&scene, &targets, &mut cache, budget, &views) {
                    Ok(rendered) => prepared = Some((pose.time, targets, rendered)),
                    Err(detail) => {
                        export.discard();
                        return Outcome::Failed { detail };
                    }
                }
            }
            if let Err(detail) = self.pump(&bricks, &scene, &mut bricks_held) {
                export.discard();
                return Outcome::Failed { detail };
            }
            if let Some(detail) = self.renderer.degraded() {
                export.discard();
                return Outcome::Failed {
                    detail: detail.to_string(),
                };
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

    fn pump(
        &mut self,
        bricks: &Arc<dyn Bricks>,
        scene: &Scene,
        held: &mut probe::Cache,
    ) -> Result<(), String> {
        let Some(requests) = self.renderer.poll_requests(&self.device, &HashSet::new())? else {
            return Ok(());
        };
        if requests.time != scene.time() {
            return Ok(());
        }
        let pyramid = scene.dataset.pyramid();
        let brick = pyramid.brick_bytes().max(1);
        let mut bytes = 0u64;
        let mut uploaded = Vec::with_capacity(requests.volume.len() + requests.prefetch.len());
        for request in requests.volume.iter().chain(&requests.prefetch) {
            if let Some(packed) = held.take(request) {
                uploaded.push((*request, packed));
                continue;
            }
            if bytes.saturating_add(brick) > READ_BYTES && !uploaded.is_empty() {
                break;
            }
            let data = bricks.read(*request, &scene.drift_warps(request.time))?;
            bytes += brick;
            uploaded.push((*request, PackedBrick::of(&data, pyramid, request.channels)));
        }
        if uploaded.is_empty() {
            return Ok(());
        }
        self.renderer.upload(&uploaded);
        Ok(())
    }
}
