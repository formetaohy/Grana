use super::signature::{channel_style, digest, hash_camera, shadow_bounds};
use super::*;

const AMBIENT_REACH: f32 = 40.0;
const AMBIENT_STRENGTH: f32 = 0.9;

impl Renderer {
    pub(super) fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        target: &mut FrameTarget,
        frame: Frame<'_, '_>,
    ) -> FrameInfo {
        let Frame {
            target: surface,
            size,
            camera,
            viewport,
            time,
            region: bounds,
            clip,
            exposure,
            samples,
            scene,
            prefetch,
            level_cap: _,
            assured,
            temporal,
        } = frame;
        assert!(inside(viewport, size), "the view lies on the surface");
        self.time = time;
        self.prefetch = prefetch;
        self.viewport = Some(viewport);
        let slice = camera.slice().is_some();
        let slice_objects = slice && (!scene.layers.is_empty() || !scene.markers.is_empty());
        let temporal = temporal.filter(|_| !slice_objects);
        let translucent = !slice
            && (scene
                .styles
                .iter()
                .any(|style| style.visible() && style.transparent())
                || scene.markers.iter().any(|marker| marker.transparent()));
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
            gtao,
            meshes,
            markers,
            pyramid,
            channels,
            volume_level,
            volume_measure,
            volume_rejection,
            level_cap,
            feedback,
            frames,
            view_bytes,
            epoch,
            resident,
            uploads,
            settled,
            planned,
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
            oit_readback,
            sections,
            sliced,
            occupancy,
            occupancy_group,
            bake,
            baked,
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
            return *info;
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
            layouts,
            scene,
            camera.slice().map(|frame| frame.plane(camera.pivot())),
        );
        atlas.prune(time);
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
            height,
            limits,
            capacity: share,
            current,
        };
        let (fit, overflowed) = if measured {
            fit_measured(view, volume_measure, rejected)
        } else {
            (fit_volume(view), None)
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
        let content = {
            let mut hasher = DefaultHasher::new();
            scene.revision.hash(&mut hasher);
            scene.step_scale.to_bits().hash(&mut hasher);
            scene.channels.len().hash(&mut hasher);
            scene.layers.len().hash(&mut hasher);
            scene.styles.len().hash(&mut hasher);
            scene.markers.len().hash(&mut hasher);
            hasher.finish()
        };
        let stamp = digest(&[content, *uploads]);
        let fresh = *resident != Some(content);
        let residency = Residency::of(
            *epoch,
            Content {
                content: stamp,
                style: channel_style(scene.channels),
            },
            Mode { prefetch, assured },
            time,
            ViewState {
                camera,
                size: [viewport[2], viewport[3]],
                region: bounds,
                levels,
            },
        );
        let replan = *settled != Some(residency) || planned.as_ref() != Some(&residency);
        info.settled = !replan;
        info.truncated = info.view.level == ladder.base && info.view.visible > info.view.demand;
        let ready = !assured || (info.settled && !info.truncated);
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
                table.rebase(queue, time, atlas, arena);
            }
        }
        let finest = pyramid.level(0);
        let world = Vec3::from(finest.world_extent());
        let spacing = finest.spacing();
        let (shadow_low, shadow_high) = shadow_bounds(scene, world);
        let space = ShadowSpace::new(
            scene.shading.light(),
            shadow_low,
            shadow_high,
            Vec3::new(spacing.x(), spacing.y(), spacing.z()),
        );
        let tables = ladder
            .levels()
            .map(|level| bricks[level].as_ref().expect("the band table exists"))
            .collect::<Vec<&BrickTable>>();
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
            scene.step_scale,
            *channels,
            space,
        );
        data.viewport = [
            viewport[0] as f32,
            viewport[1] as f32,
            viewport[2] as f32,
            viewport[3] as f32,
        ];
        data.oit = [u32::from(translucent), 0, 0, 0];
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
            data.oit[3] = oit.tail_scale();
        }
        let signature = Signature {
            data: &data,
            camera: camera.state(),
            region: viewport,
            levels,
            content,
            uploads: *uploads,
            time,
            exposure,
            epoch: *epoch,
            size,
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
            && samples > 1
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
                        volume: target.ao.as_ref().map(crate::ao::Ao::volume_normal),
                        normal: target.ao.as_ref().map(crate::ao::Ao::normal),
                    },
                );
                seeding = true;
            } else {
                target.holds = false;
                TemporalPass::invalidate(&mut target.history);
            }
            target.shot = Some(shot);
        }
        if reset {
            convergence.reset();
        }
        info.samples = target.samples;
        if reset {
            info.rebuilt = fresh;
            if fresh {
                meshes.set(device, queue, layouts, scene);
            }
            if fresh {
                markers.set(device, queue, layouts, scene.markers);
                *resident = Some(content);
            }
            markers.rewind(queue);
            markers.cull_volume(&mut encoder, scene_table.group());
            if !slice {
                meshes.cull_volume(&mut encoder, scene_table.group());
                if scene.shading.shadows > 0.0 {
                    meshes.cull_shadow(&mut encoder, scene_table.group());
                }
            }
        }
        let sample_index = if reset && !assured && temporal.is_some() {
            (sequence % u64::from(samples)) as u32
        } else {
            target.samples.min(samples - 1)
        };
        let offset = sample::offset(sample_index, [width, height]);
        data.jitter = [
            offset.x,
            offset.y,
            sample_index as f32,
            scene.step_scale.max(1.0e-3),
        ];
        data.lens = crate::frame::lens(camera, sample_index, samples, aspect);
        if fresh {
            scene_table.prepare(device, queue, pyramid, scene.channels);
        }
        if fresh {
            scene_table.write_channels(device, queue, pyramid, scene.channels);
        }
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
            *planned = Some(residency);
        }
        if target.samples < samples && ready {
            target.samples += 1;
            info.samples = target.samples;
            let surfaces = !scene.layers.is_empty() || !scene.markers.is_empty();
            let volume_visible = scene
                .channels
                .iter()
                .any(|row| row.visible && row.opacity > 0.0);
            let projected = scene.channels.iter().any(|row| {
                row.visible
                    && row.opacity > 0.0
                    && row.volume.mode() == grana_display::VolumeMode::Maximum
            });
            if surfaces || volume_visible {
                if target
                    .ao
                    .as_ref()
                    .is_none_or(|ao| ao.size() != [width, height] || ao.volume_size() != size)
                {
                    target.ao = Some(crate::ao::Ao::new(
                        device,
                        layouts,
                        gtao.pipeline(),
                        [width, height],
                        size,
                    ));
                }
                let ao = target
                    .ao
                    .as_ref()
                    .expect("the ambient occlusion target is ready");
                if reset {
                    ao.clear_volume(&mut encoder);
                }
                ao.clear(&mut encoder);
                if !slice {
                    meshes.draw_gbuffer(
                        &mut encoder,
                        ao.normal(),
                        ao.depth(),
                        viewport,
                        scene_table.group(),
                    );
                    markers.draw_gbuffer(
                        &mut encoder,
                        ao.normal(),
                        ao.depth(),
                        viewport,
                        scene_table.group(),
                    );
                    ao.evaluate(
                        queue,
                        &mut encoder,
                        &data,
                        AMBIENT_REACH,
                        AMBIENT_STRENGTH,
                        sample_index,
                    );
                }
            }
            scene_table.write_shadow_frame(queue, data);
            let channel_key = channel_style(scene.channels);
            if let Some(shadow) = shadow.as_mut() {
                let groups = ShadowGroups {
                    scene: scene_table.shadow_group(),
                    level: level_group,
                    atlas: atlas.group(),
                };
                if !slice && scene.shading.shadows > 0.0 {
                    let object = digest(&[content, space.digest()]);
                    if *object_key != Some(object) {
                        *object_key = Some(object);
                        meshes.draw_shadow(&mut encoder, shadow.objects());
                        markers.draw_shadow(&mut encoder, shadow.objects(), scene_table.group());
                    }
                    shadow.encode_tau(
                        &mut encoder,
                        groups,
                        digest(&[
                            content,
                            channel_key,
                            ladder.base as u64,
                            ladder.count as u64,
                            space.digest(),
                        ]),
                    );
                }
            }
            crate::target::clear(&mut encoder, target.scene.color(), target.scene.depth());
            let shadow_group = shadow
                .as_ref()
                .map(Shadow::group)
                .expect("the shadow is ready");
            let group = scene_table.group();
            meshes.write_uniforms(queue, &data, scene.shading);
            let ao_group: &BindGroup = target.ao.as_ref().map_or(dummy_ao, crate::ao::Ao::group);
            let oit = target.oit.as_ref().filter(|_| translucent);
            let oit_group: &BindGroup = oit.map_or(dummy_oit.read_group(), Oit::read_group);
            if slice {
                if volume_visible {
                    let normal = target
                        .ao
                        .as_ref()
                        .expect("the ambient occlusion target is ready")
                        .volume_normal();
                    volume.draw(
                        &mut encoder,
                        crate::volume::VolumeFrame {
                            target: target.scene.color(),
                            normal,
                            region: viewport,
                            scene: group,
                            level: level_group,
                            atlas: atlas.group(),
                            depth_group: target.scene.depth_group(),
                            oit: oit_group,
                            occupancy: occupancy_group.as_ref().expect("the occupancy is ready"),
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
                markers.draw_slice(
                    &mut encoder,
                    VolumeTargets {
                        color: target.scene.color(),
                        depth: target.scene.depth(),
                    },
                    viewport,
                    group,
                    Lighting {
                        shadow: shadow_group,
                        ao: ao_group,
                    },
                );
            } else {
                meshes.draw_volume(
                    &mut encoder,
                    VolumeTargets {
                        color: target.scene.color(),
                        depth: target.scene.depth(),
                    },
                    viewport,
                    Lighting {
                        shadow: shadow_group,
                        ao: ao_group,
                    },
                );
                markers.draw_volume(
                    &mut encoder,
                    VolumeTargets {
                        color: target.scene.color(),
                        depth: target.scene.depth(),
                    },
                    viewport,
                    group,
                    Lighting {
                        shadow: shadow_group,
                        ao: ao_group,
                    },
                );
            }
            if let Some(oit) = oit {
                oit.clear(&mut encoder);
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("grana.oit.key.pass"),
                        color_attachments: &[Some(RenderPassColorAttachment {
                            view: oit.coverage(),
                            depth_slice: None,
                            resolve_target: None,
                            ops: Operations {
                                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: StoreOp::Store,
                            },
                        })],
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
                    meshes.draw_oit(&mut pass, shadow_group, oit.write_group(), ao_group);
                    markers.draw_oit(&mut pass, group, oit.write_group(), ao_group);
                }
                {
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("grana.oit.color.pass"),
                        color_attachments: &[],
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
                    meshes.draw_color(
                        &mut pass,
                        shadow_group,
                        oit.write_group(),
                        oit.coverage_group(),
                        ao_group,
                    );
                    markers.draw_color(
                        &mut pass,
                        group,
                        oit.write_group(),
                        oit.coverage_group(),
                        ao_group,
                    );
                }
                {
                    let tail = oit.tail_size(viewport);
                    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                        label: Some("grana.oit.pass"),
                        color_attachments: &[
                            Some(RenderPassColorAttachment {
                                view: oit.accumulate(),
                                depth_slice: None,
                                resolve_target: None,
                                ops: Operations {
                                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: StoreOp::Store,
                                },
                            }),
                            Some(RenderPassColorAttachment {
                                view: oit.reveal(),
                                depth_slice: None,
                                resolve_target: None,
                                ops: Operations {
                                    load: LoadOp::Clear(wgpu::Color::WHITE),
                                    store: StoreOp::Store,
                                },
                            }),
                            Some(RenderPassColorAttachment {
                                view: oit.tail_depth(),
                                depth_slice: None,
                                resolve_target: None,
                                ops: Operations {
                                    load: LoadOp::Clear(wgpu::Color {
                                        r: 1.0,
                                        g: 1.0,
                                        b: 1.0,
                                        a: 1.0,
                                    }),
                                    store: StoreOp::Store,
                                },
                            }),
                        ],
                        depth_stencil_attachment: None,
                        ..Default::default()
                    });
                    pass.set_viewport(0.0, 0.0, tail[0] as f32, tail[1] as f32, 0.0, 1.0);
                    pass.set_scissor_rect(0, 0, tail[0], tail[1]);
                    meshes.draw_tail(
                        &mut pass,
                        shadow_group,
                        oit.write_group(),
                        oit.coverage_group(),
                        ao_group,
                    );
                    markers.draw_tail(
                        &mut pass,
                        group,
                        oit.write_group(),
                        oit.coverage_group(),
                        ao_group,
                    );
                }
                if let Some(slot) = oit_readback.reserve() {
                    oit_readback.stage(&mut encoder, slot, 0, oit.dropped(), 4);
                }
            }
            if volume_visible && !slice {
                if let (Some(bake), Some(occupancy)) = (bake.as_ref(), occupancy.as_mut()) {
                    let key = digest(&[
                        content,
                        channel_style(scene.channels),
                        ladder.base as u64,
                        ladder.count as u64,
                        u64::from(time),
                        space.digest(),
                        u64::from(scene.shading.ambient.to_bits()),
                        u64::from(scene.shading.diffuse.to_bits()),
                        object_key.unwrap_or(u64::MAX),
                    ]);
                    let levels = ladder.levels().collect::<Vec<usize>>();
                    let entries = baked.collect(key, atlas, &levels, time, occupancy, queue);
                    if !entries.is_empty() {
                        bake.stage(queue, entries);
                        bake.encode(
                            &mut encoder,
                            entries.len() as u32,
                            group,
                            level_group,
                            atlas.source(),
                            shadow_group,
                        );
                    }
                }
                let normal = target
                    .ao
                    .as_ref()
                    .expect("the ambient occlusion target is ready")
                    .volume_normal();
                let volume_frame = crate::volume::VolumeFrame {
                    target: target.scene.color(),
                    normal,
                    region: viewport,
                    scene: group,
                    level: level_group,
                    atlas: atlas.group(),
                    depth_group: target.scene.depth_group(),
                    oit: oit_group,
                    occupancy: occupancy_group.as_ref().expect("the occupancy is ready"),
                };
                let path = if projected {
                    crate::volume::VolumePath::Projection
                } else {
                    crate::volume::VolumePath::Composite
                };
                volume.draw(&mut encoder, volume_frame, path);
            }
            if *exporting && target.samples > 1 {
                convergence.encode(
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
                            volume: target.ao.as_ref().map(crate::ao::Ao::volume_normal),
                            normal: target.ao.as_ref().map(crate::ao::Ao::normal),
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
        if replan
            && let Some(feedback) = feedback.as_mut()
            && let Some(slot) = feedback.reserve()
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
        }
        queue.submit([encoder.finish()]);
        convergence.register();
        if let Some(feedback) = feedback.as_mut() {
            feedback.register();
        }
        oit_readback.register();
        info.view.level = ladder.base;
        info.seeded = target.seeded;
        info.history = temporal.is_some()
            && target
                .history
                .as_ref()
                .is_some_and(crate::temporal::History::ready);
        target.view = view_now;
        *info
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
