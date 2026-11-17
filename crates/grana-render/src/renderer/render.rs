use super::allocate::{self, Needed};
use super::draw;
use super::plan::{self, Adapt};
use super::resolve;
use super::signature::{ShadowKeys, channel_style};
use super::timing::{self, Begin};
use super::view::View;
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Shot {
    view: View,
    awaits: bool,
    assured: bool,
    slice: bool,
    translucent: bool,
    surfaces: bool,
    splats: bool,
    volume: bool,
    temporal: Option<Temporal>,
}

impl Shot {
    fn of(frame: &Frame<'_, '_>) -> Self {
        let slice = frame.camera.slice().is_some();
        let slice_objects =
            slice && (!frame.scene.layers.is_empty() || !frame.scene.splats.is_empty());
        Self {
            view: View::new(
                frame.viewport,
                frame.size,
                frame.camera,
                frame.region,
                frame.time,
            ),
            awaits: frame.display.awaits(),
            assured: frame.display.assures(),
            slice,
            translucent: !slice
                && (frame
                    .scene
                    .styles
                    .iter()
                    .any(|style| style.visible() && style.transparent())
                    || !frame.scene.splats.is_empty()),
            surfaces: !frame.scene.layers.is_empty(),
            splats: !frame.scene.splats.is_empty(),
            volume: frame
                .scene
                .channels
                .iter()
                .any(|row| row.visible && row.opacity > 0.0),
            temporal: frame.temporal.filter(|_| !slice_objects),
        }
    }
}

struct Prepared {
    shot: Shot,
    levels: Levels,
    ladder: Ladder,
    data: FrameData,
    view: FrameView,
    started: timing::Started,
    reserved: Option<usize>,
    residency: ViewKey,
    shadow: ShadowKeys,
    replan: bool,
    rebuilt: bool,
    ready: bool,
}

enum Halt {
    Blank,
    Hold(FrameInfo),
    Fail(String),
}

impl Renderer {
    pub(super) fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        target: &mut FrameTarget,
        frame: Frame<'_, '_>,
    ) -> Result<FrameInfo, String> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.frame"),
        });
        let shot = Shot::of(&frame);
        let mut prepared = match self.prepare(device, queue, &mut encoder, target, &frame, shot) {
            Ok(prepared) => prepared,
            Err(Halt::Blank) => {
                blank(&mut encoder, frame.target);
                queue.submit([encoder.finish()]);
                return Ok(self.info);
            }
            Err(Halt::Hold(info)) => {
                queue.submit([encoder.finish()]);
                return Ok(info);
            }
            Err(Halt::Fail(detail)) => return Err(detail),
        };
        self.record(device, queue, &mut encoder, target, &frame, &mut prepared);
        self.resolve(device, queue, &mut encoder, target, &frame, &prepared);
        queue.submit([encoder.finish()]);
        self.register();
        Ok(self.info)
    }

    fn register(&mut self) {
        if let Some(convergence) = self.convergence.as_mut() {
            convergence.register();
        }
        self.residence.register();
    }

    fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: &mut FrameTarget,
        frame: &Frame<'_, '_>,
        shot: Shot,
    ) -> Result<Prepared, Halt> {
        let view = shot.view;
        self.time = frame.time;
        self.prefetch = frame.display.prefetches();
        self.viewport = Some(frame.viewport);
        self.present_region = Some(view.region);
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        self.motion = Some(view.motion);
        let Self {
            layouts,
            present,
            resources,
            gtao,
            meshes,
            splats,
            sections,
            sliced,
            convergence,
            temporal: temporal_pass,
            fade,
            exporting,
            info,
            residence,
            dummy_normal,
            dummy_volume,
            budget,
            fragments,
            pipelines,
            ..
        } = self;
        let Some(resources) = resources.as_mut() else {
            return Err(Halt::Blank);
        };
        let scene = frame.scene;
        assert_eq!(
            scene.channels.len(),
            resources.channels,
            "the scene carries one row per channel"
        );
        assert!(
            frame.time < resources.pyramid.frame_count(),
            "the requested frame lies inside the time series"
        );
        *sliced = shot.slice;
        sections.prepare(
            device,
            queue,
            layouts,
            scene,
            frame
                .camera
                .slice()
                .map(|slice| slice.plane(frame.camera.pivot())),
        );
        resources.atlas.prune([
            frame.time,
            carried_frame(frame.time, resources.pyramid.frame_count()),
        ]);
        let outlook = residence.adapt(Adapt {
            pyramid: &resources.pyramid,
            camera: frame.camera,
            region: frame.region,
            view,
            arena: &resources.arena,
            slots: resources.atlas.slots(),
            size: frame.size,
            time: frame.time,
            versions: scene.versions,
            awaits: shot.awaits,
            assured: shot.assured,
            prefetch: frame.display.prefetches(),
            overflow: info.view.visible > info.view.demand,
        });
        let channel = channel_style(scene.channels);
        if residence.style_outdated(channel) {
            resources.scene.publish(
                device,
                queue,
                &mut resources.upload,
                &resources.pyramid,
                scene.channels,
            );
            residence.set_style(channel);
        }
        let stale = residence.passes_outdated(scene.versions);
        info.settled = outlook.settled;
        info.view = residence.view();
        info.view.level = outlook.ladder.base;
        info.truncated = outlook.truncated;
        plan::prepare(plan::Tables {
            bricks: &mut resources.bricks,
            pyramid: &resources.pyramid,
            levels: outlook.levels,
            atlas: &resources.atlas,
            arena: &resources.arena,
            upload: &mut resources.upload,
            time: frame.time,
            content: residence.content(),
        });
        resources.upload.encode(device, queue, encoder);
        let tables = plan::tables(&resources.bricks, outlook.levels);
        let (step, ray_bounds) =
            match plan::ray(frame.camera, tables[0], scene.step_scale, view.height) {
                Ok(ray) => ray,
                Err(detail) => return Err(Halt::Fail(detail)),
            };
        let reserved = residence.reserve(outlook.replan);
        if shot.awaits && !outlook.ready && reserved.is_none() {
            target.samples = 0;
            info.samples = 0;
            info.rebuilt = false;
            info.fading = false;
            return Err(Halt::Hold(*info));
        }
        let fragment = match allocate::targets(
            device,
            layouts,
            gtao,
            encoder,
            target,
            Needed {
                surfaces: shot.surfaces,
                slice: shot.slice,
                volume: shot.volume,
                translucent: shot.translucent,
                fragments: *fragments,
            },
        ) {
            Ok(fragment) => fragment,
            Err(detail) => return Err(Halt::Fail(detail)),
        };
        info.fragment = fragment;
        let planned = plan::shadow(plan::ShadowFrame {
            scene,
            pyramid: &resources.pyramid,
            ladder: outlook.ladder,
            bricks: &resources.bricks,
            atlas: &resources.atlas,
            versions: scene.versions,
            summary: &mut resources.summary,
            channel,
            time: frame.time,
        });
        let shadow = &mut resources.shadow;
        let voxel = resources.pyramid.level(planned.level).voxel();
        let unit = voxel.into_iter().fold(f32::INFINITY, f32::min);
        shadow.resize(device, layouts, shadow::extent(&planned.space, unit));
        let mut data = plan::data(plan::Frame {
            camera: frame.camera,
            region: frame.region,
            clip: frame.clip,
            view,
            time: frame.time,
            ladder: outlook.ladder,
            tables: &tables,
            scene,
            ray: crate::frame::Ray {
                step,
                bounds: ray_bounds,
                channels: resources.channels,
            },
            space: planned.space,
            translucent: shot.translucent,
            awaits: shot.awaits,
            content: residence.content(),
            table: &resources.scene,
            oit: target.oit.as_ref(),
        });
        let signature = Signature {
            data: &data,
            camera: frame.camera.state(),
            region: view.region,
            levels: outlook.levels,
            versions: scene.versions,
            channels: channel,
            content: residence.content(),
            generation: residence.generation(),
            time: frame.time,
            epoch: residence.epoch(),
            size: target.size,
            overlay: crate::schema::overlay_digest(scene.overlays),
        };
        let started = timing::begin(Begin {
            target,
            sampling: frame.sampling,
            budget: frame.budget,
            levels: LevelKey::of(outlook.levels),
            transition: signature.transition(),
            structure: signature.digest(),
            shot: signature.shot(),
            temporal: shot.temporal,
            sequence,
            fade,
            exporting: *exporting,
            assured: shot.assured,
        });
        if started.snapshot {
            target.fade_group(device, present);
            target.snapshot(encoder, view.region);
            *fade = Some(Fade { weight: 0.0 });
        }
        if started.reset {
            if shot.temporal.is_some() {
                temporal_pass.ensure(&mut target.history, device, target.size);
            }
            if started.continuing {
                let geometry =
                    target.geometry(device, layouts, dummy_normal, dummy_volume, shot.volume);
                resolve::capture(
                    temporal_pass,
                    encoder,
                    resolve::Capture {
                        device,
                        history: target.history.as_mut().expect("the history is allocated"),
                        previous: target.view,
                        scene: resources.scene.group(),
                        geometry: &geometry,
                        accumulated: &target.accumulation_view,
                        counts: &target.count_view,
                        fresh: target.scene.color(),
                    },
                );
            } else {
                TemporalPass::invalidate(&mut target.history);
            }
            if let Some(convergence) = convergence.as_mut() {
                convergence.reset();
            }
            if stale {
                if shot.surfaces {
                    meshes
                        .get_or_insert_with(|| {
                            MeshPasses::new(device, layouts, *budget / 2, pipelines)
                        })
                        .set(device, queue, layouts, scene);
                } else if let Some(passes) = meshes.as_mut() {
                    passes.clear();
                }
                if shot.splats {
                    splats
                        .get_or_insert_with(|| SplatPasses::new(device, layouts, pipelines))
                        .set(device, queue, layouts, scene.splats);
                } else if let Some(passes) = splats.as_mut() {
                    passes.clear();
                }
                residence.load_passes(scene.versions);
            }
            if let Some(passes) = splats.as_mut() {
                passes.rewind(queue);
                passes.cull_volume(encoder, resources.scene.group());
            }
            if !shot.slice
                && let Some(passes) = meshes.as_mut()
            {
                passes.cull_volume(encoder, resources.scene.group());
                if scene.shading.shadows > 0.0 {
                    passes.cull_shadow(encoder, resources.scene.group());
                }
            }
        }
        let view_now = FrameView::of(&data, view.region);
        data.jitter = [0.0; 4];
        Ok(Prepared {
            shot,
            levels: outlook.levels,
            ladder: outlook.ladder,
            data,
            view: view_now,
            started,
            reserved,
            residency: outlook.key,
            shadow: planned.keys,
            replan: outlook.replan,
            rebuilt: started.reset && stale,
            ready: outlook.ready,
        })
    }

    fn record(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: &mut FrameTarget,
        frame: &Frame<'_, '_>,
        prepared: &mut Prepared,
    ) {
        let Self {
            layouts,
            present,
            resources,
            volume,
            clear,
            surface: surface_pass,
            oit_resolve,
            gtao: _,
            meshes,
            splats,
            convergence,
            temporal: temporal_pass,
            info,
            sections,
            overlay,
            dummy_oit,
            dummy_ao,
            dummy_normal,
            dummy_volume,
            pipelines,
            ..
        } = self;
        let Some(resources) = resources.as_mut() else {
            return;
        };
        let scene = frame.scene;
        let view = prepared.shot.view;
        let started = prepared.started;
        let data = &mut prepared.data;
        if let Some(convergence) = convergence.as_mut()
            && let Some(estimate) = convergence.poll(device, target.samples)
        {
            target.estimate = Some(estimate);
        }
        let resolved = frame.sampling.met(target.estimate);
        info.samples = target.samples;
        info.rebuilt = prepared.rebuilt;
        let offset = sample::offset(started.sample, [view.region[2], view.region[3]]);
        data.jitter = [
            offset.x,
            offset.y,
            started.sample as f32,
            scene.step_scale.max(1.0e-3),
        ];
        data.lens = crate::frame::lens(
            frame.camera,
            started.sample,
            frame.sampling.ceiling(),
            view.aspect,
        );
        data.tag[2] = splats.as_ref().map_or(0, SplatPasses::count);
        resources.scene.write_frame(queue, *data);
        if prepared.replan {
            let tables = plan::tables(&resources.bricks, prepared.levels);
            plan::schedule(
                &mut resources.planner,
                queue,
                frame.camera,
                &tables,
                prepared.levels,
                frame.time,
            );
            resources.planner.encode(encoder, resources.scene.group());
        }
        let drawing =
            target.samples < frame.sampling.ceiling() && prepared.ready && !started.expired;
        if drawing {
            target.samples += 1;
            info.samples = target.samples;
            let content = data.tag[3];
            if prepared.shot.volume && !prepared.shot.slice {
                let mut ladder_levels = [0usize; BAND];
                let mut count = 0;
                for level in prepared.ladder.levels() {
                    ladder_levels[count] = level;
                    count += 1;
                }
                let entries = resources.baked.collect(
                    &resources.atlas,
                    &ladder_levels[..count],
                    frame.time,
                    content,
                    resources.bake.capacity(),
                );
                if !entries.is_empty() {
                    resources.bake.stage(queue, entries);
                    resources.bake.encode(
                        encoder,
                        entries.len() as u32,
                        resources.scene.group(),
                        &resources.level_group,
                        resources.atlas.source(),
                    );
                }
            }
            if let Some(passes) = meshes.as_mut() {
                passes.write_uniforms(queue, data, scene.shading);
            }
            draw::encode(draw::Stage {
                device,
                queue,
                layouts,
                encoder,
                target,
                scene,
                table: &resources.scene,
                groups: draw::Groups {
                    level: &resources.level_group,
                    atlas: resources.atlas.group(),
                    occupancy: &resources.occupancy_group,
                },
                shadow: Some(&mut resources.shadow),
                keys: prepared.shadow,
                volume_pass: volume,
                clear,
                surface: surface_pass,
                oit_resolve,
                meshes: meshes.as_ref(),
                splats: splats.as_ref(),
                sections,
                overlay,
                data,
                features: draw::Features {
                    region: view.region,
                    slice: prepared.shot.slice,
                    translucent: prepared.shot.translucent,
                    volume: prepared.shot.volume,
                    surfaces: prepared.shot.surfaces,
                    ambient: started.reset,
                },
                sample: started.sample,
                dummy_ao,
                dummy_oit,
                dummy_normal,
                dummy_volume,
            });
            if frame.sampling.measuring() && target.samples > 1 {
                resolve::measure(
                    encoder,
                    resolve::Measure {
                        device,
                        queue,
                        pipelines,
                        convergence,
                        sampled: crate::converge::Sampled {
                            region: view.region,
                            color: target.scene.color(),
                            accumulation: &target.accumulation_view,
                        },
                        samples: target.samples,
                    },
                );
            }
            resolve::accumulate(
                encoder,
                resolve::Accumulate {
                    clear,
                    present,
                    target,
                    region: view.region,
                    reset: started.reset,
                },
            );
            if started.continuing && target.samples == 1 {
                let request = prepared
                    .shot
                    .temporal
                    .expect("the seeding frame carries a history request");
                let spacing = resources.pyramid.level(prepared.ladder.base).spacing();
                let tolerance =
                    crate::temporal::tolerance(spacing.x().max(spacing.y()).max(spacing.z()));
                let geometry = target.geometry(
                    device,
                    layouts,
                    dummy_normal,
                    dummy_volume,
                    prepared.shot.volume,
                );
                resolve::seed(
                    temporal_pass,
                    encoder,
                    resolve::Seed {
                        device,
                        queue,
                        history: target.history.as_ref().expect("the history is captured"),
                        view: &prepared.view,
                        length: request.history(),
                        tolerance,
                        scene: resources.scene.group(),
                        geometry: &geometry,
                        accumulated: &target.accumulation_view,
                        counts: &target.count_view,
                        fresh: target.scene.color(),
                    },
                );
                target.seeded = true;
            }
            target.holds = true;
        } else if !prepared.ready {
            target.samples = 0;
            info.samples = 0;
        }
        info.converged = prepared.ready
            && (target.samples >= frame.sampling.ceiling() || resolved || started.expired);
    }

    fn resolve(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: &mut FrameTarget,
        frame: &Frame<'_, '_>,
        prepared: &Prepared,
    ) {
        let Self {
            present,
            resources,
            transparent,
            fade,
            info,
            residence,
            ..
        } = self;
        if prepared.ready {
            resolve::present(
                encoder,
                resolve::Presenting {
                    device,
                    queue,
                    present,
                    target,
                    surface: frame.target,
                    viewport: prepared.shot.view.region,
                    region: prepared.shot.view.region,
                    exposure: frame.exposure,
                    transparent: *transparent,
                    fade,
                },
            );
        }
        info.fading = prepared.ready && fade.is_some();
        if let Some(slot) = prepared.reserved
            && let Some(resources) = resources.as_ref()
        {
            residence.stage(encoder, slot, &resources.planner, prepared.residency);
        }
        info.seeded = target.seeded;
        info.history = prepared.shot.temporal.is_some()
            && target
                .history
                .as_ref()
                .is_some_and(crate::temporal::History::ready);
        target.view = prepared.view;
    }
}

fn blank(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.blank"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}
