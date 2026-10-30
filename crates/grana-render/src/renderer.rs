use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use grana_volume::{BRICK_SIDE, BrickCoord, BrickData, BrickGrid, PADDED_SIDE, Pyramid};
use half::f16;
use std::num::NonZeroU32;
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Buffer,
    BufferBindingType, BufferUsages, Color, ColorTargetState, ColorWrites, CommandEncoder, Device,
    Extent3d, FragmentState, LoadOp, MultisampleState, Operations, Origin3d,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
    SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
    TextureViewDescriptor, TextureViewDimension, VertexState,
};

use crate::atlas::Atlas;
use crate::level::{BrickRecord, LevelCache};
use crate::{Camera, ChannelParams, MAX_CHANNEL, RenderParams, TransferFunction};

const LUT_SIZE: u32 = 1024;
const MAX_STEPS: f32 = 2048.0;
const DUMMY_VOXEL: [u16; 1] = [0];
const SUPERCELL: u32 = 4;

const _: () = assert!(
    grana_volume::BRICK_SIDE == 32 && grana_volume::PADDED_SIDE == 34,
    "the volume shader bakes the brick dimensions"
);
const _: () = {
    assert!(size_of::<Uniforms>() == 304);
    assert!(std::mem::offset_of!(Uniforms, channel_opacity) == 208);
    assert!(std::mem::offset_of!(Uniforms, channel_inverse) == 272);
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    inv_view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    box_min: [f32; 4],
    box_size: [f32; 4],
    shading: [f32; 4],
    march: [f32; 4],
    light: [f32; 4],
    grid: [u32; 4],
    atlas: [u32; 4],
    dims: [f32; 4],
    channel_opacity: [[f32; 4]; 2],
    channel_window: [[f32; 4]; 2],
    channel_inverse: [[f32; 4]; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrickRequest {
    pub level: usize,
    pub coord: BrickCoord,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub level: usize,
    pub visible: usize,
    pub resident: usize,
    pub slots: usize,
    pub bytes: u64,
}

pub struct Frame<'a> {
    pub encoder: &'a mut CommandEncoder,
    pub target: &'a TextureView,
    pub region: [u32; 4],
    pub camera: &'a Camera,
    pub params: &'a RenderParams,
    pub requests: &'a mut Vec<BrickRequest>,
}

struct Backend {
    pipeline: RenderPipeline,
    frame_layout: BindGroupLayout,
    channel_layout: BindGroupLayout,
    uniforms: Buffer,
    lut: Texture,
    lut_view: TextureView,
    lut_sampler: Sampler,
    volume_sampler: Sampler,
    dummy_view: TextureView,
}

impl Backend {
    fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.volume.shader"),
            source: ShaderSource::Wgsl(include_str!("volume.wgsl").into()),
        });
        let frame_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.frame.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let channel_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.channel.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: NonZeroU32::new(MAX_CHANNEL as u32),
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.volume.pipeline_layout"),
            bind_group_layouts: &[Some(&frame_layout), Some(&channel_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.volume.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.volume.uniforms"),
            size: size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let volume_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.atlas.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let lut = device.create_texture(&TextureDescriptor {
            label: Some("grana.volume.lut"),
            size: Extent3d {
                width: LUT_SIZE,
                height: MAX_CHANNEL as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lut_view = lut.create_view(&TextureViewDescriptor {
            label: Some("grana.volume.lut.view"),
            ..Default::default()
        });
        let lut_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.volume.lut.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy = device.create_texture(&TextureDescriptor {
            label: Some("grana.atlas.dummy"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: TextureFormat::R16Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &dummy,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(&DUMMY_VOXEL),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(2),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = dummy.create_view(&TextureViewDescriptor {
            label: Some("grana.atlas.dummy.view"),
            ..Default::default()
        });
        Self {
            pipeline,
            frame_layout,
            channel_layout,
            uniforms,
            lut,
            lut_view,
            lut_sampler,
            volume_sampler,
            dummy_view,
        }
    }

    fn write_transfer(&self, queue: &Queue, index: usize, transfer: &TransferFunction) {
        let lut = transfer.lut(LUT_SIZE as usize);
        let mut quantized = Vec::with_capacity(lut.len() * 4);
        for [red, green, blue, opacity] in lut {
            for channel in [red, green, blue, opacity] {
                quantized.push(f16::from_f32(channel).to_bits());
            }
        }
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.lut,
                mip_level: 0,
                origin: Origin3d {
                    x: 0,
                    y: index as u32,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(&quantized),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LUT_SIZE * 8),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: LUT_SIZE,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }
}

struct Scene {
    pyramid: Pyramid,
    atlas: Atlas,
    levels: Vec<Option<LevelCache>>,
    active: Option<usize>,
    channels: usize,
}

pub struct Renderer {
    backend: Backend,
    scene: Option<Scene>,
    transfers: Vec<Option<TransferFunction>>,
    budget: u64,
    frame: u64,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat, budget: u64) -> Self {
        Self {
            backend: Backend::new(device, queue, format),
            scene: None,
            transfers: vec![None; MAX_CHANNEL],
            budget,
            frame: 0,
        }
    }

    pub fn set_pyramid(&mut self, device: &Device, pyramid: &Pyramid) {
        let channels = pyramid.channel_count();
        assert!(
            (1..=MAX_CHANNEL).contains(&channels),
            "a volume holds between one and {MAX_CHANNEL} channels"
        );
        assert!(
            !pyramid.levels().is_empty(),
            "a pyramid holds at least one level"
        );
        let finest = BrickGrid::new(pyramid.level(0).extent()).count() as u64;
        let slot_bytes = (PADDED_SIDE as u64).pow(3) * size_of::<u16>() as u64 * channels as u64;
        let requested = (self.budget / slot_bytes).max(1).min(finest);
        let atlas = Atlas::new(
            device,
            &self.backend.channel_layout,
            &self.backend.dummy_view,
            &self.backend.volume_sampler,
            channels,
            requested,
            device.limits().max_texture_dimension_3d,
        );
        self.scene = Some(Scene {
            pyramid: pyramid.clone(),
            atlas,
            levels: (0..pyramid.level_count()).map(|_| None).collect(),
            active: None,
            channels,
        });
        self.transfers.fill(None);
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
        let Renderer {
            backend,
            scene: scene_slot,
            ..
        } = self;
        let Some(scene) = scene_slot.as_mut() else {
            return false;
        };
        let channel_count = scene.channels;
        let Scene {
            pyramid,
            atlas,
            levels,
            ..
        } = scene;
        if level >= pyramid.level_count() || data.channels().len() != channel_count {
            return false;
        }
        let extent = pyramid.level(level).extent();
        if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
            || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
            || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
        {
            return false;
        }
        if levels[level].is_none() {
            levels[level] = Some(LevelCache::new(
                device,
                &backend.frame_layout,
                &backend.uniforms,
                &backend.lut_view,
                &backend.lut_sampler,
                pyramid.level(level),
            ));
        }
        let index = levels[level]
            .as_ref()
            .expect("the level cache has just been created")
            .grid
            .index(coord);
        if levels[level]
            .as_ref()
            .expect("the level cache exists")
            .slot(index)
            != 0
        {
            return true;
        }
        let Some((slot, evicted)) = atlas.allocate(frame) else {
            return false;
        };
        if let Some((old_level, old_index)) = evicted
            && let Some(old_cache) = levels[old_level].as_mut()
        {
            old_cache.set_record(queue, old_index, BrickRecord::new(0, [[0.0; 4]; 2]));
        }
        let mut maxima = [[0.0f32; 4]; 2];
        for channel in 0..channel_count {
            let intensity = pyramid.channels()[channel].intensity();
            let source = &data.channels()[channel];
            let mut units = vec![0u16; source.voxel_count()];
            source.write_units(intensity, &mut units);
            atlas.write(queue, channel, slot, &units);
            maxima[channel / 4][channel % 4] = source.unit_max(intensity);
        }
        levels[level]
            .as_mut()
            .expect("the level cache exists")
            .set_record(queue, index, BrickRecord::new(slot + 1, maxima));
        atlas.assign(slot, level, index, frame);
        true
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, frame: Frame<'_>) -> FrameInfo {
        let Frame {
            encoder,
            target,
            region,
            camera,
            params,
            requests,
        } = frame;
        self.frame += 1;
        let frame = self.frame;
        let Renderer {
            backend,
            scene: scene_slot,
            transfers,
            ..
        } = self;
        let Some(scene) = scene_slot.as_mut() else {
            clear_target(encoder, target);
            return FrameInfo::default();
        };
        assert_eq!(
            params.channels.len(),
            scene.channels,
            "every rendered channel needs parameters"
        );
        write_transfers(queue, backend, transfers, &params.channels);

        let width = region[2].max(1);
        let height = region[3].max(1);
        let aspect = width as f32 / height as f32;
        let Scene {
            pyramid,
            atlas,
            levels,
            active,
            ..
        } = scene;

        let mut target_level = select_level(pyramid, *active, camera, height as f32);
        let target_visible = loop {
            if let Some(visible) =
                visible_bricks_capped(pyramid, target_level, camera, aspect, atlas.slots())
            {
                break visible;
            }
            if target_level + 1 < pyramid.level_count() {
                target_level += 1;
            } else {
                let mut visible = visible_bricks(pyramid, target_level, camera, aspect);
                visible.truncate(atlas.slots());
                break visible;
            }
        };

        let mut render_level = (*active).unwrap_or(target_level);
        if active.is_none() {
            *active = Some(render_level);
        }
        let mut switch = render_level == target_level;
        let mut active_visible = Vec::new();
        if render_level != target_level {
            match visible_bricks_capped(pyramid, render_level, camera, aspect, atlas.slots()) {
                Some(visible) => {
                    let fits = visible.len() + target_visible.len() <= atlas.slots();
                    let ready = !target_visible.is_empty()
                        && count_missing(levels, target_level, &target_visible) == 0;
                    if !fits || ready {
                        switch = true;
                    } else {
                        active_visible = visible;
                    }
                }
                None => switch = true,
            }
        }
        let (visible_count, resident_count) = if switch {
            *active = Some(target_level);
            render_level = target_level;
            collect(
                levels,
                atlas,
                render_level,
                &target_visible,
                frame,
                requests,
            )
        } else {
            collect(
                levels,
                atlas,
                target_level,
                &target_visible,
                frame,
                requests,
            );
            collect(
                levels,
                atlas,
                render_level,
                &active_visible,
                frame,
                requests,
            )
        };

        let level = pyramid.level(render_level);
        if levels[render_level].is_none() {
            levels[render_level] = Some(LevelCache::new(
                device,
                &backend.frame_layout,
                &backend.uniforms,
                &backend.lut_view,
                &backend.lut_sampler,
                level,
            ));
        }
        let cache = levels[render_level]
            .as_ref()
            .expect("the level cache exists");
        let uniforms = uniforms(pyramid, cache, atlas, camera, params, aspect, height);
        queue.write_buffer(&backend.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.volume.pass"),
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
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], width, height);
        pass.set_pipeline(&backend.pipeline);
        pass.set_bind_group(0, cache.group(), &[]);
        pass.set_bind_group(1, atlas.group(), &[]);
        pass.draw(0..3, 0..1);

        FrameInfo {
            level: render_level,
            visible: visible_count,
            resident: resident_count,
            slots: atlas.slots(),
            bytes: atlas.bytes(),
        }
    }
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

fn write_transfers(
    queue: &Queue,
    backend: &Backend,
    transfers: &mut [Option<TransferFunction>],
    channels: &[ChannelParams],
) {
    for (index, channel) in channels.iter().enumerate() {
        if transfers[index].as_ref() == Some(&channel.transfer) {
            continue;
        }
        backend.write_transfer(queue, index, &channel.transfer);
        transfers[index] = Some(channel.transfer.clone());
    }
}

fn select_level(pyramid: &Pyramid, active: Option<usize>, camera: &Camera, height: f32) -> usize {
    let pixel_world = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        let voxel = minimum_spacing(pyramid, index);
        if voxel <= pixel_world {
            level = index;
        } else {
            break;
        }
    }
    if let Some(active) = active {
        let voxel = minimum_spacing(pyramid, active);
        if level < active && pixel_world >= voxel * 0.7 {
            level = active;
        }
        if level > active && pixel_world <= voxel * 1.4 {
            level = active;
        }
    }
    level
}

fn minimum_spacing(pyramid: &Pyramid, level: usize) -> f32 {
    let spacing = pyramid.level(level).spacing();
    spacing.x().min(spacing.y()).min(spacing.z())
}

fn collect(
    levels: &mut [Option<LevelCache>],
    atlas: &mut Atlas,
    level: usize,
    visible: &[(BrickCoord, f32)],
    frame: u64,
    requests: &mut Vec<BrickRequest>,
) -> (usize, usize) {
    let mut resident = 0;
    for (coord, _) in visible {
        let slot = levels[level]
            .as_ref()
            .map_or(0, |cache| cache.slot(cache.grid.index(*coord)));
        if slot == 0 {
            requests.push(BrickRequest {
                level,
                coord: *coord,
            });
        } else {
            resident += 1;
            atlas.mark(slot - 1, frame);
        }
    }
    (visible.len(), resident)
}

fn count_missing(
    levels: &[Option<LevelCache>],
    level: usize,
    visible: &[(BrickCoord, f32)],
) -> usize {
    let Some(cache) = &levels[level] else {
        return visible.len();
    };
    visible
        .iter()
        .filter(|(coord, _)| cache.slot(cache.grid.index(*coord)) == 0)
        .count()
}

fn visible_bricks(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &Camera,
    aspect: f32,
) -> Vec<(BrickCoord, f32)> {
    collect_visible(pyramid, level_index, camera, aspect, None)
        .expect("an unbounded visibility query always completes")
}

fn visible_bricks_capped(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &Camera,
    aspect: f32,
    limit: usize,
) -> Option<Vec<(BrickCoord, f32)>> {
    collect_visible(pyramid, level_index, camera, aspect, Some(limit))
}

fn collect_visible(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &Camera,
    aspect: f32,
    limit: Option<usize>,
) -> Option<Vec<(BrickCoord, f32)>> {
    let level = pyramid.level(level_index);
    let extent = level.extent();
    let grid = BrickGrid::new(extent);
    let world = Vec3::from(level.world_extent());
    let voxel = world / Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
    let box_min = -0.5 * world;
    let planes = frustum_planes(camera.view_projection(aspect, 0.5 * world.length()));
    let eye = camera.eye();
    let counts = grid.counts();
    let supercells = counts.map(|count| count.div_ceil(SUPERCELL));
    let mut visible = Vec::new();
    for z in 0..supercells[2] {
        for y in 0..supercells[1] {
            for x in 0..supercells[0] {
                let origin = [
                    x * SUPERCELL * BRICK_SIDE,
                    y * SUPERCELL * BRICK_SIDE,
                    z * SUPERCELL * BRICK_SIDE,
                ];
                let size = [
                    (extent.x() - origin[0]).min(SUPERCELL * BRICK_SIDE),
                    (extent.y() - origin[1]).min(SUPERCELL * BRICK_SIDE),
                    (extent.z() - origin[2]).min(SUPERCELL * BRICK_SIDE),
                ];
                let minimum = box_min + Vec3::from(origin.map(|value| value as f32)) * voxel;
                let maximum = minimum + Vec3::from(size.map(|value| value as f32)) * voxel;
                if outside(&planes, minimum, maximum) {
                    continue;
                }
                let bricks = [
                    size[0].div_ceil(BRICK_SIDE),
                    size[1].div_ceil(BRICK_SIDE),
                    size[2].div_ceil(BRICK_SIDE),
                ];
                for brick_z in 0..bricks[2] {
                    for brick_y in 0..bricks[1] {
                        for brick_x in 0..bricks[0] {
                            let coord = BrickCoord::new(
                                x * SUPERCELL + brick_x,
                                y * SUPERCELL + brick_y,
                                z * SUPERCELL + brick_z,
                            );
                            let brick_origin = grid.origin(coord);
                            let brick_size = grid.extent_of(coord);
                            let minimum = box_min
                                + Vec3::from(brick_origin.map(|value| value as f32)) * voxel;
                            let maximum = minimum
                                + Vec3::new(
                                    brick_size.x() as f32,
                                    brick_size.y() as f32,
                                    brick_size.z() as f32,
                                ) * voxel;
                            if outside(&planes, minimum, maximum) {
                                continue;
                            }
                            let center = 0.5 * (minimum + maximum);
                            visible.push((coord, (center - eye).length_squared()));
                            if let Some(limit) = limit
                                && visible.len() > limit
                            {
                                return None;
                            }
                        }
                    }
                }
            }
        }
    }
    visible.sort_by(|left, right| left.1.total_cmp(&right.1));
    Some(visible)
}

fn frustum_planes(view_projection: Mat4) -> [Vec4; 6] {
    let row0 = view_projection.row(0);
    let row1 = view_projection.row(1);
    let row2 = view_projection.row(2);
    let row3 = view_projection.row(3);
    [
        row3 + row0,
        row3 - row0,
        row3 + row1,
        row3 - row1,
        row2,
        row3 - row2,
    ]
    .map(|plane| plane / plane.truncate().length())
}

fn outside(planes: &[Vec4; 6], minimum: Vec3, maximum: Vec3) -> bool {
    planes.iter().any(|plane| {
        let positive = Vec3::new(
            if plane.x >= 0.0 { maximum.x } else { minimum.x },
            if plane.y >= 0.0 { maximum.y } else { minimum.y },
            if plane.z >= 0.0 { maximum.z } else { minimum.z },
        );
        plane.x * positive.x + plane.y * positive.y + plane.z * positive.z + plane.w < 0.0
    })
}

fn uniforms(
    pyramid: &Pyramid,
    cache: &LevelCache,
    atlas: &Atlas,
    camera: &Camera,
    params: &RenderParams,
    aspect: f32,
    height: u32,
) -> Uniforms {
    let view_projection = camera.view_projection(aspect, 0.5 * cache.world.length());
    let eye = camera.eye();
    let light = camera.light_direction();
    let step = step_length(camera, cache, params.step_scale, height as f32);
    let mut channel_opacity = [[0.0f32; 4]; 2];
    let mut channel_window = [[0.0f32; 4]; 2];
    let mut channel_inverse = [[0.0f32; 4]; 2];
    for (index, channel) in params.channels.iter().enumerate() {
        channel_opacity[index / 4][index % 4] = if channel.visible {
            channel.opacity.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let data = pyramid.channels()[index].intensity();
        let span = data.span();
        if span > 0.0 {
            let start = (channel.window.min() - data.min()) / span;
            let window_span = (channel.window.max() - channel.window.min()) / span;
            channel_window[index / 4][index % 4] = start;
            channel_inverse[index / 4][index % 4] = if window_span > 0.0 {
                1.0 / window_span
            } else {
                0.0
            };
        }
    }
    let grid = cache.grid.counts();
    let atlas_grid = atlas.slot_grid();
    Uniforms {
        inv_view_projection: view_projection.inverse().to_cols_array_2d(),
        eye: [eye.x, eye.y, eye.z, 0.0],
        box_min: [
            -0.5 * cache.world.x,
            -0.5 * cache.world.y,
            -0.5 * cache.world.z,
            0.0,
        ],
        box_size: [cache.world.x, cache.world.y, cache.world.z, 0.0],
        shading: [
            params.shading.ambient,
            params.shading.diffuse,
            params.shading.specular,
            params.shading.shininess,
        ],
        march: [step, 0.0, params.channels.len() as f32, MAX_STEPS],
        light: [light.x, light.y, light.z, 0.0],
        grid: [grid[0], grid[1], grid[2], 0],
        atlas: [atlas_grid[0], atlas_grid[1], atlas_grid[2], 0],
        dims: [cache.dims.x, cache.dims.y, cache.dims.z, 0.0],
        channel_opacity,
        channel_window,
        channel_inverse,
    }
}

fn step_length(camera: &Camera, cache: &LevelCache, step_scale: f32, height: f32) -> f32 {
    let pixel_world = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    let direction = camera.forward() / cache.world;
    let dims = cache.dims;
    let mut voxel_world = f32::INFINITY;
    for axis in 0..3 {
        let rate = direction[axis].abs();
        if rate > 1.0e-6 {
            voxel_world = voxel_world.min((1.0 / dims[axis]) / rate);
        }
    }
    let voxel_world = if voxel_world.is_finite() {
        voxel_world
    } else {
        cache.voxel.min_element()
    };
    (pixel_world * 0.5)
        .max(voxel_world * step_scale.max(1.0e-3))
        .max(1.0e-7)
}
