use super::residency::ResidencyView;
use super::signature::{channel_style, digest, scene_styles, shadow_bounds, shadow_geometry};
use super::*;

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
            volume_region,
            section_region,
            time,
            camera,
            section,
            region,
            clip,
            exposure,
            samples,
            scene,
            prefetch,
            level_cap: _,
        } = frame;
        assert!(
            inside(volume_region, size) && inside(section_region, size),
            "the panes lie on the surface"
        );
        self.time = time;
        self.prefetch = prefetch;
        self.regions = Some(Regions {
            volume: volume_region,
            section: section_region,
        });
        self.info.cut.objects = 0;
        self.info.cut.draws = 0;
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
            section: section_pass,
            cut,
            slice,
            meshes,
            markers,
            pyramid,
            channels,
            volume_level: volume_rendered,
            section_level: section_rendered,
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
            fade,
            object_key,
            info,
            dummy_oit,
            budget,
            oit_readback,
            dropped,
            wanted,
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
        let volume_width = volume_region[2].max(1);
        let volume_height = volume_region[3].max(1);
        let section_width = section_region[2].max(1);
        let section_height = section_region[3].max(1);
        let volume_aspect = volume_width as f32 / volume_height as f32;
        let section_aspect = section_width as f32 / section_height as f32;
        let cut_size = [section_width, section_height];
        let slots = atlas.slots();
        let level_count = pyramid.level_count();
        let first = arena.first();
        let ceiling = level_cap
            .unwrap_or(level_count - 1)
            .max(first)
            .min(level_count - 1);
        let limits = Limits { first, ceiling };
        let section_target_level = select_section_level(
            pyramid,
            section,
            section_height as f32,
            *section_rendered,
            limits,
        );
        let (section_level, section_count) = resolve_section_level(
            pyramid,
            section,
            section_aspect,
            section_target_level,
            (slots / SECTION_BUDGET_DIVISOR).max(1),
            region,
            limits,
        );
        let section_capacity = section_count.clamp(1, slots.saturating_sub(1).max(1));
        let volume_capacity = slots.saturating_sub(section_capacity).max(1);
        let ladder = volume_ladder(
            pyramid,
            camera,
            volume_height as f32,
            *volume_rendered,
            limits,
            info.volume.visible,
            volume_capacity,
        );
        let capacity = capacities(volume_capacity, wanted, ladder.count);
        let levels = Levels {
            ladder,
            capacity,
            section: section_level,
            section_fallback: neighbour(*section_rendered, section_level, level_count)
                .clamp(first, ceiling),
            section_capacity,
        };
        *volume_rendered = Some(ladder.base);
        *section_rendered = Some(section_level);
        let residency = Residency::of(
            *epoch,
            *content,
            channel_style(scene.channels),
            prefetch,
            time,
            ResidencyView {
                camera,
                section,
                volume_region,
                section_region,
                region,
            },
            levels,
        );
        let replan = *settled != Some(residency) || planned.as_ref() != Some(&residency);
        info.settled = !replan;
        let slot_grid = atlas.slot_grid();
        let mut prepared = [usize::MAX; BAND + 2];
        let mut count = 0;
        for level in ladder
            .levels()
            .chain([section_level, levels.section_fallback])
        {
            if prepared[..count].contains(&level) {
                continue;
            }
            prepared[count] = level;
            count += 1;
            ensure_brick(bricks, pyramid.level(level), level, slot_grid, arena);
            let table = bricks[level].as_mut().expect("the brick table exists");
            if table.time() != Some(time) {
                table.rebase(queue, time, atlas, arena);
            }
        }
        let tables = ladder
            .levels()
            .map(|level| bricks[level].as_ref().expect("the band table exists"))
            .collect::<Vec<&BrickTable>>();
        let section_table = bricks[section_level]
            .as_ref()
            .expect("the section level table exists");
        let section_fallback = bricks[levels.section_fallback]
            .as_ref()
            .expect("the section fallback table exists");
        let mut section_data = SectionData::new(
            section,
            section_aspect,
            section_table,
            Some(section_fallback),
            *channels,
        );
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
        let mut frame_data = FrameData::new(
            VolumeView {
                camera,
                region,
                clip,
                aspect: volume_aspect,
                height: volume_height,
                time,
            },
            ladder,
            &tables,
            scene.shading,
            scene.step_scale,
            *channels,
            space,
        );
        let translucent = scene
            .styles
            .iter()
            .any(|style| style.visible() && style.transparent())
            || scene.markers.iter().any(|marker| marker.transparent());
        frame_data.viewport = [
            volume_region[0] as f32,
            volume_region[1] as f32,
            volume_region[2] as f32,
            volume_region[3] as f32,
        ];
        frame_data.oit = [u32::from(translucent), 0, 0, 0];
        if translucent
            && target
                .oit
                .as_ref()
                .is_none_or(|oit| !oit.matches(size, volume_region))
        {
            target.oit = Some(Oit::new(
                device,
                layouts,
                size,
                volume_region,
                *budget / FRAGMENT_BUDGET_DIVISOR,
            ));
        }
        if let Some(oit) = target.oit.as_ref().filter(|_| translucent) {
            frame_data.oit[1] = oit.layers();
            frame_data.oit[3] = oit.tail_scale();
        }
        let signature = Signature {
            scene,
            data: &frame_data,
            section: &section_data,
            camera: camera.state(),
            view: section.state(),
            levels,
            time,
            exposure,
            epoch: *epoch,
            content: *content,
            size,
            volume_region,
            section_region,
        };
        let structure = signature.digest();
        let transition = signature.transition();
        let key = LevelKey::of(levels);
        let moved = target.transition != Some(transition);
        if moved {
            *fade = None;
        }
        if target.levels.is_some_and(|previous| previous != key)
            && !moved
            && !*exporting
            && samples > 1
            && target.samples > 0
        {
            target.fade_group(device, present);
            target.snapshot(&mut encoder);
            *fade = Some(Fade {
                weight: 0.0,
                samples: target.samples,
            });
        }
        target.transition = Some(transition);
        target.levels = Some(key);
        if target.signature != Some(structure) {
            target.signature = Some(structure);
            target.samples = 0;
        }
        let oit_group = match target.oit.as_ref() {
            Some(oit) if translucent => oit.read_group(),
            _ => dummy_oit.read_group(),
        };
        let reset = target.samples == 0;
        info.samples = target.samples;
        if reset {
            meshes.set(device, queue, layouts, scene);
            slice.prepare(
                device,
                queue,
                layouts,
                scene,
                crate::slice::CutView {
                    section: &section_data,
                    size: cut_size,
                    camera,
                },
            );
            markers.prepare(device, queue, layouts, scene.markers);
            meshes.cull_volume(&mut encoder, scene_table.group());
            markers.cull_volume(&mut encoder, scene_table.group());
            markers.cull_section(&mut encoder, scene_table.group());
            if scene.shading.shadows > 0.0 {
                meshes.cull_shadow(&mut encoder, scene_table.group());
            }
        }
        let sample_index = target.samples.min(samples - 1);
        let offset = sample::offset(sample_index, [volume_width, volume_height]);
        frame_data.jitter = [
            offset.x,
            offset.y,
            sample_index as f32,
            scene.step_scale.max(1.0e-3),
        ];
        let cut_offset = sample::offset(sample_index, cut_size);
        section_data.jitter = [cut_offset.x, cut_offset.y, 0.0, 0.0];
        let range = section_range(pyramid, section_level, section, section_aspect, region);
        scene_table.write(
            device,
            queue,
            pyramid,
            scene.channels,
            frame_data,
            section_data,
        );
        let scene_group = scene_table.group();
        if replan {
            let scale = bucket_scale(
                camera,
                tables[0].origin.to_array(),
                tables[0].world.to_array(),
            );
            for band in 0..BAND {
                let plan = &mut planner.bands[band];
                if band < ladder.count {
                    let table = tables[band];
                    plan.prepare(
                        queue,
                        Schedule {
                            time,
                            band: band as u32,
                            capacity: capacity[band] as u32,
                            range: SectionRange::default(),
                            dispatch: volume_dispatch(table),
                            bucket_scale: scale,
                            base: table.record_base(),
                        },
                    );
                } else {
                    plan.prepare(
                        queue,
                        Schedule {
                            time,
                            band: band as u32,
                            capacity: 0,
                            range: SectionRange::default(),
                            dispatch: 0,
                            bucket_scale: scale,
                            base: 0,
                        },
                    );
                }
            }
            planner.section.prepare(
                queue,
                Schedule {
                    time,
                    band: 0,
                    capacity: section_capacity as u32,
                    range,
                    dispatch: range.dispatch(),
                    bucket_scale: 0.0,
                    base: section_table.record_base(),
                },
            );
            planner.encode(&mut encoder, scene_group);
            *planned = Some(residency);
        }
        if target.samples < samples {
            target.samples += 1;
            info.samples = target.samples;
            meshes.write_uniforms(queue, &frame_data, scene.shading);
            let styles = scene_styles(scene);
            if let Some(shadow) = shadow.as_mut() {
                let groups = ShadowGroups {
                    scene: scene_group,
                    level: level_group,
                    atlas: atlas.group(),
                };
                shadow.encode_occlusion(
                    &mut encoder,
                    groups,
                    digest(&[*content, styles, ladder.base as u64, ladder.count as u64]),
                );
                if scene.shading.shadows > 0.0 {
                    let object = digest(&[shadow_geometry(scene), space.digest()]);
                    if *object_key != Some(object) {
                        *object_key = Some(object);
                        meshes.draw_shadow(&mut encoder, shadow.objects());
                        markers.draw_shadow(&mut encoder, shadow.objects(), scene_group);
                    }
                    shadow.encode_tau(
                        &mut encoder,
                        groups,
                        digest(&[
                            *content,
                            styles,
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
            meshes.draw_volume(
                &mut encoder,
                target.scene.color(),
                target.scene.depth(),
                volume_region,
                shadow_group,
            );
            markers.draw_volume(
                &mut encoder,
                target.scene.color(),
                target.scene.depth(),
                volume_region,
                scene_group,
            );
            if let Some(oit) = target.oit.as_ref().filter(|_| translucent) {
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
                        volume_region[0] as f32,
                        volume_region[1] as f32,
                        volume_width as f32,
                        volume_height as f32,
                        0.0,
                        1.0,
                    );
                    pass.set_scissor_rect(
                        volume_region[0],
                        volume_region[1],
                        volume_width,
                        volume_height,
                    );
                    meshes.draw_oit(&mut pass, shadow_group, oit.write_group());
                    markers.draw_oit(&mut pass, scene_group, oit.write_group());
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
                        volume_region[0] as f32,
                        volume_region[1] as f32,
                        volume_width as f32,
                        volume_height as f32,
                        0.0,
                        1.0,
                    );
                    pass.set_scissor_rect(
                        volume_region[0],
                        volume_region[1],
                        volume_width,
                        volume_height,
                    );
                    meshes.draw_color(
                        &mut pass,
                        shadow_group,
                        oit.write_group(),
                        oit.coverage_group(),
                    );
                    markers.draw_color(
                        &mut pass,
                        scene_group,
                        oit.write_group(),
                        oit.coverage_group(),
                    );
                }
                {
                    let tail = oit.tail_size();
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
                    );
                    markers.draw_tail(
                        &mut pass,
                        scene_group,
                        oit.write_group(),
                        oit.coverage_group(),
                    );
                }
                if let Some(slot) = oit_readback.reserve() {
                    oit_readback.stage(&mut encoder, slot, 0, oit.dropped(), 4);
                }
            }
            volume.draw(
                &mut encoder,
                crate::volume::VolumeFrame {
                    target: target.scene.color(),
                    region: volume_region,
                    scene: scene_group,
                    level: level_group,
                    atlas: atlas.group(),
                    depth_group: target.scene.depth_group(),
                    shadow: shadow_group,
                    oit: oit_group,
                },
            );
            section_pass.draw(
                &mut encoder,
                crate::section::SectionFrame {
                    target: target.scene.color(),
                    region: section_region,
                    scene: scene_group,
                    level: level_group,
                    atlas: atlas.group(),
                },
            );
            cut.draw(device, &mut encoder, cut_size, |pass| {
                markers.draw_section_behind(pass, scene_group);
                slice.draw(pass, scene_group);
                markers.draw_section_front(pass, scene_group);
            });
            if slice.objects() > 0 {
                cut.outline(&mut encoder, cut_size, slice.style_group());
            }
            cut.composite(&mut encoder, target.scene.color(), section_region);
            info.cut.objects = slice.objects();
            info.cut.draws = slice.draws() + 1 + u32::from(slice.objects() > 0);
            info.cut.targets = cut.targets();
            info.cut.indices = slice.index_bytes();
            present.accumulate(
                &mut encoder,
                &target.accumulation_view,
                &target.accumulate_group,
                reset,
            );
        }
        if let Some(active) = fade.as_mut() {
            active.weight = (active.weight + 1.0 / FADE_DRAWS).min(1.0);
        }
        let blend = fade.as_ref().map(|fade| (fade.weight, fade.samples));
        present.write(queue, exposure, target.samples, *transparent, blend);
        if fade.is_some() {
            let group = target.fade_group(device, present);
            present.present(&mut encoder, surface, group);
            if fade.as_ref().is_some_and(|active| active.weight >= 1.0) {
                *fade = None;
            }
        } else {
            present.present(&mut encoder, surface, &target.present_group);
        }
        info.fading = fade.is_some();
        if replan
            && let Some(feedback) = feedback.as_mut()
            && let Some(slot) = feedback.reserve()
        {
            for band in 0..BAND {
                feedback.stage(
                    &mut encoder,
                    slot,
                    band as u64 * *view_bytes,
                    planner.bands[band].buffer(),
                    *view_bytes,
                );
            }
            feedback.stage(
                &mut encoder,
                slot,
                BAND as u64 * *view_bytes,
                planner.section.buffer(),
                *view_bytes,
            );
            frames[slot] = Some(residency);
        }
        queue.submit([encoder.finish()]);
        if let Some(feedback) = feedback.as_mut() {
            feedback.register();
        }
        oit_readback.register();
        info.volume.level = ladder.base;
        info.section.level = section_level;
        info.bytes = atlas.bytes();
        info.geometry = meshes.bytes();
        info.fragments = target.oit.as_ref().map_or(0, Oit::bytes);
        info.oit.layers = target.oit.as_ref().map_or(0, Oit::layers);
        info.oit.dropped = *dropped;
        *info
    }
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
