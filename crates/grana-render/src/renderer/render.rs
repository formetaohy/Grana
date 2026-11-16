use super::signature::{SceneSummary, ShadowKeys, channel_style, digest, hash_camera};
use super::*;
use std::time::Instant;

const AMBIENT_REACH: f32 = 40.0;
const AMBIENT_STRENGTH: f32 = 0.9;

impl Renderer {
    pub(super) fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        target: &mut FrameTarget,
        frame: Frame<'_, '_>,
    ) -> Result<FrameInfo, String> {
        let Frame {
            target: surface,
            size: surface_size,
            resolution,
            camera,
            viewport: pick,
            time,
            region: bounds,
            clip,
            exposure,
            sampling,
            budget: convergence_budget,
            scene,
            display,
            level_cap: _,
            temporal,
        } = frame;
        assert!(inside(pick, surface_size), "the view lies on the surface");
        let viewport = scaled(pick, surface_size, resolution);
        let size = resolution;
        let display_height = pick[3].max(1);
        assert!(
            inside(viewport, size),
            "the view lies on the render surface"
        );
        let awaits = display.awaits();
        let assured = display.assures();
        self.time = time;
        self.prefetch = display.prefetches();
        self.viewport = Some(pick);
        let slice = camera.slice().is_some();
        let slice_objects = slice && (!scene.layers.is_empty() || !scene.splats.is_empty());
        let temporal = temporal.filter(|_| !slice_objects);
        let translucent = !slice
            && (scene
                .styles
                .iter()
                .any(|style| style.visible() && style.transparent())
                || !scene.splats.is_empty());
        let surfaces = !scene.layers.is_empty();
        let splats_present = !scene.splats.is_empty();
        let motion = view_motion(camera, viewport, bounds, time, exposure);
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        self.motion = Some(motion);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.frame"),
        });
        let Self {
            layouts,
            present,
            scene: scene_table,
            atlas,
            arena,
            level_group,
            bricks,
            planner,
            volume,
            oit_resolve,
            gtao,
            meshes,
            splats,
            pyramid,
            channels,
            volume_level,
            volume_measure,
            volume_rejection,
            fit_cache,
            summary,
            level_cap,
            feedback,
            frames,
            view_bytes,
            epoch,
            resident,
            channels_key,
            content,
            generation,
            settled,
            planned,
            demand,
            shadow,
            transparent,
            exporting,
            convergence,
            temporal: temporal_pass,
            fade,
            object_key,
            info,
            dummy_oit,
            dummy_ao,
            budget,
            sections,
            sliced,
            occupancy_group,
            bake,
            baked,
            overlay,
            pipelines,
            ..
        } = self;
        let (
            Some(pyramid),
            Some(atlas),
            Some(scene_table),
            Some(planner),
            Some(arena),
            Some(level_group),
        ) = (
            pyramid.as_ref(),
            atlas.as_mut(),
            scene_table.as_mut(),
            planner.as_mut(),
            arena.as_ref(),
            level_group.as_ref(),
        )
        else {
            clear(&mut encoder, surface);
            queue.submit([encoder.finish()]);
            return Ok(*info);
        };
        assert_eq!(
            scene.channels.len(),
            *channels,
            "the scene carries one row per channel"
        );
        assert!(
            time < pyramid.frame_count(),
            "the requested frame lies inside the time series"
        );
        *sliced = slice;
        sections.prepare(
            device,
            queue,
            layouts,
            scene,
            camera.slice().map(|frame| frame.plane(camera.pivot())),
        );
        atlas.prune([time, carried_frame(time, pyramid.frame_count())]);
        let slots = atlas.slots();
        let level_count = pyramid.level_count();
        let first = arena.first();
        let ceiling = level_cap
            .unwrap_or(level_count - 1)
            .max(first)
            .min(level_count - 1);
        let limits = Limits { first, ceiling };
        let allowed = limits.count(pyramid);
        let share = slots.max(1);
        let width = viewport[2].max(1);
        let height = viewport[3].max(1);
        let aspect = width as f32 / height as f32;
        let current = if assured { None } else { *volume_level };
        let measured = !assured
            && current.is_some_and(|level| volume_measure.get(level).copied().flatten().is_some());
        let rejected = if assured {
            None
        } else {
            volume_rejection
                .filter(|(captured, _)| *captured == motion)
                .map(|(_, level)| level)
        };
        let view = FitView {
            pyramid,
            camera,
            region: bounds,
            aspect,
            height: display_height,
            limits,
            capacity: share,
            current,
        };
        let fit_key = digest(&[
            motion,
            share as u64,
            u64::from(display_height),
            ceiling as u64,
            first as u64,
            u64::from(u32::from(measured)),
        ]);
        let (fit, overflowed) = if measured {
            fit_measured(view, volume_measure, rejected)
        } else if let Some((key, held)) = *fit_cache
            && key == fit_key
        {
            (held, None)
        } else {
            let computed = fit_volume(view);
            *fit_cache = Some((fit_key, computed));
            (computed, None)
        };
        let base = fit.ladder.base.min(allowed - 1);
        let count = BAND.min(allowed - base);
        let ladder = Ladder { base, count };
        let capacity = if base == fit.ladder.base && count == fit.ladder.count {
            fit.capacity
        } else {
            capacities(share, &fit.capacity, count)
        };
        if !assured {
            let overflow = info.view.visible > info.view.demand;
            *volume_rejection = overflowed
                .or_else(|| current.filter(|_| overflow))
                .map(|level| (motion, level));
        }
        let levels = Levels { ladder, capacity };
        *volume_level = Some(base);
        let versions = scene.versions;
        let channel_key = channel_style(scene.channels);
        if *channels_key != Some(channel_key) {
            scene_table.prepare(device, queue, pyramid, scene.channels);
            scene_table.write_channels(device, queue, pyramid, scene.channels);
            *channels_key = Some(channel_key);
        }
        let stamp = digest(&[versions.data(), *generation, u64::from(*content)]);
        let next = Resident {
            objects: versions.objects(),
            style: versions.style(),
        };
        let stale = *resident != Some(next);
        let residency = Residency::of(
            *epoch,
            stamp,
            Mode {
                prefetch: display.prefetches(),
                assured,
            },
            time,
            ViewState {
                camera,
                size: surface_size,
                region: bounds,
                levels,
            },
        );
        let replan = planned.as_ref() != Some(&residency);
        info.settled = *settled == Some(residency);
        info.view.level = ladder.base;
        info.truncated = info.view.level == ladder.base && info.view.visible > info.view.demand;
        let ready_at_time = info.settled && (!assured || !info.truncated);
        let ready = if awaits || assured {
            ready_at_time
        } else {
            true
        };
        let slot_grid = atlas.slot_grid();
        let mut prepared = [usize::MAX; BAND + 2];
        let mut prepared_count = 0;
        for level in ladder.levels() {
            if prepared[..prepared_count].contains(&level) {
                continue;
            }
            prepared[prepared_count] = level;
            prepared_count += 1;
            ensure_brick(bricks, pyramid.level(level), level, slot_grid, arena);
            let table = bricks[level].as_mut().expect("the brick table exists");
            if table.time() != Some(time) {
                table.rebase(queue, time, *content, atlas, arena);
            }
        }
        let tables = ladder
            .levels()
            .map(|level| bricks[level].as_ref().expect("the band table exists"))
            .collect::<Vec<&BrickTable>>();
        let step = crate::frame::step_length(camera, tables[0], scene.step_scale);
        let ray_bounds =
            crate::frame::sample_bounds(camera, tables[0], step, scene.step_scale, height)?;
        let reserved = if replan {
            feedback.as_mut().and_then(Readback::reserve)
        } else {
            None
        };
        if awaits && !ready_at_time && reserved.is_none() {
            target.samples = 0;
            info.samples = 0;
            info.rebuilt = false;
            info.fading = false;
            return Ok(*info);
        }
        let finest = pyramid.level(0);
        let world = Vec3::from(finest.world_extent());
        let spacing = finest.spacing();
        let scene_summary = match *summary {
            Some(held) if held.holds(versions) => held,
            _ => {
                let held = SceneSummary::of(scene, world);
                *summary = Some(held);
                held
            }
        };
        let (shadow_low, shadow_high) = scene_summary.bounds();
        let space = ShadowSpace::new(
            scene.shading.light(),
            shadow_low,
            shadow_high,
            Vec3::new(spacing.x(), spacing.y(), spacing.z()),
        );
        let shadow_level = ladder.base + ladder.count - 1;
        let shadow_revision = bricks[shadow_level]
            .as_ref()
            .expect("the shadow band carries a table")
            .revision();
        if let Some(shadow) = shadow.as_mut() {
            let voxel = pyramid.level(shadow_level).voxel();
            let unit = voxel.into_iter().fold(f32::INFINITY, f32::min);
            shadow.resize(device, layouts, shadow::extent(&space, unit));
        }
        let shadow_keys = ShadowKeys::of(
            versions.objects(),
            scene_summary.visibility(),
            shadow_revision,
            time,
            channel_key,
            shadow_level,
            &space,
        );
        let ray = crate::frame::Ray {
            step,
            bounds: ray_bounds,
            channels: *channels,
        };
        let mut data = FrameData::new(
            VolumeView {
                camera,
                region: bounds,
                clip,
                aspect,
                height,
                time,
            },
            ladder,
            &tables,
            scene.shading,
            ray,
            space,
        );
        data.viewport = [
            viewport[0] as f32,
            viewport[1] as f32,
            viewport[2] as f32,
            viewport[3] as f32,
        ];
        data.oit = [u32::from(translucent), 0, 0, 0];
        data.tag[1] = u32::from(!awaits);
        data.tag[3] = *content;
        data.occupancy[0] = scene_table.threshold_floor();
        let pixels = u64::from(width) * u64::from(height);
        if translucent
            && target
                .oit
                .as_ref()
                .is_none_or(|oit| !oit.fits(size, pixels))
        {
            target.oit = Some(Oit::new(
                device,
                layouts,
                size,
                pixels,
                *budget / FRAGMENT_BUDGET_DIVISOR,
            ));
        }
        if let Some(oit) = target.oit.as_ref().filter(|_| translucent) {
            data.oit[1] = oit.layers();
            data.oit[2] = oit.next_epoch();
        }
        let signature = Signature {
            data: &data,
            camera: camera.state(),
            region: viewport,
            levels,
            versions,
            channels: channel_key,
            content: *content,
            generation: *generation,
            time,
            exposure,
            epoch: *epoch,
            size,
            overlay: crate::schema::overlay_digest(scene.overlays),
        };
        let structure = signature.digest();
        let transition = signature.transition();
        let shot = signature.shot();
        let view_now = FrameView::of(&data, viewport);
        let key = LevelKey::of(levels);
        let moved = target.transition != Some(transition);
        if moved {
            *fade = None;
        }
        if target.levels.is_some_and(|previous| previous != key)
            && !moved
            && !*exporting
            && !assured
            && sampling.ceiling() > 1
            && target.samples > 0
        {
            target.fade_group(device, present);
            target.snapshot(&mut encoder);
            *fade = Some(Fade { weight: 0.0 });
        }
        target.transition = Some(transition);
        target.levels = Some(key);
        if target.signature != Some(structure) {
            target.signature = Some(structure);
            target.samples = 0;
        }
        let reset = target.samples == 0;
        if reset {
            target.deadline =
                convergence_budget.and_then(|budget| Instant::now().checked_add(budget));
            target.estimate = None;
        }
        let expired = target
            .deadline
            .is_some_and(|deadline| deadline <= Instant::now());
        let mut seeding = false;
        if reset {
            target.seeded = false;
            let continuing = temporal.is_some() && target.holds && target.shot == Some(shot);
            if temporal.is_some() {
                temporal_pass.ensure(&mut target.history, device, size);
            }
            if continuing {
                let previous = target.view;
                let history = target.history.as_mut().expect("the history is allocated");
                temporal_pass.capture(
                    device,
                    queue,
                    &mut encoder,
                    history,
                    &previous,
                    &crate::temporal::Inputs {
                        accumulated: &target.accumulation_view,
                        counts: &target.count_view,
                        fresh: target.scene.color(),
                        depth: target.scene.depth(),
                        volume: target.guide.view(),
                        normal: target.gbuffer.as_ref().map(crate::target::Gbuffer::normal),
                    },
                );
                seeding = true;
            } else {
                target.holds = false;
                TemporalPass::invalidate(&mut target.history);
            }
            target.shot = Some(shot);
        }
        if reset && let Some(convergence) = convergence.as_mut() {
            convergence.reset();
        }
        if let Some(convergence) = convergence.as_mut()
            && let Some(estimate) = convergence.poll(device, target.samples)
        {
            target.estimate = Some(estimate);
        }
        let resolved = sampling.met(target.estimate);
        info.samples = target.samples;
        info.rebuilt = false;
        if reset {
            info.rebuilt = stale;
            if stale {
                if surfaces {
                    meshes
                        .get_or_insert_with(|| {
                            MeshPasses::new(device, layouts, *budget / 2, pipelines)
                        })
                        .set(device, queue, layouts, scene);
                } else if let Some(passes) = meshes.as_mut() {
                    passes.clear();
                }
                if splats_present {
                    splats
                        .get_or_insert_with(|| SplatPasses::new(device, layouts, pipelines))
                        .set(device, queue, layouts, scene.splats);
                } else if let Some(passes) = splats.as_mut() {
                    passes.clear();
                }
                *resident = Some(next);
            }
            if let Some(splat_passes) = splats.as_mut() {
                splat_passes.rewind(queue);
                splat_passes.cull_volume(&mut encoder, scene_table.group());
            }
            if !slice && let Some(mesh_passes) = meshes.as_mut() {
                mesh_passes.cull_volume(&mut encoder, scene_table.group());
                if scene.shading.shadows > 0.0 {
                    mesh_passes.cull_shadow(&mut encoder, scene_table.group());
                }
            }
        }
        let sample_index = if reset && !assured && temporal.is_some() {
            (sequence % u64::from(sampling.ceiling())) as u32
        } else {
            target.samples.min(sampling.ceiling() - 1)
        };
        let offset = sample::offset(sample_index, [width, height]);
        data.jitter = [
            offset.x,
            offset.y,
            sample_index as f32,
            scene.step_scale.max(1.0e-3),
        ];
        data.lens = crate::frame::lens(camera, sample_index, sampling.ceiling(), aspect);
        data.tag[2] = splats.as_ref().map_or(0, SplatPasses::count);
        scene_table.write_frame(queue, data);
        if replan {
            let scale = bucket_scale(
                camera,
                tables[0].bounds.0.to_array(),
                tables[0].world.to_array(),
            );
            for band in 0..BAND {
                let (band_capacity, dispatch, record_base) = match tables.get(band) {
                    Some(table) => (
                        capacity.get(band).copied().unwrap_or(0) as u32,
                        volume_dispatch(table),
                        table.record_base(),
                    ),
                    None => (0, 0, 0),
                };
                planner.view.bands[band].prepare(
                    queue,
                    Schedule {
                        time,
                        band: band as u32,
                        capacity: band_capacity,
                        dispatch,
                        bucket_scale: scale,
                        base: record_base,
                    },
                );
            }
            planner.encode(&mut encoder, scene_table.group());
        }
        if target.samples < sampling.ceiling() && ready && !expired {
            target.samples += 1;
            info.samples = target.samples;
            let volume_visible = scene
                .channels
                .iter()
                .any(|row| row.visible && row.opacity > 0.0);
            let projected = scene.channels.iter().any(|row| {
                row.visible
                    && row.opacity > 0.0
                    && row.volume.mode() == grana_display::VolumeMode::Maximum
            });
            if volume_visible
                && !slice
                && let Some(bake) = bake.as_ref()
            {
                let mut levels = [0usize; BAND];
                let mut level_count = 0;
                for level in ladder.levels() {
                    levels[level_count] = level;
                    level_count += 1;
                }
                let entries = baked.collect(
                    atlas,
                    &levels[..level_count],
                    time,
                    *content,
                    bake.capacity(),
                );
                if !entries.is_empty() {
                    bake.stage(queue, entries);
                    bake.encode(
                        &mut encoder,
                        entries.len() as u32,
                        scene_table.group(),
                        level_group,
                        atlas.source(),
                    );
                }
            }
            if let Some(mesh_passes) = meshes.as_mut() {
                mesh_passes.write_uniforms(queue, &data, scene.shading);
            }
            if surfaces && !slice && target.gbuffer.is_none() {
                let gbuffer = crate::target::Gbuffer::new(
                    device,
                    &layouts.gbuffer,
                    target.scene.depth(),
                    size,
                );
                let ao = crate::ao::Ao::new(
                    device,
                    layouts,
                    gtao,
                    gbuffer.normal(),
                    target.scene.depth(),
                    target.guide.view(),
                    size,
                );
                target.gbuffer = Some(gbuffer);
                target.ao = Some(ao);
            }
            if reset && !volume_visible {
                target.guide.clear(&mut encoder);
            }
            if let Some(shadow) = shadow.as_mut() {
                let groups = ShadowGroups {
                    scene: scene_table.shadow_group(),
                    level: level_group,
                    atlas: atlas.group(),
                };
                if !slice && scene.shading.shadows > 0.0 && volume_visible {
                    let object = shadow_keys.objects();
                    if *object_key != Some(object) {
                        *object_key = Some(object);
                        shadow.clear_objects(&mut encoder);
                        if let Some(mesh_passes) = meshes.as_ref() {
                            mesh_passes.draw_shadow(&mut encoder, shadow.objects());
                        }
                    }
                    let volume = shadow_keys.volume();
                    if shadow.stale(volume) {
                        scene_table.write_shadow_frame(queue, data);
                    }
                    shadow.encode_tau(&mut encoder, groups, volume);
                }
            }
            let group = scene_table.group();
            let shadow_group = shadow
                .as_ref()
                .map(Shadow::group)
                .expect("the shadow is ready");
            let occupancy = occupancy_group.as_ref().expect("the occupancy is ready");
            if slice {
                crate::target::clear(&mut encoder, target.scene.color(), target.scene.depth());
                if volume_visible {
                    volume.draw_surface(
                        &mut encoder,
                        crate::volume::SurfaceFrame {
                            normal: target.guide.view(),
                            region: viewport,
                            scene: group,
                            level: level_group,
                            atlas: atlas.group(),
                            depth_group: target.scene.depth_group(),
                            occupancy,
                        },
                    );
                }
            } else if surfaces && let Some(gbuffer) = target.gbuffer.as_ref() {
                if let Some(mesh_passes) = meshes.as_ref() {
                    mesh_passes.draw_gbuffer(
                        &mut encoder,
                        viewport,
                        gbuffer,
                        target.scene.depth(),
                        group,
                    );
                }
                if volume_visible {
                    volume.draw_surface(
                        &mut encoder,
                        crate::volume::SurfaceFrame {
                            normal: target.guide.view(),
                            region: viewport,
                            scene: group,
                            level: level_group,
                            atlas: atlas.group(),
                            depth_group: target.scene.depth_group(),
                            occupancy,
                        },
                    );
                }
                target
                    .ao
                    .as_ref()
                    .expect("the ambient occlusion target is ready")
                    .evaluate(
                        queue,
                        &mut encoder,
                        &data,
                        size,
                        crate::ao::Ambient::new(AMBIENT_REACH, AMBIENT_STRENGTH, sample_index),
                    );
            } else {
                crate::target::clear(&mut encoder, target.scene.color(), target.scene.depth());
            }
            let ao_group: &BindGroup = target.ao.as_ref().map_or(&*dummy_ao, crate::ao::Ao::group);
            let oit_epoch = target
                .oit
                .as_mut()
                .filter(|_| translucent)
                .map(|oit| oit.begin(&mut encoder));
            if let Some(epoch) = oit_epoch {
                assert_eq!(
                    epoch, data.oit[2],
                    "the fragment store opens the epoch the frame names"
                );
            }
            let oit = target.oit.as_ref().filter(|_| translucent);
            let oit_group: &BindGroup = oit.map_or(dummy_oit.read_group(), Oit::read_group);
            if slice {
                if volume_visible {
                    volume.draw(
                        device,
                        layouts,
                        pipelines,
                        &mut encoder,
                        crate::volume::VolumeFrame {
                            target: target.scene.color(),
                            region: viewport,
                            scene: group,
                            level: level_group,
                            atlas: atlas.group(),
                            depth_group: target.scene.depth_group(),
                            oit: oit_group,
                            occupancy,
                            shadow: shadow_group,
                        },
                        crate::volume::VolumePath::Composite,
                    );
                }
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("grana.section.pass"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: target.scene.color(),
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Load,
                                store: StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        ..Default::default()
                    });
                    pass.set_viewport(
                        viewport[0] as f32,
                        viewport[1] as f32,
                        width as f32,
                        height as f32,
                        0.0,
                        1.0,
                    );
                    pass.set_scissor_rect(viewport[0], viewport[1], width, height);
                    sections.draw(&mut pass, group);
                }
                if let Some(splat_passes) = splats.as_ref() {
                    splat_passes.draw_slice(
                        &mut encoder,
                        SliceTargets {
                            color: target.scene.color(),
                            depth: target.scene.depth(),
                        },
                        viewport,
                        group,
                    );
                }
            }
            if let Some(oit) = oit {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("grana.oit.pass"),
                    color_attachments: &[
                        Some(RenderPassColorAttachment {
                            view: oit.residual_color(),
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: StoreOp::Store,
                            },
                        }),
                        Some(RenderPassColorAttachment {
                            view: oit.residual_moment(),
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: StoreOp::Store,
                            },
                        }),
                    ],
                    depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                        view: target.scene.depth(),
                        depth_ops: Some(Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_viewport(
                    viewport[0] as f32,
                    viewport[1] as f32,
                    width as f32,
                    height as f32,
                    0.0,
                    1.0,
                );
                pass.set_scissor_rect(viewport[0], viewport[1], width, height);
                if let Some(mesh_passes) = meshes.as_ref() {
                    mesh_passes.draw_oit(&mut pass, shadow_group, oit.write_group(), ao_group);
                }
                if let Some(splat_passes) = splats.as_ref() {
                    splat_passes.draw_oit(&mut pass, group, oit.write_group());
                }
            }
            if surfaces
                && !slice
                && let Some(gbuffer) = target.gbuffer.as_ref()
            {
                meshes
                    .as_ref()
                    .expect("a geometry buffer carries mesh passes")
                    .draw_deferred(
                        &mut encoder,
                        viewport,
                        target.scene.color(),
                        gbuffer.group(),
                        shadow_group,
                        ao_group,
                    );
            }
            if volume_visible && !slice {
                let volume_frame = crate::volume::VolumeFrame {
                    target: target.scene.color(),
                    region: viewport,
                    scene: group,
                    level: level_group,
                    atlas: atlas.group(),
                    depth_group: target.scene.depth_group(),
                    oit: oit_group,
                    occupancy,
                    shadow: shadow_group,
                };
                let path = if projected {
                    crate::volume::VolumePath::Projection
                } else {
                    crate::volume::VolumePath::Composite
                };
                volume.draw(device, layouts, pipelines, &mut encoder, volume_frame, path);
            }
            if let Some(oit) = oit
                && !volume_visible
            {
                oit_resolve
                    .get_or_insert_with(|| OitResolve::new(device, layouts, pipelines))
                    .draw(
                        &mut encoder,
                        target.scene.color(),
                        viewport,
                        group,
                        oit.read_group(),
                    );
            }
            if !scene.overlays.is_empty() {
                overlay.prepare(
                    device,
                    queue,
                    scene.overlays,
                    glam::Mat4::from_cols_array_2d(&data.view_projection),
                );
                overlay.draw(&mut encoder, target.scene.color(), viewport);
            }
            if sampling.measuring() && target.samples > 1 {
                convergence
                    .get_or_insert_with(|| Convergence::new(device, pipelines))
                    .encode(
                        device,
                        &mut encoder,
                        target.size,
                        target.scene.color(),
                        &target.accumulation_view,
                        target.samples,
                    );
            }
            present.accumulate(
                &mut encoder,
                &target.accumulation_view,
                &target.count_view,
                &target.accumulate_group,
                reset,
            );
            if seeding && target.samples == 1 {
                let request = temporal.expect("the seeding frame carries a history request");
                let history = target.history.as_ref().expect("the history is captured");
                let spacing = pyramid.level(ladder.base).spacing();
                let tolerance =
                    crate::temporal::tolerance(spacing.x().max(spacing.y()).max(spacing.z()));
                temporal_pass.seed(
                    device,
                    queue,
                    &mut encoder,
                    crate::temporal::Reuse {
                        history,
                        view: &view_now,
                        length: request.history(),
                        tolerance,
                        inputs: &crate::temporal::Inputs {
                            accumulated: &target.accumulation_view,
                            counts: &target.count_view,
                            fresh: target.scene.color(),
                            depth: target.scene.depth(),
                            volume: target.guide.view(),
                            normal: target.gbuffer.as_ref().map(crate::target::Gbuffer::normal),
                        },
                    },
                );
                target.seeded = true;
            }
            target.holds = true;
        } else if !ready {
            target.samples = 0;
            info.samples = 0;
        }
        info.converged = ready && (target.samples >= sampling.ceiling() || resolved || expired);
        if ready {
            if let Some(active) = fade.as_mut() {
                active.weight = (active.weight + 1.0 / FADE_DRAWS).min(1.0);
            }
            let blend = fade.as_ref().map(|fade| fade.weight);
            present.write(queue, exposure, *transparent, blend);
            if fade.is_some() {
                let group = target.fade_group(device, present);
                present.present(&mut encoder, surface, group);
                if fade.as_ref().is_some_and(|active| active.weight >= 1.0) {
                    *fade = None;
                }
            } else {
                present.present(&mut encoder, surface, &target.present_group);
            }
        }
        info.fading = ready && fade.is_some();
        if let Some(slot) = reserved
            && let Some(feedback) = feedback.as_mut()
        {
            for band in 0..BAND {
                feedback.stage(
                    &mut encoder,
                    slot,
                    band as u64 * *view_bytes,
                    planner.view.bands[band].buffer(),
                    *view_bytes,
                );
            }
            frames[slot] = Some(residency);
            *planned = Some(residency);
            if demand.as_ref().is_none_or(|held| !held.covers(&residency)) {
                *demand = None;
            }
        }
        queue.submit([encoder.finish()]);
        if let Some(convergence) = convergence.as_mut() {
            convergence.register();
        }
        if let Some(feedback) = feedback.as_mut() {
            feedback.register();
        }
        info.seeded = target.seeded;
        info.history = temporal.is_some()
            && target
                .history
                .as_ref()
                .is_some_and(crate::temporal::History::ready);
        target.view = view_now;
        Ok(*info)
    }
}

fn view_motion(camera: &Camera, viewport: [u32; 4], clip: Region, time: u32, exposure: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_camera(camera.state(), &mut hasher);
    let aspect = viewport[2].max(1) as f32 / viewport[3].max(1) as f32;
    aspect.to_bits().hash(&mut hasher);
    viewport.hash(&mut hasher);
    time.hash(&mut hasher);
    exposure.to_bits().hash(&mut hasher);
    clip.enabled().hash(&mut hasher);
    clip.center().to_array().map(f32::to_bits).hash(&mut hasher);
    clip.half().to_array().map(f32::to_bits).hash(&mut hasher);
    for axis in 0..3 {
        clip.axis(axis)
            .to_array()
            .map(f32::to_bits)
            .hash(&mut hasher);
    }
    hasher.finish()
}

pub(super) fn clear(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.clear"),
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

pub(super) fn inside(region: [u32; 4], size: [u32; 2]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0] + region[2] <= size[0]
        && region[1] + region[3] <= size[1]
}

pub(super) fn scaled(region: [u32; 4], surface: [u32; 2], resolution: [u32; 2]) -> [u32; 4] {
    if surface == resolution {
        return region;
    }
    let scale = |value: u32, source: u32, target: u32| -> u32 {
        (u64::from(value) * u64::from(target) / u64::from(source)) as u32
    };
    let x = scale(region[0], surface[0], resolution[0]);
    let y = scale(region[1], surface[1], resolution[1]);
    let width = scale(region[2], surface[0], resolution[0])
        .max(1)
        .min(resolution[0].saturating_sub(x));
    let height = scale(region[3], surface[1], resolution[1])
        .max(1)
        .min(resolution[1].saturating_sub(y));
    [x, y, width, height]
}
