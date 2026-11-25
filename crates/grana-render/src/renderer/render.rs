use super::allocate::{self, Needed};
use super::draw;
use super::plan::{self, Adapt};
use super::resolve;
use super::signature::channel_style;
use super::timing::{self, Begin};
use super::view::View;
use super::*;
use crate::jitter;
use crate::temporal;
use glam::Mat4;

#[derive(Clone, Copy)]
pub(super) struct Shot {
    pub(super) view: View,
    pub(super) awaits: bool,
    pub(super) assured: bool,
    pub(super) stale: bool,
    pub(super) slice: bool,
    pub(super) transparent: bool,
    pub(super) translucent: bool,
    pub(super) surfaces: bool,
    pub(super) splats: bool,
    pub(super) volume: bool,
}

impl Shot {
    pub(super) fn of(frame: &Frame<'_, '_>, transparent: bool) -> Self {
        let slice = frame.camera.slice().is_some();
        Self {
            view: View::new(
                frame.viewport,
                frame.size,
                frame.camera,
                frame.region,
                frame.time,
            ),
            awaits: frame.delivery.awaits(),
            assured: frame.delivery.assures(),
            stale: frame.delivery.shows_stale(),
            slice,
            transparent,
            translucent: transparent && !slice,
            surfaces: !frame.scene.layers.is_empty(),
            splats: !frame.scene.splats.is_empty(),
            volume: frame
                .scene
                .channels
                .iter()
                .any(|row| row.visible && row.opacity > 0.0),
        }
    }

    pub(super) fn demand(self) -> budget::FrameDemand {
        budget::FrameDemand {
            surfaces: self.surfaces,
            volume: self.volume,
            transparent: self.transparent,
        }
    }
}

struct Prepared {
    shot: Shot,
    levels: Levels,
    ladder: Ladder,
    data: FrameData,
    started: timing::Started,
    reserved: Option<usize>,
    residency: ViewKey,
    shadow: plan::ShadowPlan,
    replan: bool,
    rebuilt: bool,
    ready: bool,
    stride: u32,
    reach: f32,
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
        shot: Shot,
    ) -> Result<FrameInfo, String> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.frame"),
        });
        self.info.drawn = false;
        let mut prepared = match self.prepare(device, queue, &mut encoder, target, &frame, shot) {
            Ok(prepared) => prepared,
            Err(Halt::Blank) => {
                blank(&mut encoder, frame.target);
                queue.submit([encoder.finish()]);
                return Ok(self.info);
            }
            Err(Halt::Hold(info)) => {
                self.waits += 1;
                queue.submit([encoder.finish()]);
                return Ok(info);
            }
            Err(Halt::Fail(detail)) => {
                queue.submit([encoder.finish()]);
                return Err(detail);
            }
        };
        let stride = prepared.stride;
        queue.submit([encoder.finish()]);
        for _ in 0..stride {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("grana.sample"),
            });
            self.record(device, queue, &mut encoder, target, &frame, &mut prepared);
            self.accumulate(device, queue, &mut encoder, target, &frame, &prepared);
            queue.submit([encoder.finish()]);
        }
        if let Err(detail) = self.judge(device, queue, target, &frame, &prepared) {
            self.register();
            return Err(detail);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.present"),
        });
        self.stage(&mut encoder, &prepared);
        self.show(device, queue, &mut encoder, target, &frame, prepared.ready);
        queue.submit([encoder.finish()]);
        self.register();
        Ok(self.info)
    }

    fn register(&mut self) {
        self.residence.register();
        self.convergence.register();
    }

    pub(super) fn transparency(&mut self, frame: &Frame<'_, '_>) -> bool {
        if !frame.scene.splats.is_empty() {
            return true;
        }
        let version = frame.scene.versions.appearance();
        if let Some((held, transparent)) = self.transparency
            && held == version
        {
            return transparent;
        }
        let transparent = frame
            .scene
            .layers
            .iter()
            .flat_map(|layer| layer.styles.iter())
            .any(|style| style.visible() && style.transparent());
        self.transparency = Some((version, transparent));
        transparent
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
        self.prefetch = frame.delivery.prefetches();
        self.viewport = Some(frame.viewport);
        self.present_region = Some(view.region);
        self.sequence = self.sequence.wrapping_add(1);
        self.motion = Some(view.motion);
        let stride = self.convergence.stride(target.age);
        let fragments = self.fragment_allowance(frame.size);
        let splat_budget = self.splat_budget();
        let Self {
            layouts,
            present,
            resources,
            gtao,
            meshes,
            splats,
            sections,
            sliced,
            fade,
            exporting,
            info,
            residence,
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
        let forgotten = resources.atlas.remember(frame.time);
        resources.retire(frame.time);
        resources.drop_hollows(&forgotten);
        let outlook = residence.adapt(Adapt {
            pyramid: &resources.pyramid,
            camera: frame.camera,
            region: frame.region,
            view,
            arena: &resources.arena,
            budget: resources.atlas.slots(),
            size: frame.size,
            time: frame.time,
            versions: scene.versions,
            awaits: shot.awaits,
            assured: shot.assured,
            prefetch: frame.delivery.prefetches(),
            hollow: Some(crate::hollow::Cursor::new(&resources.atlas, frame.time)),
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
        let reach = crate::frame::reach(frame.camera, &tables);
        let (step, ray_bounds) =
            match plan::ray(frame.camera, tables[0], scene.step_scale, view.height) {
                Ok(ray) => ray,
                Err(detail) => return Err(Halt::Fail(detail)),
            };
        let layers = shot.transparent.then_some(crate::fragment::LAYERS);
        if let Err(detail) = allocate::targets(
            device,
            layouts,
            gtao,
            encoder,
            target,
            Needed {
                surfaces: shot.surfaces,
                slice: shot.slice,
                volume: shot.volume,
                layers,
                fragments,
            },
        ) {
            return Err(Halt::Fail(detail));
        }
        let reserved = residence.reserve(outlook.replan);
        if shot.awaits && !outlook.ready && reserved.is_none() {
            info.rebuilt = false;
            info.fading = false;
            return Err(Halt::Hold(*info));
        }
        let planned = plan::shadow(plan::ShadowFrame {
            scene,
            pyramid: &resources.pyramid,
            ladder: outlook.ladder,
            versions: scene.versions,
            summary: &mut resources.summary,
            channel,
            time: frame.time,
            content: residence.content(),
            stale: shot.stale,
        });
        let shadow = &mut resources.shadow;
        if scene.shading.shadows > 0.0 && !shot.slice {
            let voxel = resources.pyramid.level(planned.level).voxel();
            let unit = voxel.into_iter().fold(f32::INFINITY, f32::min);
            shadow.resize(device, layouts, shadow::extent(&planned.space, unit));
        }
        let data = plan::data(plan::Frame {
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
            stale: shot.stale,
            content: residence.content(),
            table: &resources.scene,
            oit: target.oit.as_ref(),
        });
        let signature = Signature {
            data: &data,
            region: view.region,
            versions: scene.versions,
            channels: channel,
            content: residence.content(),
            stamp: residence.stamp(scene.versions.topology()),
            time: frame.time,
            epoch: residence.epoch(),
            size: target.size,
            step: scene.step_scale,
            levels: LevelKey::of(outlook.levels),
            stale: shot.stale,
        };
        let started = timing::begin(Begin {
            target,
            levels: LevelKey::of(outlook.levels),
            layers: signature.layers(),
            content: signature.content(),
            classification: signature.classification(),
            appearance: signature.appearance(),
            splats: scene.versions.splats(),
            detail: signature.detail(),
            lens: signature::lens(&data),
            camera: Mat4::from_cols_array_2d(&data.view_projection),
            exporting: *exporting,
            assured: shot.assured,
            fade,
        });
        if started.snapshot {
            target.fade_group(device, present);
            target.snapshot(encoder, view.region);
            *fade = Some(Fade { weight: 0.0 });
        }
        if stale || started.restaged || started.restyled {
            if shot.surfaces {
                meshes
                    .get_or_insert_with(|| {
                        MeshPasses::new(device, layouts, resources.surface_bytes(), pipelines)
                    })
                    .set(device, queue, layouts, scene);
            } else if let Some(passes) = meshes.as_mut() {
                passes.clear();
            }
        }
        if started.resplatted || started.restaged || started.restyled {
            if shot.splats {
                splats
                    .get_or_insert_with(|| SplatPasses::new(device, layouts, pipelines))
                    .set(device, queue, layouts, scene.splats, splat_budget);
            } else if let Some(passes) = splats.as_mut() {
                passes.clear();
            }
        }
        if stale {
            residence.load_passes(scene.versions);
        }
        if started.recompacts() {
            if let Some(passes) = splats.as_mut() {
                passes.rewind(queue);
                passes.cull_volume(encoder, resources.scene.group());
            }
            if !shot.slice
                && let Some(passes) = meshes.as_mut()
            {
                passes.compact(queue, encoder, resources.scene.group());
            }
        }
        Ok(Prepared {
            shot,
            levels: outlook.levels,
            ladder: outlook.ladder,
            data,
            started,
            reserved,
            residency: outlook.key,
            shadow: planned,
            replan: outlook.replan,
            rebuilt: stale,
            ready: outlook.ready,
            stride,
            reach,
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
            resources,
            volume,
            surface: surface_pass,
            oit_resolve,
            meshes,
            splats,
            sections,
            info,
            dummy_ao,
            dummy_oit,
            dummy_normal,
            dummy_volume,
            ..
        } = self;
        let Some(resources) = resources.as_mut() else {
            return;
        };
        let shot = prepared.shot;
        let view = shot.view;
        let started = prepared.started;
        let ladder = prepared.ladder;
        let data = &mut prepared.data;
        let phase = target.phase;
        let offset = jitter::offset(phase, [view.region[2], view.region[3]]);
        data.jitter = [
            offset.x,
            offset.y,
            phase as f32,
            frame.scene.step_scale.max(1.0e-3),
        ];
        data.lens = crate::frame::lens(
            frame.camera,
            phase % temporal::APERTURE_PHASE,
            temporal::APERTURE_PHASE,
            view.aspect,
        );
        if let Some(oit) = target.oit.as_ref().filter(|_| shot.translucent) {
            data.oit[2] = oit.next_epoch();
        }
        data.tag[2] = splats.as_ref().map_or(0, SplatPasses::count);
        resources.scene.write_frame(queue, *data);
        if std::mem::take(&mut prepared.replan) {
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
        info.rebuilt = prepared.rebuilt;
        if !prepared.ready {
            return;
        }
        let content = data.tag[3];
        if shot.volume && !shot.slice {
            let mut ladder_levels = [0usize; BAND];
            let mut count = 0;
            for level in ladder.levels() {
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
            passes.write_uniforms(queue, data, frame.scene.shading);
        }
        draw::encode(draw::Stage {
            device,
            queue,
            layouts,
            encoder,
            target,
            scene: frame.scene,
            table: &resources.scene,
            groups: draw::Groups {
                level: &resources.level_group,
                atlas: resources.atlas.group(),
                occupancy: &resources.occupancy_group,
            },
            shadow: Some(&mut resources.shadow),
            keys: prepared.shadow,
            bricks: &resources.bricks,
            volume_pass: volume,
            surface: surface_pass,
            oit_resolve,
            meshes: meshes.as_ref(),
            splats: splats.as_ref(),
            sections,
            data,
            features: draw::Features {
                region: view.region,
                slice: shot.slice,
                translucent: shot.translucent,
                volume: shot.volume,
                surfaces: shot.surfaces,
            },
            sample: phase,
            dummy_ao,
            dummy_oit,
            dummy_normal,
            dummy_volume,
        });
        target.age = if started.resets_age() {
            1
        } else {
            target.age.saturating_add(1)
        };
        target.phase = target.phase.wrapping_add(1);
        info.drawn = true;
        info.age = target.age;
    }

    fn judge(
        &mut self,
        device: &Device,
        queue: &Queue,
        target: &mut FrameTarget,
        frame: &Frame<'_, '_>,
        prepared: &Prepared,
    ) -> Result<(), String> {
        let Self {
            convergence, info, ..
        } = self;
        if !prepared.ready || !info.drawn || convergence.converged() {
            return Ok(());
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.judge"),
        });
        convergence.evaluate(
            device,
            queue,
            &mut encoder,
            &target.temporal,
            prepared.shot.view.region,
            frame.exposure,
        );
        queue.submit([encoder.finish()]);
        convergence.poll(device)?;
        info.error = convergence.error();
        info.converged = convergence.converged();
        info.exhausted = target.age >= temporal::SAMPLE_BUDGET;
        Ok(())
    }

    fn stage(&mut self, encoder: &mut CommandEncoder, prepared: &Prepared) {
        let Self {
            resources,
            residence,
            plans,
            ..
        } = self;
        let (Some(resources), Some(slot)) = (resources.as_ref(), prepared.reserved) else {
            return;
        };
        *plans += 1;
        residence.stage(encoder, slot, &resources.planner, prepared.residency);
    }

    fn accumulate(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: &mut FrameTarget,
        frame: &Frame<'_, '_>,
        prepared: &Prepared,
    ) {
        let Self {
            layouts,
            temporal,
            convergence,
            resources,
            info,
            dummy_normal,
            dummy_volume,
            ..
        } = self;
        let (Some(resources), true) = (resources.as_ref(), prepared.ready) else {
            info.fading = false;
            return;
        };
        let view = prepared.shot.view;
        if prepared.started.resets_age() {
            convergence.invalidate();
        }
        let intact = resolve::accumulate(resolve::Accumulate {
            device,
            queue,
            encoder,
            layouts,
            temporal,
            convergence,
            scene: resources.scene.group(),
            target,
            data: &prepared.data,
            region: view.region,
            exposure: frame.exposure,
            volume: prepared.shot.volume,
            reset: prepared.started.refresh,
            clamp: prepared.started.clamps_history(),
            reach: prepared.reach,
            dummy_normal,
            dummy_volume,
        });
        info.history = intact;
        info.age = target.age;
    }

    fn show(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: &mut FrameTarget,
        frame: &Frame<'_, '_>,
        ready: bool,
    ) {
        if !ready || !self.info.drawn {
            return;
        }
        let Self {
            present,
            fade,
            info,
            overlay,
            transparent,
            ..
        } = self;
        let view = View::new(
            frame.viewport,
            frame.size,
            frame.camera,
            frame.region,
            frame.time,
        );
        let blend = timing::fade(fade);
        resolve::present(resolve::Present {
            device,
            queue,
            encoder,
            pass: present,
            target,
            overlay,
            overlays: frame.scene.overlays,
            surface: frame.target,
            fade: blend,
            region: view.region,
            exposure: frame.exposure,
            transparent: *transparent,
        });
        if blend.is_none() {
            target.drop_fade();
        }
        info.fading = blend.is_some();
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
