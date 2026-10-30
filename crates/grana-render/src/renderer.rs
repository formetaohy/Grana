use glam::Vec3;
use grana_object::Mesh;
use grana_volume::{BrickCoord, BrickData, Pyramid};
use wgpu::{
    Color, CommandEncoder, Device, LoadOp, Operations, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp, TextureView,
};

use crate::backend::Backend;
use crate::cache::Cache;
use crate::level::{LevelCache, Resources};
use crate::marker::{SectionFrame, VolumeFrame};
use crate::mesh::SurfaceFrame;
use crate::residency::{self, Residency};
use crate::transfer::TransferFunction;
use crate::uniforms;
use crate::{Camera, Marker, RenderParams, SectionCamera};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrickRequest {
    pub level: usize,
    pub coord: BrickCoord,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ViewInfo {
    pub level: usize,
    pub visible: usize,
    pub resident: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub volume: ViewInfo,
    pub section: ViewInfo,
    pub bytes: u64,
}

pub struct Frame<'a> {
    pub encoder: &'a mut CommandEncoder,
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub params: &'a RenderParams,
    pub markers: &'a [Marker],
    pub surface: Option<SurfaceFrame<'a>>,
    pub volume_requests: &'a mut Vec<BrickRequest>,
    pub section_requests: &'a mut Vec<BrickRequest>,
}

pub struct Renderer {
    backend: Backend,
    cache: Option<Cache>,
    transfers: Vec<Option<TransferFunction>>,
    budget: u64,
    frame: u64,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: wgpu::TextureFormat, budget: u64) -> Self {
        Self {
            backend: Backend::new(device, queue, format),
            cache: None,
            transfers: vec![None; crate::MAX_CHANNEL],
            budget,
            frame: 0,
        }
    }

    pub fn set_pyramid(&mut self, device: &Device, pyramid: &Pyramid) {
        self.cache = Some(self.backend.cache(device, pyramid, self.budget));
        self.transfers.fill(None);
    }

    pub fn set_surface(&mut self, device: &Device, mesh: Option<&Mesh>) {
        self.backend.surfaces.set(device, mesh);
    }

    pub fn upload(
        &mut self,
        device: &Device,
        queue: &Queue,
        level: usize,
        coord: BrickCoord,
        data: &BrickData,
    ) -> bool {
        let frame = self.frame;
        let resources = self.backend.resources();
        let Some(cache) = self.cache.as_mut() else {
            return false;
        };
        cache.upload(
            device,
            queue,
            &resources,
            frame,
            BrickRequest { level, coord },
            data,
        )
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, frame: Frame<'_>) -> FrameInfo {
        let Frame {
            encoder,
            target,
            size,
            volume_region,
            section_region,
            camera,
            section,
            params,
            markers,
            surface,
            volume_requests,
            section_requests,
        } = frame;
        self.frame += 1;
        let frame_index = self.frame;
        let Renderer {
            backend,
            cache: cache_slot,
            transfers,
            ..
        } = self;
        let Some(cache) = cache_slot.as_mut() else {
            clear_target(encoder, target);
            return FrameInfo::default();
        };
        assert_eq!(
            params.channels.len(),
            cache.channels,
            "every rendered channel needs parameters"
        );
        assert!(size[0] > 0 && size[1] > 0, "the surface is not empty");
        assert!(
            inside(volume_region, size) && inside(section_region, size),
            "the panes lie on the surface"
        );
        backend.write_transfers(queue, transfers, &params.channels);
        backend.ensure_target(device, size);
        backend.screen.write(queue);
        backend.markers.prepare(device, queue, markers);
        if let Some(surface) = &surface {
            backend.surfaces.write_styles(queue, surface.styles);
        }
        backend
            .screen
            .background(encoder, backend.scene(), volume_region);

        let slots = cache.atlas.slots();
        let section_width = section_region[2].max(1);
        let section_height = section_region[3].max(1);
        let section_aspect = section_width as f32 / section_height as f32;
        let section_target = residency::select_section_level(
            &cache.pyramid,
            cache.section_level,
            section,
            section_height as f32,
        );
        let (section_render, section_visible, section_resident) = Residency {
            pyramid: &cache.pyramid,
            levels: &mut cache.levels,
            atlas: &mut cache.atlas,
            capacity: slots,
            frame: frame_index,
        }
        .stabilize(
            &mut cache.section_level,
            section_target,
            section_requests,
            |level, limit| {
                residency::visible_section(
                    &cache.pyramid,
                    level,
                    camera,
                    section,
                    section_aspect,
                    limit,
                )
            },
        );

        let volume_width = volume_region[2].max(1);
        let volume_height = volume_region[3].max(1);
        let volume_aspect = volume_width as f32 / volume_height as f32;
        let volume_target = residency::select_volume_level(
            &cache.pyramid,
            cache.volume_level,
            camera,
            volume_height as f32,
        );
        let volume_capacity = if section_render == volume_target {
            slots
        } else {
            slots.saturating_sub(section_visible).max(1)
        };
        let (volume_render, volume_visible, volume_resident) = Residency {
            pyramid: &cache.pyramid,
            levels: &mut cache.levels,
            atlas: &mut cache.atlas,
            capacity: volume_capacity,
            frame: frame_index,
        }
        .stabilize(
            &mut cache.volume_level,
            volume_target,
            volume_requests,
            |level, limit| {
                residency::collect_visible(&cache.pyramid, level, camera, volume_aspect, limit)
            },
        );

        let scene_radius =
            0.5 * Vec3::from(cache.pyramid.level(volume_render).world_extent()).length();
        backend.clear_depth(encoder);
        if surface.is_some() {
            backend.surfaces.draw_volume(
                encoder,
                queue,
                crate::mesh::VolumeFrame {
                    target: backend.scene(),
                    depth: backend.depth(),
                    region: volume_region,
                    camera,
                    shading: params.shading,
                    scene_radius,
                },
            );
        }
        backend.markers.draw_volume(
            encoder,
            queue,
            VolumeFrame {
                target: backend.scene(),
                depth: backend.depth(),
                region: volume_region,
                camera,
                shading: params.shading,
                scene_radius,
            },
        );

        let resources = backend.resources();
        let volume_cache = ensure_level(
            device,
            &resources,
            &mut cache.levels,
            &cache.pyramid,
            volume_render,
        );
        let uniforms = uniforms::volume_uniforms(
            &cache.pyramid,
            volume_cache,
            &cache.atlas,
            camera,
            params,
            volume_aspect,
            volume_height,
        );
        queue.write_buffer(&backend.uniforms, 0, bytemuck::bytes_of(&uniforms));
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.volume.pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: backend.scene(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
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
            pass.set_pipeline(&backend.volume_pipeline);
            pass.set_bind_group(0, volume_cache.group(), &[]);
            pass.set_bind_group(1, cache.atlas.group(), &[]);
            pass.set_bind_group(2, backend.depth_group(), &[]);
            pass.draw(0..3, 0..1);
        }

        let section_cache = ensure_level(
            device,
            &resources,
            &mut cache.levels,
            &cache.pyramid,
            section_render,
        );
        let uniforms = uniforms::section_uniforms(
            &cache.pyramid,
            section_cache,
            &cache.atlas,
            camera,
            section,
            params,
            section_aspect,
        );
        queue.write_buffer(&backend.section_uniforms, 0, bytemuck::bytes_of(&uniforms));
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("grana.section.pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: backend.scene(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_viewport(
                section_region[0] as f32,
                section_region[1] as f32,
                section_width as f32,
                section_height as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(
                section_region[0],
                section_region[1],
                section_width,
                section_height,
            );
            pass.set_pipeline(&backend.section_pipeline);
            pass.set_bind_group(0, section_cache.section_group(), &[]);
            pass.set_bind_group(1, cache.atlas.group(), &[]);
            pass.draw(0..3, 0..1);
        }

        if surface.is_some() {
            backend.surfaces.draw_section(
                encoder,
                backend.scene(),
                backend.depth(),
                section_region,
                section_cache.section_group(),
            );
        }

        backend.markers.draw_section(
            encoder,
            SectionFrame {
                target: backend.scene(),
                region: section_region,
                uniforms: section_cache.section_group(),
            },
        );

        backend.screen.present(encoder, target, backend.present());

        FrameInfo {
            volume: ViewInfo {
                level: volume_render,
                visible: volume_visible,
                resident: volume_resident,
            },
            section: ViewInfo {
                level: section_render,
                visible: section_visible,
                resident: section_resident,
            },
            bytes: cache.atlas.bytes(),
        }
    }
}

fn inside(region: [u32; 4], size: [u32; 2]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0] + region[2] <= size[0]
        && region[1] + region[3] <= size[1]
}

fn clear_target(encoder: &mut CommandEncoder, target: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.clear"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

fn ensure_level<'a>(
    device: &Device,
    resources: &Resources<'_>,
    levels: &'a mut [Option<LevelCache>],
    pyramid: &Pyramid,
    level: usize,
) -> &'a LevelCache {
    if levels[level].is_none() {
        levels[level] = Some(LevelCache::new(device, resources, pyramid.level(level)));
    }
    levels[level].as_ref().expect("the level cache exists")
}
