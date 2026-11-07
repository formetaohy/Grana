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
            summary,
            bricks,
            planner,
            census,
            volume,
            section: section_pass,
            cut,
            slice,
            meshes,
            markers,
            pyramid,
            channels,
            feedback,
            frames,
            counts,
            view_bytes,
            epoch,
            content,
            settled,
            planned,
            shadow,
            transparent,
            object_key,
            info,
            dummy_oit,
            budget,
            oit_readback,
            dropped,
            ..
        } = self;
        let (
            Some(pyramid),
            Some(atlas),
            Some(scene_table),
            Some(planner),
            Some(census),
            Some(summary),
        ) = (
            pyramid.as_ref(),
            atlas.as_mut(),
            scene_table.as_mut(),
            planner.as_mut(),
            census.as_ref(),
            summary.as_ref(),
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
        let section_target_level = select_section_level(pyramid, section, section_region[3] as f32);
        let (section_level, section_count) = resolve_section_level(
            pyramid,
            camera,
            section,
            section_aspect,
            section_target_level,
            (slots / SECTION_BUDGET_DIVISOR).max(1),
            region,
        );
        let volume_target_level = select_volume_level(pyramid, camera, volume_region[3] as f32);
        let section_capacity = section_count.clamp(1, slots.saturating_sub(1).max(1));
        let volume_capacity = slots.saturating_sub(section_capacity).max(1);
        let volume_level =
            resolve_volume_level(volume_target_level, level_count, volume_capacity, counts);
        let levels = Levels {
            volume: volume_level,
            section: section_level,
            volume_capacity,
            section_capacity,
        };
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
        ensure_brick(
            device,
            &layouts.level,
            bricks,
            pyramid.level(section_level),
            section_level,
            slot_grid,
            summary,
        );
        ensure_brick(
            device,
            &layouts.level,
            bricks,
            pyramid.level(volume_level),
            volume_level,
            slot_grid,
            summary,
        );
        if bricks[section_level]
            .as_ref()
            .expect("the section level table exists")
            .time()
            != Some(time)
        {
            bricks[section_level]
                .as_mut()
                .expect("the section level table exists")
                .sync(queue, time, atlas);
        }
        if bricks[volume_level]
            .as_ref()
            .expect("the volume level table exists")
            .time()
            != Some(time)
        {
            bricks[volume_level]
                .as_mut()
                .expect("the volume level table exists")
                .sync(queue, time, atlas);
        }
        let section_table = bricks[section_level]
            .as_ref()
            .expect("the section level table exists");
        let volume_table = bricks[volume_level]
            .as_ref()
            .expect("the volume level table exists");
        let mut section_data =
            SectionData::new(camera, section, section_aspect, section_table, *channels);
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
            },
            volume_table,
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
        let oit_group = match target.oit.as_ref() {
            Some(oit) if translucent => oit.read_group(),
            _ => dummy_oit.read_group(),
        };
        let signature = Signature {
            scene,
            data: &frame_data,
            section: &section_data,
            levels,
            time,
            exposure,
            epoch: *epoch,
            content: *content,
            size,
            volume_region,
            section_region,
        }
        .digest();
        if target.signature != Some(signature) {
            target.signature = Some(signature);
            target.samples = 0;
        }
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
        frame_data.jitter = [offset.x, offset.y, sample_index as f32, 0.0];
        let cut_offset = sample::offset(sample_index, cut_size);
        section_data.jitter = [cut_offset.x, cut_offset.y, 0.0, 0.0];
        let range = section_range(
            pyramid,
            section_level,
            camera,
            section,
            section_aspect,
            region,
        );
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
            census.prepare(queue);
            planner.volume.prepare(
                queue,
                Schedule {
                    time,
                    level: volume_level,
                    capacity: volume_capacity as u32,
                    layer: 0,
                    range: SectionRange::default(),
                    dispatch: volume_dispatch(volume_table),
                    bucket_scale: bucket_scale(camera, volume_table.world.to_array()),
                },
            );
            planner.section.prepare(
                queue,
                Schedule {
                    time,
                    level: section_level,
                    capacity: section_capacity as u32,
                    layer: range.layer,
                    range,
                    dispatch: range.dispatch(),
                    bucket_scale: 0.0,
                },
            );
            planner.volume.bind_level(device, layouts, volume_table);
            planner.section.bind_level(device, layouts, section_table);
            census.encode(&mut encoder, scene_group);
            planner.encode(&mut encoder, scene_group, &planner.volume, &planner.section);
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
                    level: volume_table.group(),
                    atlas: atlas.group(),
                };
                shadow.encode_occlusion(
                    &mut encoder,
                    groups,
                    digest(&[*content, styles, volume_level as u64]),
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
                        digest(&[*content, styles, volume_level as u64, space.digest()]),
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
                    level: volume_table.group(),
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
                    level: section_table.group(),
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
                &target.accumulation,
                &target.accumulate_group,
                reset,
            );
        }
        present.write(queue, exposure, target.samples, *transparent);
        present.present(&mut encoder, surface, &target.present_group);
        if replan
            && let Some(feedback) = feedback.as_mut()
            && let Some(slot) = feedback.reserve()
        {
            feedback.stage(&mut encoder, slot, 0, planner.volume.buffer(), *view_bytes);
            feedback.stage(
                &mut encoder,
                slot,
                *view_bytes,
                planner.section.buffer(),
                *view_bytes,
            );
            feedback.stage(
                &mut encoder,
                slot,
                2 * *view_bytes,
                census.counts(),
                CENSUS_BYTES,
            );
            frames[slot] = Some(residency);
        }
        queue.submit([encoder.finish()]);
        if let Some(feedback) = feedback.as_mut() {
            feedback.register();
        }
        oit_readback.register();
        info.volume.level = volume_level;
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
