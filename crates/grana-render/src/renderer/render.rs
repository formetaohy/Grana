use super::allocate::{self, Needed};
use super::draw;
use super::plan::{self, Adapt};
use super::resolve;
use super::signature::channel_style;
use super::timing::{self, Begin};
use super::view::View;
use super::*;
use crate::jitter;
use crate::taa;
use glam::Mat4;

#[derive(Clone, Copy)]
pub(super) struct Shot {
    pub(super) view: View,
    pub(super) awaits: bool,
    pub(super) assured: bool,
    pub(super) slice: bool,
    pub(super) translucent: bool,
    pub(super) surfaces: bool,
    pub(super) splats: bool,
    pub(super) volume: bool,
}

impl Shot {
    fn of(frame: &Frame<'_, '_>, translucent: bool) -> Self {
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
            slice,
            translucent,
            surfaces: !frame.scene.layers.is_empty(),
            splats: !frame.scene.splats.is_empty(),
            volume: frame
                .scene
                .channels
                .iter()
                .any(|row| row.visible && row.opacity > 0.0),
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
        self.info.drawn = false;
        let translucent = self.translucent(&frame);
        let shot = Shot::of(&frame, translucent);
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
        self.residence.register();
    }

    fn translucent(&mut self, frame: &Frame<'_, '_>) -> bool {
        if frame.camera.slice().is_some() {
            return false;
        }
        if !frame.scene.splats.is_empty() {
            return true;
        }
        let version = frame.scene.versions.appearance();
        if let Some((held, translucent)) = self.translucent
            && held == version
        {
            return translucent;
        }
        let translucent = frame
            .scene
            .layers
            .iter()
            .flat_map(|layer| layer.styles.iter())
            .any(|style| style.visible() && style.transparent());
        self.translucent = Some((version, translucent));
        translucent
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
        resources.atlas.remember(frame.time);
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
            prefetch: frame.delivery.prefetches(),
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
            info.rebuilt = false;
            info.fading = false;
            return Err(Halt::Hold(*info));
        }
        let layers = shot.translucent.then_some(crate::fragment::LAYERS);
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
                fragments: *fragments,
            },
        ) {
            return Err(Halt::Fail(detail));
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
            awaits: shot.awaits,
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
            awaits: shot.awaits,
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
        };
        let started = timing::begin(Begin {
            target,
            levels: LevelKey::of(outlook.levels),
            layers: signature.layers(),
            content: signature.content(),
            classification: signature.classification(),
            appearance: signature.appearance(),
            view: signature::view(&data),
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
                    .get_or_insert_with(|| MeshPasses::new(device, layouts, *budget / 2, pipelines))
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
        let offset = jitter::offset(started.phase, [view.region[2], view.region[3]]);
        data.jitter = [
            offset.x,
            offset.y,
            started.phase as f32,
            frame.scene.step_scale.max(1.0e-3),
        ];
        data.lens = crate::frame::lens(
            frame.camera,
            started.phase % taa::APERTURE_PHASE,
            taa::APERTURE_PHASE,
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
            sample: started.phase,
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
            layouts,
            present,
            taa,
            resources,
            transparent,
            fade,
            info,
            residence,
            overlay,
            dummy_normal,
            dummy_volume,
            ..
        } = self;
        let (Some(resources), true) = (resources.as_ref(), prepared.ready) else {
            info.fading = false;
            if let (Some(slot), Some(resources)) = (prepared.reserved, resources.as_ref()) {
                residence.stage(encoder, slot, &resources.planner, prepared.residency);
            }
            return;
        };
        let view = prepared.shot.view;
        let intact = resolve::encode(resolve::Stage {
            device,
            queue,
            encoder,
            layouts,
            present,
            taa,
            scene: resources.scene.group(),
            target,
            overlay,
            overlays: frame.scene.overlays,
            data: &prepared.data,
            surface: frame.target,
            fade,
            region: view.region,
            exposure: frame.exposure,
            transparent: *transparent,
            volume: prepared.shot.volume,
            phase: prepared.started.phase,
            reset: prepared.started.refresh,
            clamp: prepared.started.clamps_history(),
            previous: prepared.started.previous,
            dummy_normal,
            dummy_volume,
        });
        info.history = intact;
        info.fading = fade.is_some();
        if let Some(slot) = prepared.reserved {
            residence.stage(encoder, slot, &resources.planner, prepared.residency);
        }
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
