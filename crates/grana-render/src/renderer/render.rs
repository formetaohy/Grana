use super::allocate::{self, Needed};
use super::draw::{self, Draw};
use super::plan;
use super::resolve;
use super::signature::{channel_style, digest};
use super::timing::{self, Begin};
use super::view::View;
use super::*;

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
        let view = View::new(
            pick,
            surface_size,
            resolution,
            camera,
            bounds,
            time,
            exposure,
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
        let volume_visible = scene
            .channels
            .iter()
            .any(|row| row.visible && row.opacity > 0.0);
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        self.motion = Some(view.motion);
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
            clear,
            surface: surface_pass,
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
            dummy_normal,
            dummy_volume,
            budget,
            pipelines,
            sections,
            sliced,
            occupancy_group,
            bake,
            baked,
            overlay,
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
            blank(&mut encoder, surface);
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
        let current = if assured { None } else { *volume_level };
        let measured = !assured
            && current.is_some_and(|level| volume_measure.get(level).copied().flatten().is_some());
        let rejected = if assured {
            None
        } else {
            volume_rejection
                .filter(|(captured, _)| *captured == view.motion)
                .map(|(_, level)| level)
        };
        let planned_levels = plan::levels(
            plan::Input {
                pyramid,
                camera,
                region: bounds,
                view,
                arena,
                slots,
                cap: *level_cap,
                current,
                measured,
                rejected,
            },
            volume_measure,
            fit_cache,
        );
        let levels = planned_levels.levels;
        let ladder = planned_levels.ladder;
        if !assured {
            let overflow = info.view.visible > info.view.demand;
            *volume_rejection = planned_levels
                .overflowed
                .or_else(|| current.filter(|_| overflow))
                .map(|level| (view.motion, level));
        }
        *volume_level = Some(ladder.base);
        let versions = scene.versions;
        let channel_key = channel_style(scene.channels);
        if *channels_key != Some(channel_key) {
            scene_table.publish(device, queue, pyramid, scene.channels);
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
        plan::prepare(plan::Tables {
            bricks,
            pyramid,
            levels,
            atlas,
            arena,
            queue,
            time,
            content: *content,
        });
        let tables = plan::tables(bricks, levels);
        let (step, ray_bounds) = plan::ray(camera, tables[0], scene.step_scale, view.height)?;
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
        allocate::targets(
            device,
            layouts,
            gtao,
            &mut encoder,
            target,
            Needed {
                surfaces,
                slice,
                volume: volume_visible,
                translucent,
                budget: *budget,
            },
        );
        let planned_shadow = plan::shadow(plan::ShadowFrame {
            scene,
            pyramid,
            ladder,
            bricks,
            atlas,
            versions,
            summary,
            channel: channel_key,
            time,
        });
        let space = planned_shadow.space;
        let shadow_keys = planned_shadow.keys;
        if let Some(shadow) = shadow.as_mut() {
            let voxel = pyramid.level(planned_shadow.level).voxel();
            let unit = voxel.into_iter().fold(f32::INFINITY, f32::min);
            shadow.resize(device, layouts, shadow::extent(&space, unit));
        }
        let ray = crate::frame::Ray {
            step,
            bounds: ray_bounds,
            channels: *channels,
        };
        let mut data = plan::data(plan::Frame {
            camera,
            region: bounds,
            clip,
            view,
            time,
            ladder,
            tables: &tables,
            scene,
            ray,
            space,
            translucent,
            awaits,
            content: *content,
            table: scene_table,
            oit: target.oit.as_ref(),
        });
        let signature = Signature {
            data: &data,
            camera: camera.state(),
            region: view.region,
            levels,
            versions,
            channels: channel_key,
            content: *content,
            generation: *generation,
            time,
            exposure,
            epoch: *epoch,
            size: target.size,
            overlay: crate::schema::overlay_digest(scene.overlays),
        };
        let structure = signature.digest();
        let transition = signature.transition();
        let shot = signature.shot();
        let view_now = FrameView::of(&data, view.region);
        let key = LevelKey::of(levels);
        let started = timing::begin(Begin {
            target,
            sampling,
            budget: convergence_budget,
            levels: key,
            transition,
            structure,
            shot,
            temporal,
            sequence,
            fade,
            exporting: *exporting,
            assured,
        });
        if started.snapshot {
            target.fade_group(device, present);
            target.snapshot(&mut encoder, view.region);
            *fade = Some(Fade { weight: 0.0 });
        }
        if started.reset {
            if temporal.is_some() {
                temporal_pass.ensure(&mut target.history, device, target.size);
            }
            if started.continuing {
                let geometry =
                    target.geometry(device, layouts, dummy_normal, dummy_volume, volume_visible);
                resolve::capture(
                    temporal_pass,
                    &mut encoder,
                    resolve::Capture {
                        device,
                        history: target.history.as_mut().expect("the history is allocated"),
                        previous: target.view,
                        scene: scene_table.group(),
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
        if let Some(convergence) = convergence.as_mut()
            && let Some(estimate) = convergence.poll(device, target.samples)
        {
            target.estimate = Some(estimate);
        }
        let resolved = sampling.met(target.estimate);
        info.samples = target.samples;
        info.rebuilt = started.reset && stale;
        let offset = sample::offset(started.sample, [view.region[2], view.region[3]]);
        data.jitter = [
            offset.x,
            offset.y,
            started.sample as f32,
            scene.step_scale.max(1.0e-3),
        ];
        data.lens = crate::frame::lens(camera, started.sample, sampling.ceiling(), view.aspect);
        data.tag[2] = splats.as_ref().map_or(0, SplatPasses::count);
        let volume_extent = crate::target::volume_size(target.size);
        data.volume = [
            target.size[0] as f32 / volume_extent[0] as f32,
            target.size[1] as f32 / volume_extent[1] as f32,
            volume_extent[0] as f32,
            volume_extent[1] as f32,
        ];
        scene_table.write_frame(queue, data);
        if replan {
            plan::schedule(planner, queue, camera, &tables, levels, time);
            planner.encode(&mut encoder, scene_table.group());
        }
        let drawing = target.samples < sampling.ceiling() && ready && !started.expired;
        if drawing {
            target.samples += 1;
            info.samples = target.samples;
            if volume_visible
                && !slice
                && let Some(bake) = bake.as_ref()
            {
                let mut ladder_levels = [0usize; BAND];
                let mut count = 0;
                for level in ladder.levels() {
                    ladder_levels[count] = level;
                    count += 1;
                }
                let entries = baked.collect(
                    atlas,
                    &ladder_levels[..count],
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
            let occupancy = occupancy_group.as_ref().expect("the occupancy is ready");
            {
                let draw = Draw {
                    encoder: &mut encoder,
                    device,
                    queue,
                    layouts,
                    pipelines,
                    target,
                    scene,
                    table: scene_table,
                    shadow_scene: scene_table.shadow_group(),
                    level_group,
                    atlas_group: atlas.group(),
                    occupancy,
                    shadow: shadow.as_mut(),
                    shadow_keys,
                    volume,
                    clear,
                    surface: surface_pass,
                    oit_resolve,
                    meshes: meshes.as_mut(),
                    splats: splats.as_mut(),
                    sections,
                    overlay,
                    data: &data,
                    view,
                    slice,
                    translucent,
                    volume_visible,
                    surfaces,
                    sample: started.sample,
                    dummy_ao,
                    dummy_oit,
                    dummy_normal,
                    dummy_volume,
                    object_key,
                };
                draw::encode(draw);
            }
            if sampling.measuring() && target.samples > 1 {
                resolve::measure(
                    &mut encoder,
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
                &mut encoder,
                resolve::Accumulate {
                    clear,
                    present,
                    target,
                    region: view.region,
                    reset: started.reset,
                },
            );
            if started.continuing && target.samples == 1 {
                let request = temporal.expect("the seeding frame carries a history request");
                let spacing = pyramid.level(ladder.base).spacing();
                let tolerance =
                    crate::temporal::tolerance(spacing.x().max(spacing.y()).max(spacing.z()));
                let geometry =
                    target.geometry(device, layouts, dummy_normal, dummy_volume, volume_visible);
                resolve::seed(
                    temporal_pass,
                    &mut encoder,
                    resolve::Seed {
                        device,
                        queue,
                        history: target.history.as_ref().expect("the history is captured"),
                        view: &view_now,
                        length: request.history(),
                        tolerance,
                        scene: scene_table.group(),
                        geometry: &geometry,
                        accumulated: &target.accumulation_view,
                        counts: &target.count_view,
                        fresh: target.scene.color(),
                    },
                );
                target.seeded = true;
            }
            target.holds = true;
        } else if !ready {
            target.samples = 0;
            info.samples = 0;
        }
        info.converged =
            ready && (target.samples >= sampling.ceiling() || resolved || started.expired);
        if ready {
            resolve::present(
                &mut encoder,
                resolve::Presenting {
                    device,
                    queue,
                    present,
                    target,
                    surface,
                    region: view.region,
                    exposure,
                    transparent: *transparent,
                    fade,
                },
            );
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
