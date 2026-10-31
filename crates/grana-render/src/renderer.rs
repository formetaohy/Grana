use bytemuck::cast_slice;
use grana_gpu::Readback;
use grana_volume::{
    BRICK_SIDE, BrickCoord, BrickData, BrickGrid, Level, PADDED_SIDE, Pyramid, Region,
};
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
use crate::census::{CENSUS_LEVELS, Census};
use crate::frame::{FrameData, SectionData, VolumeView};
use crate::layout::Layouts;
use crate::marker::MarkerPasses;
use crate::mesh::MeshPasses;
use crate::present::Present;
use crate::schedule::{
    Cull, CullPass, Scheduler, SectionRange, VIEW_WORDS, resolve_section_level,
    resolve_volume_level, section_range, select_section_level, select_volume_level, view_requests,
    view_visible, volume_dispatch,
};
use crate::schema::SceneFrame;
use crate::section::SectionPass;
use crate::target::SceneTarget;
use crate::volume::VolumePass;

const DUMMY_VOXEL: [u16; 1] = [0];
const FEEDBACK_SLOTS: usize = 4;
const SECTION_BUDGET_DIVISOR: usize = 2;
const VIEW_BYTES: u64 = (VIEW_WORDS * size_of::<u32>()) as u64;
const CENSUS_BYTES: u64 = (CENSUS_LEVELS * size_of::<u32>()) as u64;
const FEEDBACK_WORDS: usize = 2 * VIEW_WORDS + CENSUS_LEVELS;

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
    pub time: u32,
    pub volume: Vec<BrickRequest>,
    pub section: Vec<BrickRequest>,
    pub volume_view: ViewInfo,
    pub section_view: ViewInfo,
}

pub struct Frame<'a, 'b> {
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub volume_region: [u32; 4],
    pub section_region: [u32; 4],
    pub time: u32,
    pub camera: &'a Camera,
    pub section: &'a SectionCamera,
    pub region: Region,
    pub scene: &'a SceneFrame<'b>,
}

#[derive(Clone, Copy, PartialEq)]
struct Levels {
    volume: usize,
    section: usize,
    volume_capacity: usize,
    section_capacity: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct Residency {
    epoch: u64,
    content: u64,
    time: u32,
    eye: [f32; 3],
    focus: [f32; 3],
    field_of_view: f32,
    axis: grana_volume::Axis,
    span: f32,
    volume: [u32; 2],
    section: [u32; 2],
    region: Region,
    levels: Levels,
}

struct ResidencyView<'a> {
    camera: &'a Camera,
    section: &'a SectionCamera,
    volume_region: [u32; 4],
    section_region: [u32; 4],
    region: Region,
}

impl Residency {
    fn of(epoch: u64, content: u64, time: u32, view: ResidencyView<'_>, levels: Levels) -> Self {
        Self {
            epoch,
            content,
            time,
            eye: view.camera.eye().to_array(),
            focus: view.camera.focus().to_array(),
            field_of_view: view.camera.fov_y(),
            axis: view.section.axis(),
            span: view.section.span(),
            volume: [view.volume_region[2], view.volume_region[3]],
            section: [view.section_region[2], view.section_region[3]],
            region: view.region,
            levels,
        }
    }
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
    census: Option<Census>,
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
    feedback: Readback,
    frames: Vec<Option<Residency>>,
    counts: Vec<u32>,
    epoch: u64,
    content: u64,
    settled: Option<Residency>,
    info: FrameInfo,
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
            census: None,
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
            feedback: Readback::new(
                device,
                "grana.feedback",
                (FEEDBACK_WORDS * size_of::<u32>()) as u64,
                FEEDBACK_SLOTS,
            ),
            frames: vec![None; FEEDBACK_SLOTS],
            counts: vec![0; CENSUS_LEVELS],
            epoch: 0,
            content: 0,
            settled: None,
            info: FrameInfo::default(),
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
        self.scheduler = Some(Scheduler::new(device, &self.layouts));
        self.census = Some(Census::new(device, &self.layouts, pyramid));
        self.scene = Some(crate::table::SceneTable::new(
            device,
            &self.layouts,
            channels,
        ));
        self.atlas = Some(atlas);
        self.pyramid = Some(pyramid.clone());
        self.channels = channels;
        self.tick = 0;
        self.counts.fill(0);
        self.epoch += 1;
        self.settled = None;
        self.info = FrameInfo::default();
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
            content,
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
        *content += 1;
        true
    }

    pub fn draw(&mut self, device: &Device, queue: &Queue, frame: Frame<'_, '_>) -> FrameInfo {
        let Frame {
            target,
            size,
            volume_region,
            section_region,
            time,
            camera,
            section,
            region,
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
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.frame"),
        });
        let Self {
            layouts,
            present,
            present_group,
            target: scene_target,
            scene: scene_table,
            atlas,
            bricks,
            scheduler,
            census,
            volume,
            section: section_pass,
            meshes,
            markers,
            pyramid,
            channels,
            feedback,
            frames,
            counts,
            epoch,
            content,
            settled,
            info,
            ..
        } = self;
        let (Some(pyramid), Some(atlas), Some(scene_table), Some(scheduler), Some(census)) = (
            pyramid.as_ref(),
            atlas.as_mut(),
            scene_table.as_mut(),
            scheduler.as_mut(),
            census.as_ref(),
        ) else {
            clear(&mut encoder, target);
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
        let scene_target = scene_target.as_ref().expect("the scene target is ready");
        let present_group = present_group.as_ref().expect("the present group is ready");
        let volume_width = volume_region[2].max(1);
        let volume_height = volume_region[3].max(1);
        let section_width = section_region[2].max(1);
        let section_height = section_region[3].max(1);
        let volume_aspect = volume_width as f32 / volume_height as f32;
        let section_aspect = section_width as f32 / section_height as f32;
        let slots = atlas.slots();
        let level_count = pyramid.level_count();
        let section_target_level = select_section_level(pyramid, section, section_height as f32);
        let (section_level, section_count) = resolve_section_level(
            pyramid,
            camera,
            section,
            section_aspect,
            section_target_level,
            (slots / SECTION_BUDGET_DIVISOR).max(1),
            region,
        );
        let volume_target_level = select_volume_level(pyramid, camera, volume_height as f32);
        let volume_capacity = slots.saturating_sub(section_count).max(1);
        let volume_level =
            resolve_volume_level(volume_target_level, level_count, volume_capacity, counts);
        let section_capacity = section_count.clamp(1, slots);
        let levels = Levels {
            volume: volume_level,
            section: section_level,
            volume_capacity,
            section_capacity,
        };
        let residency = Residency::of(
            *epoch,
            *content,
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
        let unsettled = *settled != Some(residency);
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
        let section_table = bricks[section_level]
            .as_ref()
            .expect("the section level table exists");
        let volume_table = bricks[volume_level]
            .as_ref()
            .expect("the volume level table exists");
        let section_data =
            SectionData::new(camera, section, section_aspect, section_table, *channels);
        let frame_data = FrameData::new(
            VolumeView {
                camera,
                region,
                aspect: volume_aspect,
                height: volume_height,
            },
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
        if unsettled {
            census.prepare(queue);
        }
        let range = section_range(
            pyramid,
            section_level,
            camera,
            section,
            section_aspect,
            region,
        );
        scheduler.section.prepare(
            queue,
            Cull {
                time,
                level: section_level,
                capacity: section_capacity as u32,
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
        if unsettled {
            census.encode(&mut encoder, scene_group);
        }
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
        .encode(&mut encoder);
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
        .encode(&mut encoder);
        meshes.cull_volume(&mut encoder, scene_group);
        meshes.cull_section(&mut encoder, scene_group);
        markers.cull_volume(&mut encoder, scene_group);
        markers.cull_section(&mut encoder, scene_group);
        present.background(&mut encoder, scene_target.color(), volume_region);
        present.write(queue);
        clear_depth(&mut encoder, scene_target.depth());
        meshes.draw_volume(
            &mut encoder,
            scene_target.color(),
            scene_target.depth(),
            volume_region,
        );
        markers.draw_volume(
            &mut encoder,
            scene_target.color(),
            scene_target.depth(),
            volume_region,
            scene_group,
        );
        volume.draw(
            &mut encoder,
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
            &mut encoder,
            crate::section::SectionFrame {
                target: scene_target.color(),
                region: section_region,
                scene: scene_group,
                level: section_table.group(),
                atlas: atlas.group(),
            },
        );
        meshes.draw_section(
            &mut encoder,
            scene_target.color(),
            scene_target.depth(),
            section_region,
            scene_group,
        );
        markers.draw_section(
            &mut encoder,
            scene_target.color(),
            section_region,
            scene_group,
        );
        present.present(&mut encoder, target, present_group);
        if unsettled && let Some(slot) = feedback.reserve() {
            feedback.stage(&mut encoder, slot, 0, scheduler.volume.buffer(), VIEW_BYTES);
            feedback.stage(
                &mut encoder,
                slot,
                VIEW_BYTES,
                scheduler.section.buffer(),
                VIEW_BYTES,
            );
            feedback.stage(
                &mut encoder,
                slot,
                2 * VIEW_BYTES,
                census.counts(),
                CENSUS_BYTES,
            );
            frames[slot] = Some(residency);
        }
        queue.submit([encoder.finish()]);
        feedback.register();
        info.volume = scheduler.volume.info();
        info.section = scheduler.section.info();
        info.bytes = atlas.bytes();
        *info
    }

    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        let mut latest = None;
        while let Some((slot, bytes)) = self.feedback.take(device) {
            let Some(meta) = self.frames[slot].take() else {
                continue;
            };
            if let Some(requests) = self.decode(meta, &bytes) {
                latest = Some(requests);
            }
        }
        latest
    }

    pub fn requests_pending(&self) -> bool {
        self.feedback.pending()
    }

    fn decode(&mut self, meta: Residency, bytes: &[u8]) -> Option<Requests> {
        if meta.epoch != self.epoch {
            return None;
        }
        let pyramid = self.pyramid.as_ref()?;
        let words = feedback_words(bytes);
        if words.len() < FEEDBACK_WORDS {
            return None;
        }
        let volume_words = &words[..VIEW_WORDS];
        let section_words = &words[VIEW_WORDS..2 * VIEW_WORDS];
        let census_words = &words[2 * VIEW_WORDS..FEEDBACK_WORDS];
        let volume = view_requests(
            volume_words,
            meta.time,
            meta.levels.volume,
            meta.levels.volume_capacity,
            pyramid,
        );
        let section = view_requests(
            section_words,
            meta.time,
            meta.levels.section,
            meta.levels.section_capacity,
            pyramid,
        );
        let volume_view = ViewInfo {
            level: meta.levels.volume,
            visible: volume_words[0] as usize,
            resident: volume_words[1] as usize,
        };
        let section_view = ViewInfo {
            level: meta.levels.section,
            visible: section_words[0] as usize,
            resident: section_words[1] as usize,
        };
        if let Some(scheduler) = self.scheduler.as_mut() {
            scheduler
                .volume
                .record(volume_view.visible, volume_view.resident);
            scheduler
                .section
                .record(section_view.visible, section_view.resident);
        }
        if let Some(atlas) = self.atlas.as_mut() {
            atlas.touch(
                meta.time,
                meta.levels.volume,
                view_visible(volume_words, meta.levels.volume_capacity),
                self.tick,
            );
            atlas.touch(
                meta.time,
                meta.levels.section,
                view_visible(section_words, meta.levels.section_capacity),
                self.tick,
            );
        }
        self.counts.copy_from_slice(census_words);
        self.info.volume = volume_view;
        self.info.section = section_view;
        self.settled = (volume.is_empty()
            && section.is_empty()
            && volume_view.resident == volume_view.visible
            && section_view.resident == section_view.visible)
            .then_some(meta);
        Some(Requests {
            time: meta.time,
            volume,
            section,
            volume_view,
            section_view,
        })
    }
}

fn feedback_words(bytes: &[u8]) -> Vec<u32> {
    let (words, _) = bytes.as_chunks::<4>();
    words.iter().map(|word| u32::from_ne_bytes(*word)).collect()
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
