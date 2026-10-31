use bytemuck::cast_slice;
use grana_volume::{BRICK_SIDE, BrickCoord, BrickData, BrickGrid, Level, PADDED_SIDE, Pyramid};
use wgpu::{
    BindGroup, BindGroupLayout, CommandEncoder, Device, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, Sampler,
    SamplerDescriptor, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::CHANNEL_LIMIT;
use crate::atlas::{Atlas, ResidentBrick};
use crate::brick::{BrickRecord, BrickTable};
use crate::camera::{Camera, SectionCamera};
use crate::frame::{FrameData, SectionData};
use crate::layout::Layouts;
use crate::marker::MarkerPasses;
use crate::mesh::MeshPasses;
use crate::present::Present;
use crate::schedule::{
    Cull, CullPass, Scheduler, SectionRange, section_range, select_section_level,
    select_volume_level, volume_dispatch,
};
use crate::schema::SceneFrame;
use crate::section::SectionPass;
use crate::target::SceneTarget;
use crate::volume::VolumePass;

const DUMMY_VOXEL: [u16; 1] = [0];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BrickRequest {
    pub time: u32,
    pub level: usize,
    pub coord: BrickCoord,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

#[derive(Default)]
pub struct Requests {
    pub volume: Vec<BrickRequest>,
    pub section: Vec<BrickRequest>,
    pub volume_view: ViewInfo,
    pub section_view: ViewInfo,
}

pub struct Frame<'a, 'b> {
    pub encoder: &'a mut CommandEncoder,
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub time: u32,
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub scene: &'a SceneFrame<'b>,
}

pub struct Renderer {
    layouts: Layouts,
    present: Present,
    present_group: Option<BindGroup>,
    target: Option<SceneTarget>,
    scene: Option<crate::table::SceneTable>,
    atlas: Option<Atlas>,
    bricks: Vec<Option<BrickTable>>,
    scheduler: Option<Scheduler>,
    volume: VolumePass,
    section: SectionPass,
    meshes: MeshPasses,
    markers: MarkerPasses,
    dummy: TextureView,
    atlas_sampler: Sampler,
    pyramid: Option<Pyramid>,
    channels: usize,
    tick: u64,
    budget: u64,
    signature: Option<Signature>,
}

#[derive(Clone, Copy, PartialEq)]
struct Signature {
    distance: f32,
    span: f32,
    axis: grana_volume::Axis,
}

impl Signature {
    fn of(camera: &Camera, section: &SectionCamera) -> Self {
        Self {
            distance: camera.distance(),
            span: section.span(),
            axis: section.axis(),
        }
    }

    fn refines(self, previous: Self) -> bool {
        self.axis != previous.axis
            || self.distance < previous.distance * 0.98
            || self.span < previous.span * 0.98
    }
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat, budget: u64) -> Self {
        let layouts = Layouts::new(device);
        let dummy = dummy_texture(device, queue);
        let atlas_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.atlas.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        Self {
            present: Present::new(device, format),
            present_group: None,
            target: None,
            scene: None,
            atlas: None,
            bricks: Vec::new(),
            scheduler: None,
            volume: VolumePass::new(device, &layouts),
            section: SectionPass::new(device, &layouts),
            meshes: MeshPasses::new(device, &layouts),
            markers: MarkerPasses::new(device, &layouts),
            layouts,
            dummy,
            atlas_sampler,
            pyramid: None,
            channels: 0,
            tick: 0,
            budget,
            signature: None,
        }
    }

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        let channels = pyramid.channel_count();
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channels),
            "a dataset holds one to {CHANNEL_LIMIT} channels"
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
            &self.layouts.atlas,
            &self.dummy,
            &self.atlas_sampler,
            channels,
            requested,
            device.limits().max_texture_dimension_3d,
        );
        self.bricks = (0..pyramid.level_count()).map(|_| None).collect();
        self.scheduler = Some(Scheduler::new(device, &self.layouts, pyramid.level_count()));
        self.scene = Some(crate::table::SceneTable::new(
            device,
            &self.layouts,
            channels,
        ));
        self.atlas = Some(atlas);
        self.pyramid = Some(pyramid.clone());
        self.channels = channels;
        self.tick = 0;
    }

    pub fn upload(
        &mut self,
        device: &Device,
        queue: &Queue,
        request: BrickRequest,
        data: &BrickData,
    ) -> bool {
        let Self {
            layouts,
            bricks,
            atlas,
            pyramid,
            channels,
            tick,
            ..
        } = self;
        let (Some(pyramid), Some(atlas)) = (pyramid.as_ref(), atlas.as_mut()) else {
            return false;
        };
        let BrickRequest { time, level, coord } = request;
        if level >= pyramid.level_count()
            || time >= pyramid.frame_count()
            || data.channels().len() != *channels
        {
            return false;
        }
        let extent = pyramid.level(level).extent();
        if coord.x() >= extent.x().div_ceil(BRICK_SIDE)
            || coord.y() >= extent.y().div_ceil(BRICK_SIDE)
            || coord.z() >= extent.z().div_ceil(BRICK_SIDE)
        {
            return false;
        }
        let grid = BrickGrid::new(extent);
        let index = grid.index(coord);
        if atlas.slot(time, level, index).is_some() {
            return true;
        }
        let slot_grid = atlas.slot_grid();
        ensure_brick(
            device,
            &layouts.level,
            bricks,
            pyramid.level(level),
            level,
            slot_grid,
        );
        if bricks[level]
            .as_ref()
            .expect("the level table exists")
            .time()
            != Some(time)
        {
            bricks[level]
                .as_mut()
                .expect("the level table exists")
                .sync(queue, time, atlas);
        }
        let Some((slot, evicted)) = atlas.allocate(*tick) else {
            return false;
        };
        if let Some(brick) = evicted
            && let Some(other) = bricks.get_mut(brick.level).and_then(Option::as_mut)
            && other.time() == Some(brick.time)
        {
            other.set_record(queue, brick.index, BrickRecord::EMPTY);
        }
        let mut maxima = [[0.0f32; 4]; CHANNEL_LIMIT / 4];
        for channel in 0..*channels {
            let intensity = pyramid.channels()[channel].intensity();
            let source = &data.channels()[channel];
            let mut units = vec![0u16; source.voxel_count()];
            source.write_units(intensity, &mut units);
            atlas.write(queue, channel, slot, &units);
            maxima[channel / 4][channel % 4] = source.unit_max(intensity);
        }
        atlas.assign(
            slot,
            ResidentBrick {
                time,
                level,
                index,
                maxima,
            },
            *tick,
        );
        bricks[level]
            .as_mut()
            .expect("the level table exists")
            .set_record(queue, index, BrickRecord::new(slot + 1, maxima));
        true
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, frame: Frame<'_, '_>) -> FrameInfo {
        let Frame {
            encoder,
            target,
            size,
            volume_region,
            section_region,
            time,
            camera,
            section,
            scene,
        } = frame;
        assert!(size[0] > 0 && size[1] > 0, "the surface is not empty");
        assert!(
            inside(volume_region, size) && inside(section_region, size),
            "the panes lie on the surface"
        );
        if self
            .target
            .as_ref()
            .is_none_or(|scene_target| scene_target.size() != size)
        {
            let scene_target = SceneTarget::new(device, &self.layouts.depth, size);
            self.present_group = Some(self.present.bind(device, scene_target.color()));
            self.target = Some(scene_target);
        }
        self.tick += 1;
        let tick = self.tick;
        let signature = Signature::of(camera, section);
        let refines = self
            .signature
            .is_none_or(|previous| signature.refines(previous));
        self.signature = Some(signature);
        let Self {
            layouts,
            present,
            present_group,
            target: scene_target,
            scene: scene_table,
            atlas,
            bricks,
            scheduler,
            volume,
            section: section_pass,
            meshes,
            markers,
            pyramid,
            channels,
            ..
        } = self;
        let (Some(pyramid), Some(atlas), Some(scene_table), Some(scheduler)) = (
            pyramid.as_ref(),
            atlas.as_mut(),
            scene_table.as_mut(),
            scheduler.as_mut(),
        ) else {
            clear(encoder, target);
            return FrameInfo::default();
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
        let scene_target = scene_target.as_ref().expect("the scene target is ready");
        let present_group = present_group.as_ref().expect("the present group is ready");
        let volume_width = volume_region[2].max(1);
        let volume_height = volume_region[3].max(1);
        let section_width = section_region[2].max(1);
        let section_height = section_region[3].max(1);
        let volume_aspect = volume_width as f32 / volume_height as f32;
        let section_aspect = section_width as f32 / section_height as f32;
        if refines {
            scheduler.volume.clear_counts();
            scheduler.section.clear_counts();
        }
        let slots = atlas.slots();
        let section_target_level = select_section_level(pyramid, section, section_height as f32);
        let section_level =
            scheduler
                .section
                .select(section_target_level, pyramid.level_count(), slots);
        let volume_target_level = select_volume_level(pyramid, camera, volume_height as f32);
        let volume_capacity = if section_level == volume_target_level {
            slots
        } else {
            slots
                .saturating_sub(scheduler.section.info().visible)
                .max(1)
        };
        let volume_level =
            scheduler
                .volume
                .select(volume_target_level, pyramid.level_count(), volume_capacity);
        let slot_grid = atlas.slot_grid();
        ensure_brick(
            device,
            &layouts.level,
            bricks,
            pyramid.level(section_level),
            section_level,
            slot_grid,
        );
        ensure_brick(
            device,
            &layouts.level,
            bricks,
            pyramid.level(volume_level),
            volume_level,
            slot_grid,
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
        atlas.mark_view(time, section_level, tick);
        atlas.mark_view(time, volume_level, tick);
        let section_table = bricks[section_level]
            .as_ref()
            .expect("the section level table exists");
        let volume_table = bricks[volume_level]
            .as_ref()
            .expect("the volume level table exists");
        let section_data =
            SectionData::new(camera, section, section_aspect, section_table, *channels);
        let frame_data = FrameData::new(
            camera,
            volume_aspect,
            volume_height,
            volume_table,
            scene.shading,
            scene.step_scale,
            *channels,
        );
        scene_table.write(
            device,
            queue,
            pyramid,
            scene.channels,
            frame_data,
            section_data,
        );
        let range = section_range(pyramid, section_level, camera, section, section_aspect);
        scheduler.section.prepare(
            queue,
            Cull {
                time,
                level: section_level,
                capacity: slots as u32,
                layer: range.layer,
                range,
                dispatch: range.dispatch(),
            },
        );
        scheduler.volume.prepare(
            queue,
            Cull {
                time,
                level: volume_level,
                capacity: volume_capacity as u32,
                layer: 0,
                range: SectionRange::default(),
                dispatch: volume_dispatch(volume_table),
            },
        );
        scheduler.section.bind_level(device, layouts, section_table);
        scheduler.volume.bind_level(device, layouts, volume_table);
        meshes.set(device, queue, layouts, scene);
        meshes.write_uniforms(queue, &frame_data, scene.shading);
        markers.prepare(device, queue, layouts, scene.markers);
        let scene_group = scene_table.group();
        CullPass {
            pipeline: &scheduler.section_cull,
            scene: scene_group,
            level: scheduler
                .section
                .level_group()
                .expect("the section level group exists"),
            requests: scheduler.section.requests_group(),
            count: scheduler.section.dispatch(),
        }
        .encode(encoder);
        CullPass {
            pipeline: &scheduler.volume_cull,
            scene: scene_group,
            level: scheduler
                .volume
                .level_group()
                .expect("the volume level group exists"),
            requests: scheduler.volume.requests_group(),
            count: scheduler.volume.dispatch(),
        }
        .encode(encoder);
        meshes.cull_volume(encoder, scene_group);
        meshes.cull_section(encoder, scene_group);
        markers.cull_volume(encoder, scene_group);
        markers.cull_section(encoder, scene_group);
        present.background(encoder, scene_target.color(), volume_region);
        present.write(queue);
        clear_depth(encoder, scene_target.depth());
        meshes.draw_volume(
            encoder,
            scene_target.color(),
            scene_target.depth(),
            volume_region,
        );
        markers.draw_volume(
            encoder,
            scene_target.color(),
            scene_target.depth(),
            volume_region,
            scene_group,
        );
        volume.draw(
            encoder,
            crate::volume::VolumeFrame {
                target: scene_target.color(),
                region: volume_region,
                scene: scene_group,
                level: volume_table.group(),
                atlas: atlas.group(),
                depth_group: scene_target.depth_group(),
            },
        );
        section_pass.draw(
            encoder,
            crate::section::SectionFrame {
                target: scene_target.color(),
                region: section_region,
                scene: scene_group,
                level: section_table.group(),
                atlas: atlas.group(),
            },
        );
        meshes.draw_section(
            encoder,
            scene_target.color(),
            scene_target.depth(),
            section_region,
            scene_group,
        );
        markers.draw_section(encoder, scene_target.color(), section_region, scene_group);
        present.present(encoder, target, present_group);
        FrameInfo {
            volume: scheduler.volume.info(),
            section: scheduler.section.info(),
            bytes: atlas.bytes(),
        }
    }

    pub fn take_requests(&mut self, device: &Device, queue: &Queue) -> Requests {
        let Self {
            scheduler, pyramid, ..
        } = self;
        let (Some(scheduler), Some(pyramid)) = (scheduler.as_mut(), pyramid.as_ref()) else {
            return Requests::default();
        };
        let volume = scheduler.volume.read(device, queue, pyramid);
        let section = scheduler.section.read(device, queue, pyramid);
        Requests {
            volume,
            section,
            volume_view: scheduler.volume.info(),
            section_view: scheduler.section.info(),
        }
    }
}

fn ensure_brick(
    device: &Device,
    layout: &BindGroupLayout,
    bricks: &mut [Option<BrickTable>],
    source: Level,
    level: usize,
    atlas: [u32; 3],
) {
    if bricks[level].is_none() {
        bricks[level] = Some(BrickTable::new(device, layout, level, source, atlas));
    }
}

fn clear_depth(encoder: &mut CommandEncoder, depth: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.depth.clear"),
        color_attachments: &[],
        depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
}

fn clear(encoder: &mut CommandEncoder, target: &TextureView) {
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

fn inside(region: [u32; 4], size: [u32; 2]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0] + region[2] <= size[0]
        && region[1] + region[3] <= size[1]
}

fn dummy_texture(device: &Device, queue: &Queue) -> TextureView {
    let texture: Texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.atlas.dummy"),
        size: wgpu::Extent3d {
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
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        cast_slice(&DUMMY_VOXEL),
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(2),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&TextureViewDescriptor {
        label: Some("grana.atlas.dummy.view"),
        ..Default::default()
    })
}
