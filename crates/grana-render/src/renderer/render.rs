use super::signature::{
    channel_style, channel_style_panes, digest, hash_camera, hash_section, shadow_bounds,
    shadow_geometry,
};
use super::*;
use crate::ChannelRow;
use grana_display::ChannelMask;

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
            panes,
            time,
            region: bounds,
            clip,
            exposure,
            samples,
            scene,
            pane_channels,
            pane_quality,
            prefetch,
            level_cap: _,
            assured,
            temporal,
        } = frame;
        for pane in panes.iter().flatten() {
            assert!(inside(pane.region(), size), "the panes lie on the surface");
        }
        let rows: [Vec<ChannelRow>; PANE_COUNT] = std::array::from_fn(|index| match panes[index] {
            Some(_) => pane_rows(scene.channels, pane_channels[index]),
            None => Vec::new(),
        });
        self.time = time;
        self.prefetch = prefetch;
        self.regions = Some(Regions {
            panes: std::array::from_fn(|index| {
                panes[index].map(|pane| PaneRegion {
                    kind: pane.kind(),
                    region: pane.region(),
                })
            }),
        });
        self.info.cut.objects = 0;
        self.info.cut.draws = 0;
        let translucent = scene
            .styles
            .iter()
            .any(|style| style.visible() && style.transparent())
            || scene.markers.iter().any(|marker| marker.transparent());
        let motion: [Option<u64>; PANE_COUNT] = std::array::from_fn(|index| {
            panes[index].map(|pane| {
                pane_motion(
                    pane,
                    bounds,
                    time,
                    pane_channels[index],
                    pane_quality[index],
                    exposure,
                )
            })
        });
        let moved: [bool; PANE_COUNT] =
            std::array::from_fn(|index| motion[index] != self.motion[index]);
        let interactive = moved.iter().any(|moved| *moved);
        self.interactive = interactive;
        let frozen = self.exporting || assured;
        let penalty = self.governor.decide(interactive, !frozen);
        let sequence = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        self.motion = motion;
        self.info.profile.penalty = penalty;
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
            section: section_pass,
            cut,
            slice,
            meshes,
            markers,
            pyramid,
            channels,
            volume_level,
            section_level,
            section_probe,
            section_measure,
            section_sequence,
            probe_mark,
            level_cap,
            feedback,
            frames,
            view_bytes,
            epoch,
            content,
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
            profile,
            sampled,
            oit_readback,
            dropped,
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
        let steps: [f32; PANE_COUNT] =
            std::array::from_fn(|index| scene.step_scale * pane_quality[index].scale() * penalty);
        let active = panes.iter().flatten().count();
        assert!(active > 0, "a frame draws at least one pane");
        let share = (slots / active).max(1);
        let mut levels = Levels {
            panes: [None; PANE_COUNT],
        };
        let mut section_used = 0usize;
        for (index, pane) in panes.iter().enumerate() {
            let Some(PaneFrame::Section { section, region }) = pane else {
                continue;
            };
            let height = region[3].max(1);
            let aspect = region[2].max(1) as f32 / height as f32;
            let section_target = select_section_level(
                pyramid,
                section,
                height as f32,
                if assured { None } else { section_level[index] },
                limits,
            );
            let (resolved, visible) = resolve_section_level(
                pyramid,
                section,
                aspect,
                section_target,
                share,
                bounds,
                limits,
            );
            let pick = if !assured && !*exporting {
                section_probe[index].enter(SectionKey {
                    state: section.state(),
                    aspect,
                    region: bounds,
                    first: limits.first,
                    ceiling: limits.ceiling,
                    capacity: share,
                });
                let measure = section_measure[index].as_ref().map(|(_, measure)| *measure);
                let fresh = section_measure[index]
                    .as_ref()
                    .is_some_and(|(sequence, _)| *sequence > probe_mark[index]);
                section_probe[index].level(
                    section_target,
                    resolved,
                    measure,
                    fresh,
                    share,
                    |level| atlas.hollow_count(time, level),
                )
            } else {
                SectionPick::settled(resolved)
            };
            if pick.started {
                probe_mark[index] = *section_sequence;
            }
            let level = pick.level.min(allowed - 1);
            let needed = info.panes[index].visible.max(info.panes[index].demand);
            let hint = if needed == 0 { visible } else { needed };
            let capacity = if pick.probing {
                share
            } else {
                pick.measured.unwrap_or(hint)
            }
            .clamp(1, share)
            .min(slots.saturating_sub(1).max(1));
            section_used += capacity;
            let fallback =
                neighbour(section_level[index], level, level_count).clamp(first, ceiling);
            levels.panes[index] = Some(PaneLevels::Section(SectionLevels {
                level,
                fallback,
                capacity,
            }));
            section_level[index] = Some(level);
        }
        let section_used = section_used.min(slots.saturating_sub(1).max(1));
        let volume_count = panes
            .iter()
            .filter(|pane| matches!(pane, Some(PaneFrame::Volume { .. })))
            .count();
        let volume_slots = slots.saturating_sub(section_used).max(1);
        let volume_share = volume_slots.checked_div(volume_count).unwrap_or(1).max(1);
        for (index, pane) in panes.iter().enumerate() {
            let Some(PaneFrame::Volume { camera, region, .. }) = pane else {
                continue;
            };
            let height = region[3].max(1);
            let aspect = region[2].max(1) as f32 / height as f32;
            let fit = fit_volume(FitView {
                pyramid,
                camera,
                region: bounds,
                aspect,
                height,
                limits,
                capacity: volume_share,
                current: if assured { None } else { volume_level[index] },
            });
            let base = fit.ladder.base.min(allowed - 1);
            let count = BAND.min(allowed - base);
            let ladder = Ladder { base, count };
            let capacity = if base == fit.ladder.base && count == fit.ladder.count {
                fit.capacity
            } else {
                capacities(volume_share, &fit.capacity, count)
            };
            levels.panes[index] = Some(PaneLevels::Volume(VolumeLevels { ladder, capacity }));
            volume_level[index] = Some(base);
        }
        let residency = Residency::of(
            *epoch,
            Content {
                content: *content,
                style: channel_style_panes(&rows),
            },
            Mode { prefetch, assured },
            time,
            &panes,
            bounds,
            levels,
        );
        let replan = *settled != Some(residency) || planned.as_ref() != Some(&residency);
        info.settled = !replan;
        let decoded =
            panes
                .iter()
                .enumerate()
                .all(|(index, pane)| match (pane, levels.panes[index]) {
                    (Some(PaneFrame::Volume { .. }), Some(PaneLevels::Volume(levels))) => {
                        info.panes[index].level == levels.ladder.base
                    }
                    (Some(PaneFrame::Section { .. }), Some(PaneLevels::Section(levels))) => {
                        info.panes[index].level == levels.level
                    }
                    _ => true,
                });
        info.truncated = decoded
            && panes.iter().enumerate().any(|(index, pane)| {
                pane.is_some() && info.panes[index].visible > info.panes[index].demand
            });
        let ready = !assured || (info.settled && !info.truncated);
        let slot_grid = atlas.slot_grid();
        let mut prepared = [usize::MAX; PANE_COUNT * (BAND + 2)];
        let mut prepared_count = 0;
        for (index, pane) in panes.iter().enumerate() {
            let Some(pane) = pane else {
                continue;
            };
            let needed: Vec<usize> = match (pane, levels.panes[index]) {
                (PaneFrame::Volume { .. }, Some(PaneLevels::Volume(levels))) => {
                    levels.ladder.levels().collect()
                }
                (PaneFrame::Section { .. }, Some(PaneLevels::Section(levels))) => {
                    vec![levels.level, levels.fallback]
                }
                _ => Vec::new(),
            };
            for level in needed {
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
        let step_scale = |index: usize| steps[index];
        let mut volume_data: [Option<FrameData>; PANE_COUNT] = [None; PANE_COUNT];
        let mut section_data: [Option<SectionData>; PANE_COUNT] = [None; PANE_COUNT];
        let mut section_frame: [Option<FrameData>; PANE_COUNT] = [None; PANE_COUNT];
        let mut lenses: [Option<Lens<'_>>; PANE_COUNT] = [None; PANE_COUNT];
        for (index, pane) in panes.iter().enumerate() {
            let Some(pane) = pane else {
                continue;
            };
            match (pane, levels.panes[index]) {
                (
                    PaneFrame::Volume {
                        camera,
                        lens,
                        region,
                    },
                    Some(PaneLevels::Volume(levels)),
                ) => {
                    let tables = levels
                        .ladder
                        .levels()
                        .map(|level| bricks[level].as_ref().expect("the band table exists"))
                        .collect::<Vec<&BrickTable>>();
                    let mut data = FrameData::new(
                        VolumeView {
                            camera,
                            region: bounds,
                            clip,
                            aspect: region[2].max(1) as f32 / region[3].max(1) as f32,
                            height: region[3].max(1),
                            time,
                        },
                        levels.ladder,
                        &tables,
                        scene.shading,
                        step_scale(index),
                        *channels,
                        space,
                    );
                    data.viewport = [
                        region[0] as f32,
                        region[1] as f32,
                        region[2] as f32,
                        region[3] as f32,
                    ];
                    data.oit = [u32::from(translucent), 0, 0, 0];

                    volume_data[index] = Some(data);
                    lenses[index] = Some(*lens);
                }
                (PaneFrame::Section { section, region }, Some(PaneLevels::Section(levels))) => {
                    let table = bricks[levels.level]
                        .as_ref()
                        .expect("the section level table exists");
                    let fallback = bricks[levels.fallback]
                        .as_ref()
                        .expect("the section fallback table exists");
                    section_data[index] = Some(SectionData::new(
                        section,
                        region[2].max(1) as f32 / region[3].max(1) as f32,
                        table,
                        Some(fallback),
                        *channels,
                    ));
                    section_frame[index] = Some(FrameData::shared(SharedView {
                        shading: scene.shading,
                        channels: *channels,
                        region: bounds,
                        clip,
                        time,
                        space,
                    }));
                }
                _ => {}
            }
        }
        for (index, slot) in volume_data.iter_mut().enumerate() {
            let Some(data) = slot else {
                continue;
            };
            let extent = panes[index].as_ref().map_or([1, 1], |pane| {
                [pane.region()[2].max(1), pane.region()[3].max(1)]
            });
            let pane_pixels = u64::from(extent[0]) * u64::from(extent[1]);
            if translucent
                && target.oit[index]
                    .as_ref()
                    .is_none_or(|oit| !oit.fits(size, pane_pixels))
            {
                target.oit[index] = Some(Oit::new(
                    device,
                    layouts,
                    size,
                    pane_pixels,
                    *budget / FRAGMENT_BUDGET_DIVISOR,
                ));
            }
            if let Some(oit) = target.oit[index].as_ref().filter(|_| translucent) {
                data.oit[1] = oit.layers();
                data.oit[3] = oit.tail_scale();
            }
        }
        let (structure, transition, shot) = {
            let signatures: [Option<PaneSignature<'_>>; PANE_COUNT] =
                std::array::from_fn(|index| match (panes[index], &volume_data[index]) {
                    (Some(PaneFrame::Volume { camera, region, .. }), Some(data)) => {
                        Some(PaneSignature::Volume {
                            data,
                            camera: camera.state(),
                            region,
                        })
                    }
                    _ => match (panes[index], &section_data[index]) {
                        (Some(PaneFrame::Section { section, region }), Some(data)) => {
                            Some(PaneSignature::Section {
                                data,
                                view: section.state(),
                                region,
                            })
                        }
                        _ => None,
                    },
                });
            let signature = Signature {
                scene,
                panes: &signatures,
                pane_channels,
                levels,
                time,
                exposure,
                epoch: *epoch,
                content: *content,
                size,
            };
            (signature.digest(), signature.transition(), signature.shot())
        };
        let mut panes_now = FramePanes::default();
        panes_now.collect(&panes, &volume_data);
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
                let previous = target.panes;
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
                        volumes: std::array::from_fn(|index| {
                            target.ao[index].as_ref().map(crate::ao::Ao::volume_normal)
                        }),
                        normals: std::array::from_fn(|index| {
                            target.ao[index].as_ref().map(crate::ao::Ao::normal)
                        }),
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
        *sampled = false;
        let mut marks = Marks::new();
        let first_pane = panes
            .iter()
            .position(Option::is_some)
            .expect("a frame draws a pane");
        if reset {
            meshes.set(device, queue, layouts, scene);
            for (index, pane) in panes.iter().enumerate() {
                if let (Some(PaneFrame::Section { section, region }), Some(data)) =
                    (pane, &section_data[index])
                {
                    let (eye, forward) = cut_view(&panes, section);
                    slice[index].prepare(
                        device,
                        queue,
                        layouts,
                        scene,
                        crate::slice::CutView {
                            section: data,
                            size: [region[2].max(1), region[3].max(1)],
                            eye,
                            forward,
                        },
                    );
                }
            }
            markers.prepare(device, queue, layouts, scene.markers);
            mark(profile, &mut encoder, Stage::Cull, &mut marks);
            for (index, pane) in panes.iter().enumerate() {
                match pane {
                    Some(PaneFrame::Volume { .. }) => {
                        meshes.cull_volume(index, &mut encoder, scene_table.group(index));
                        markers.cull_volume(index, &mut encoder, scene_table.group(index));
                    }
                    Some(PaneFrame::Section { .. }) => {
                        markers.cull_section(index, &mut encoder, scene_table.group(index));
                    }
                    None => {}
                }
            }
            if scene.shading.shadows > 0.0 {
                meshes.cull_shadow(&mut encoder, scene_table.group(first_pane));
            }
        }
        let sample_index = if reset && !assured && temporal.is_some() {
            (sequence % u64::from(samples)) as u32
        } else {
            target.samples.min(samples - 1)
        };
        for (index, slot) in volume_data.iter_mut().enumerate() {
            let Some(data) = slot else {
                continue;
            };
            let extent = panes[index].as_ref().map_or([1, 1], |pane| {
                [pane.region()[2].max(1), pane.region()[3].max(1)]
            });
            let offset = sample::offset(sample_index, extent);
            data.jitter = [
                offset.x,
                offset.y,
                sample_index as f32,
                step_scale(index).max(1.0e-3),
            ];
            if let Some(lens) = lenses[index] {
                data.lens = crate::frame::lens(lens.camera, sample_index, samples, lens.aspect);
            }
        }
        for (index, slot) in section_data.iter_mut().enumerate() {
            let Some(data) = slot else {
                continue;
            };
            let extent = panes[index].as_ref().map_or([1, 1], |pane| {
                [pane.region()[2].max(1), pane.region()[3].max(1)]
            });
            let offset = sample::offset(sample_index, extent);
            data.jitter = [offset.x, offset.y, 0.0, 0.0];
        }
        scene_table.prepare(device, queue, pyramid, scene.channels);
        for index in 0..PANE_COUNT {
            if volume_data[index].is_some() || section_data[index].is_some() {
                scene_table.write_channels(device, queue, pyramid, index, &rows[index]);
                scene_table.write_frame(
                    index,
                    queue,
                    volume_data[index].or(section_frame[index]),
                    section_data[index],
                );
            }
        }
        if replan {
            for (index, pane) in panes.iter().enumerate() {
                let Some(pane) = pane else {
                    continue;
                };
                let panes_plan = &mut planner.panes[index];
                match (pane, levels.panes[index]) {
                    (
                        PaneFrame::Volume { camera, region, .. },
                        Some(PaneLevels::Volume(levels)),
                    ) => {
                        let tables = levels
                            .ladder
                            .levels()
                            .map(|level| bricks[level].as_ref().expect("the band table exists"))
                            .collect::<Vec<&BrickTable>>();
                        let scale = bucket_scale(
                            camera,
                            tables[0].bounds.0.to_array(),
                            tables[0].world.to_array(),
                        );
                        for band in 0..BAND {
                            let (capacity, dispatch, base) = match tables.get(band) {
                                Some(table) => (
                                    levels.capacity.get(band).copied().unwrap_or(0) as u32,
                                    volume_dispatch(table),
                                    table.record_base(),
                                ),
                                None => (0, 0, 0),
                            };
                            panes_plan.bands[band].prepare(
                                queue,
                                Schedule {
                                    time,
                                    band: band as u32,
                                    capacity,
                                    range: SectionRange::default(),
                                    dispatch,
                                    bucket_scale: scale,
                                    base,
                                },
                            );
                        }
                        panes_plan.section.prepare(
                            queue,
                            Schedule {
                                time,
                                band: 0,
                                capacity: 0,
                                range: SectionRange::default(),
                                dispatch: 0,
                                bucket_scale: 0.0,
                                base: 0,
                            },
                        );
                        let _ = region;
                    }
                    (PaneFrame::Section { section, region }, Some(PaneLevels::Section(levels))) => {
                        let table = bricks[levels.level]
                            .as_ref()
                            .expect("the section table exists");
                        let range = section_range(
                            pyramid,
                            levels.level,
                            section,
                            region[2].max(1) as f32 / region[3].max(1) as f32,
                            bounds,
                        );
                        panes_plan.section.prepare(
                            queue,
                            Schedule {
                                time,
                                band: 0,
                                capacity: levels.capacity as u32,
                                range,
                                dispatch: range.dispatch(),
                                bucket_scale: 0.0,
                                base: table.record_base(),
                            },
                        );
                        for band in 0..BAND {
                            panes_plan.bands[band].prepare(
                                queue,
                                Schedule {
                                    time,
                                    band: band as u32,
                                    capacity: 0,
                                    range: SectionRange::default(),
                                    dispatch: 0,
                                    bucket_scale: 0.0,
                                    base: 0,
                                },
                            );
                        }
                    }
                    _ => {}
                }
            }
            mark(profile, &mut encoder, Stage::Plan, &mut marks);
            let groups: [&BindGroup; PANE_COUNT] =
                std::array::from_fn(|index| scene_table.group(index));
            planner.encode(&mut encoder, &groups);
            *planned = Some(residency);
        }
        if target.samples < samples && ready {
            target.samples += 1;
            info.samples = target.samples;
            *sampled = true;
            let surfaces = !scene.layers.is_empty() || !scene.markers.is_empty();
            let volume_visible: [bool; PANE_COUNT] = std::array::from_fn(|index| {
                rows[index]
                    .iter()
                    .any(|row| row.visible && row.opacity > 0.0)
            });
            for (index, pane) in panes.iter().enumerate() {
                let Some(PaneFrame::Volume { region, .. }) = pane else {
                    continue;
                };
                if !surfaces && !volume_visible[index] {
                    continue;
                }
                let extent = [region[2].max(1), region[3].max(1)];
                if target.ao[index]
                    .as_ref()
                    .is_none_or(|ao| ao.size() != extent || ao.volume_size() != size)
                {
                    target.ao[index] = Some(crate::ao::Ao::new(
                        device,
                        layouts,
                        gtao.pipeline(),
                        extent,
                        size,
                    ));
                }
                let ao = target.ao[index]
                    .as_ref()
                    .expect("the ambient occlusion target is ready");
                if reset {
                    ao.clear_volume(&mut encoder);
                }
                ao.clear(&mut encoder);
                meshes.draw_gbuffer(
                    index,
                    &mut encoder,
                    ao.normal(),
                    ao.depth(),
                    *region,
                    scene_table.group(index),
                );
                markers.draw_gbuffer(
                    index,
                    &mut encoder,
                    ao.normal(),
                    ao.depth(),
                    *region,
                    scene_table.group(index),
                );
                ao.evaluate(
                    queue,
                    &mut encoder,
                    &volume_data[index].expect("the volume data is ready"),
                    AMBIENT_REACH,
                    AMBIENT_STRENGTH,
                    sample_index,
                );
            }
            if let Some(data) = panes.iter().enumerate().find_map(|(index, pane)| {
                matches!(pane, Some(PaneFrame::Volume { .. }))
                    .then(|| volume_data[index])
                    .flatten()
            }) {
                scene_table.write_shadow_frame(queue, data);
            }
            let channel_key = channel_style(scene.channels);
            mark(profile, &mut encoder, Stage::Shadow, &mut marks);
            if let Some(shadow) = shadow.as_mut() {
                let groups = ShadowGroups {
                    scene: scene_table.shadow_group(),
                    level: level_group,
                    atlas: atlas.group(),
                };
                if scene.shading.shadows > 0.0 {
                    let object = digest(&[shadow_geometry(scene), space.digest()]);
                    if *object_key != Some(object) {
                        *object_key = Some(object);
                        meshes.draw_shadow(&mut encoder, shadow.objects());
                        markers.draw_shadow(
                            &mut encoder,
                            shadow.objects(),
                            scene_table.group(first_pane),
                        );
                    }
                    let ladder = panes
                        .iter()
                        .enumerate()
                        .find_map(|(index, pane)| match (pane, levels.panes[index]) {
                            (Some(PaneFrame::Volume { .. }), Some(PaneLevels::Volume(levels))) => {
                                Some(levels.ladder)
                            }
                            _ => None,
                        })
                        .unwrap_or(Ladder {
                            base: first,
                            count: 0,
                        });
                    shadow.encode_tau(
                        &mut encoder,
                        groups,
                        digest(&[
                            *content,
                            channel_key,
                            ladder.base as u64,
                            ladder.count as u64,
                            space.digest(),
                        ]),
                    );
                }
            }
            mark(profile, &mut encoder, Stage::Geometry, &mut marks);
            crate::target::clear(&mut encoder, target.scene.color(), target.scene.depth());
            let shadow_group = shadow
                .as_ref()
                .map(Shadow::group)
                .expect("the shadow is ready");
            for (index, pane) in panes.iter().enumerate() {
                let Some(pane) = pane else {
                    continue;
                };
                let group = scene_table.group(index);
                match pane {
                    PaneFrame::Volume { region, .. } => {
                        let data = volume_data[index].expect("the volume data is ready");
                        meshes.write_uniforms(index, queue, &data, scene.shading);
                        let ao_group: &BindGroup = target.ao[index]
                            .as_ref()
                            .map_or(dummy_ao, crate::ao::Ao::group);
                        let oit = target.oit[index].as_ref().filter(|_| translucent);
                        let oit_group: &BindGroup =
                            oit.map_or(dummy_oit.read_group(), Oit::read_group);
                        let (width, height) = (region[2].max(1), region[3].max(1));
                        meshes.draw_volume(
                            index,
                            &mut encoder,
                            VolumeTargets {
                                color: target.scene.color(),
                                depth: target.scene.depth(),
                            },
                            *region,
                            Lighting {
                                shadow: shadow_group,
                                ao: ao_group,
                            },
                        );
                        markers.draw_volume(
                            index,
                            &mut encoder,
                            VolumeTargets {
                                color: target.scene.color(),
                                depth: target.scene.depth(),
                            },
                            *region,
                            group,
                            Lighting {
                                shadow: shadow_group,
                                ao: ao_group,
                            },
                        );
                        if let Some(oit) = oit {
                            mark(profile, &mut encoder, Stage::Oit, &mut marks);
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
                                    depth_stencil_attachment: Some(
                                        RenderPassDepthStencilAttachment {
                                            view: target.scene.depth(),
                                            depth_ops: Some(Operations {
                                                load: LoadOp::Load,
                                                store: StoreOp::Store,
                                            }),
                                            stencil_ops: None,
                                        },
                                    ),
                                    ..Default::default()
                                });
                                pass.set_viewport(
                                    region[0] as f32,
                                    region[1] as f32,
                                    width as f32,
                                    height as f32,
                                    0.0,
                                    1.0,
                                );
                                pass.set_scissor_rect(region[0], region[1], width, height);
                                meshes.draw_oit(
                                    index,
                                    &mut pass,
                                    shadow_group,
                                    oit.write_group(),
                                    ao_group,
                                );
                                markers.draw_oit(
                                    index,
                                    &mut pass,
                                    group,
                                    oit.write_group(),
                                    ao_group,
                                );
                            }
                            {
                                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                                    label: Some("grana.oit.color.pass"),
                                    color_attachments: &[],
                                    depth_stencil_attachment: Some(
                                        RenderPassDepthStencilAttachment {
                                            view: target.scene.depth(),
                                            depth_ops: Some(Operations {
                                                load: LoadOp::Load,
                                                store: StoreOp::Store,
                                            }),
                                            stencil_ops: None,
                                        },
                                    ),
                                    ..Default::default()
                                });
                                pass.set_viewport(
                                    region[0] as f32,
                                    region[1] as f32,
                                    width as f32,
                                    height as f32,
                                    0.0,
                                    1.0,
                                );
                                pass.set_scissor_rect(region[0], region[1], width, height);
                                meshes.draw_color(
                                    index,
                                    &mut pass,
                                    shadow_group,
                                    oit.write_group(),
                                    oit.coverage_group(),
                                    ao_group,
                                );
                                markers.draw_color(
                                    index,
                                    &mut pass,
                                    group,
                                    oit.write_group(),
                                    oit.coverage_group(),
                                    ao_group,
                                );
                            }
                            {
                                let tail = oit.tail_pane(*region);
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
                                pass.set_viewport(
                                    0.0,
                                    0.0,
                                    tail[0] as f32,
                                    tail[1] as f32,
                                    0.0,
                                    1.0,
                                );
                                pass.set_scissor_rect(0, 0, tail[0], tail[1]);
                                meshes.draw_tail(
                                    index,
                                    &mut pass,
                                    shadow_group,
                                    oit.write_group(),
                                    oit.coverage_group(),
                                    ao_group,
                                );
                                markers.draw_tail(
                                    index,
                                    &mut pass,
                                    group,
                                    oit.write_group(),
                                    oit.coverage_group(),
                                    ao_group,
                                );
                            }
                            if let Some(slot) = oit_readback[index].reserve() {
                                oit_readback[index].stage(&mut encoder, slot, 0, oit.dropped(), 4);
                            }
                        }
                        mark(profile, &mut encoder, Stage::Volume, &mut marks);
                        if volume_visible[index] {
                            let normal = target.ao[index]
                                .as_ref()
                                .expect("the ambient occlusion target is ready")
                                .volume_normal();
                            let volume_frame = crate::volume::VolumeFrame {
                                target: target.scene.color(),
                                normal,
                                region: *region,
                                clear: false,
                                scene: group,
                                level: level_group,
                                atlas: atlas.group(),
                                depth_group: target.scene.depth_group(),
                                shadow: shadow_group,
                                oit: oit_group,
                                ao: ao_group,
                            };
                            volume.draw(&mut encoder, volume_frame);
                        }
                    }
                    PaneFrame::Section { region, .. } => {
                        let (width, height) = (region[2].max(1), region[3].max(1));
                        mark(profile, &mut encoder, Stage::Section, &mut marks);
                        section_pass.draw(
                            &mut encoder,
                            crate::section::SectionFrame {
                                target: target.scene.color(),
                                region: *region,
                                scene: group,
                                level: level_group,
                                atlas: atlas.group(),
                            },
                        );
                        mark(profile, &mut encoder, Stage::Cut, &mut marks);
                        cut[index].draw(device, &mut encoder, [width, height], |pass| {
                            markers.draw_section_behind(index, pass, group);
                            slice[index].draw(pass, group);
                            markers.draw_section_front(index, pass, group);
                        });
                        if slice[index].objects() > 0 {
                            cut[index].outline(
                                &mut encoder,
                                [width, height],
                                slice[index].style_group(),
                            );
                        }
                        cut[index].composite(&mut encoder, target.scene.color(), *region);
                        info.cut.objects += slice[index].objects();
                        info.cut.draws +=
                            slice[index].draws() + 1 + u32::from(slice[index].objects() > 0);
                    }
                }
            }
            mark(profile, &mut encoder, Stage::Present, &mut marks);
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
                let tolerances: [f32; PANE_COUNT] =
                    std::array::from_fn(|index| match levels.panes[index] {
                        Some(PaneLevels::Volume(levels)) => {
                            let spacing = pyramid.level(levels.ladder.base).spacing();
                            crate::temporal::tolerance(
                                spacing.x().max(spacing.y()).max(spacing.z()),
                            )
                        }
                        _ => 0.0,
                    });
                temporal_pass.seed(
                    device,
                    queue,
                    &mut encoder,
                    crate::temporal::Reuse {
                        history,
                        panes: &panes_now,
                        length: request.history(),
                        tolerances,
                        inputs: &crate::temporal::Inputs {
                            accumulated: &target.accumulation_view,
                            counts: &target.count_view,
                            fresh: target.scene.color(),
                            depth: target.scene.depth(),
                            volumes: std::array::from_fn(|index| {
                                target.ao[index].as_ref().map(crate::ao::Ao::volume_normal)
                            }),
                            normals: std::array::from_fn(|index| {
                                target.ao[index].as_ref().map(crate::ao::Ao::normal)
                            }),
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
            for index in 0..PANE_COUNT {
                let base = (index * (BAND + 1)) as u64 * *view_bytes;
                for band in 0..BAND {
                    feedback.stage(
                        &mut encoder,
                        slot,
                        base + band as u64 * *view_bytes,
                        planner.panes[index].bands[band].buffer(),
                        *view_bytes,
                    );
                }
                feedback.stage(
                    &mut encoder,
                    slot,
                    base + BAND as u64 * *view_bytes,
                    planner.panes[index].section.buffer(),
                    *view_bytes,
                );
            }
            frames[slot] = Some(residency);
        }
        if let Some(profiler) = profile.as_mut() {
            profiler.finish(&mut encoder, marks, penalty);
        }
        queue.submit([encoder.finish()]);
        convergence.register();
        if let Some(feedback) = feedback.as_mut() {
            feedback.register();
        }
        if let Some(profiler) = profile.as_mut() {
            profiler.register();
        }
        for readback in oit_readback.iter_mut() {
            readback.register();
        }
        for (index, pane) in panes.iter().enumerate() {
            match (pane, levels.panes[index]) {
                (Some(PaneFrame::Volume { .. }), Some(PaneLevels::Volume(levels))) => {
                    info.panes[index].level = levels.ladder.base;
                }
                (Some(PaneFrame::Section { .. }), Some(PaneLevels::Section(levels))) => {
                    info.panes[index].level = levels.level;
                }
                _ => {}
            }
        }
        info.bytes = atlas.bytes();
        info.geometry = meshes.bytes();
        info.fragments = target.oit.iter().flatten().map(Oit::bytes).sum::<u64>();
        info.oit.layers = target
            .oit
            .iter()
            .flatten()
            .map(Oit::layers)
            .max()
            .unwrap_or(0);
        info.oit.dropped = dropped.iter().sum();
        info.cut.indices = slice.iter().map(SlicePass::index_bytes).sum();
        info.seeded = target.seeded;
        info.history = temporal.is_some()
            && target
                .history
                .as_ref()
                .is_some_and(crate::temporal::History::ready);
        target.panes = panes_now;
        *info
    }
}

fn pane_motion(
    pane: PaneFrame<'_>,
    clip: Region,
    time: u32,
    channels: ChannelMask,
    quality: Quality,
    exposure: f32,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    match pane {
        PaneFrame::Volume {
            camera,
            lens,
            region,
        } => {
            hash_camera(camera.state(), &mut hasher);
            lens.aspect.to_bits().hash(&mut hasher);
            region.hash(&mut hasher);
        }
        PaneFrame::Section { section, region } => {
            hash_section(section.state(), &mut hasher);
            region.hash(&mut hasher);
        }
    }
    time.hash(&mut hasher);
    channels.hidden().hash(&mut hasher);
    quality.index().hash(&mut hasher);
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

fn cut_view(panes: &[Option<PaneFrame<'_>>; PANE_COUNT], section: &SectionCamera) -> (Vec3, Vec3) {
    if let Some(PaneFrame::Volume { camera, .. }) =
        panes.iter().flatten().find(|pane| pane.kind().volume())
    {
        (camera.eye(), camera.forward())
    } else {
        (
            section.center() + section.normal() * 1.0e3,
            -section.normal(),
        )
    }
}

fn mark(profile: &Option<Profiler>, encoder: &mut CommandEncoder, stage: Stage, marks: &mut Marks) {
    if let Some(profiler) = profile.as_ref() {
        profiler.mark(encoder, stage, marks);
    }
}

fn pane_rows(channels: &[ChannelRow], mask: ChannelMask) -> Vec<ChannelRow> {
    channels
        .iter()
        .enumerate()
        .map(|(index, channel)| ChannelRow {
            visible: channel.visible && mask.shown(index),
            ..channel.clone()
        })
        .collect()
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
