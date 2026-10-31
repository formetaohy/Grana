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
use crate::light::Light;
use crate::marker::MarkerPasses;
use crate::mesh::MeshPasses;
use crate::planner::{
    self, Demand, Planner, Schedule, SectionRange, bucket_scale, decode_plan,
    resolve_section_level, resolve_volume_level, section_range, select_section_level,
    select_volume_level, volume_dispatch,
};
use crate::present::Present;
use crate::schema::SceneFrame;
use crate::section::SectionPass;
use crate::target::SceneTarget;
use crate::volume::VolumePass;

const DUMMY_VOXEL: [u16; 1] = [0];
const FEEDBACK_SLOTS: usize = 4;
const SECTION_BUDGET_DIVISOR: usize = 2;
const PREFETCH_LIMIT: usize = 1024;
const CENSUS_BYTES: u64 = (CENSUS_LEVELS * size_of::<u32>()) as u64;

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
    pub demand: usize,
    pub resident: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub volume: ViewInfo,
    pub section: ViewInfo,
    pub bytes: u64,
    pub geometry: u64,
}

#[derive(Default)]
pub struct Requests {
    pub time: u32,
    pub volume: Vec<BrickRequest>,
    pub section: Vec<BrickRequest>,
    pub prefetch: Vec<BrickRequest>,
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
    pub prefetch: bool,
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
    style: u64,
    prefetch: bool,
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
    fn of(
        epoch: u64,
        content: u64,
        style: u64,
        prefetch: bool,
        time: u32,
        view: ResidencyView<'_>,
        levels: Levels,
    ) -> Self {
        Self {
            epoch,
            content,
            style,
            prefetch,
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
    planner: Option<Planner>,
    census: Option<Census>,
    light: Option<Light>,
    volume: VolumePass,
    section: SectionPass,
    meshes: MeshPasses,
    markers: MarkerPasses,
    dummy: TextureView,
    atlas_sampler: Sampler,
    pyramid: Option<Pyramid>,
    channels: usize,
    time: u32,
    budget: u64,
    supersample: u32,
    feedback: Option<Readback>,
    frames: Vec<Option<Residency>>,
    counts: Vec<u32>,
    view_words: usize,
    view_bytes: u64,
    epoch: u64,
    content: u64,
    pin: u64,
    prefetch: bool,
    settled: Option<Residency>,
    info: FrameInfo,
}

impl Renderer {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        budget: u64,
        supersample: u32,
    ) -> Self {
        assert!(
            (1..=4).contains(&supersample),
            "a renderer resolves one to four samples per axis"
        );
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
            planner: None,
            census: None,
            light: None,
            volume: VolumePass::new(device, &layouts),
            section: SectionPass::new(device, &layouts),
            meshes: MeshPasses::new(device, &layouts, budget / 2),
            markers: MarkerPasses::new(device, &layouts),
            layouts,
            dummy,
            atlas_sampler,
            pyramid: None,
            channels: 0,
            time: 0,
            budget,
            supersample,
            feedback: None,
            frames: vec![None; FEEDBACK_SLOTS],
            counts: vec![0; CENSUS_LEVELS],
            view_words: 0,
            view_bytes: 0,
            epoch: 0,
            content: 0,
            pin: 0,
            prefetch: false,
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
        let slots = atlas.slots();
        self.view_words = planner::plan_words(slots);
        self.view_bytes = (self.view_words * size_of::<u32>()) as u64;
        self.feedback = Some(Readback::new(
            device,
            "grana.feedback",
            (2 * self.view_words + CENSUS_LEVELS) as u64 * size_of::<u32>() as u64,
            FEEDBACK_SLOTS,
        ));
        self.frames = vec![None; FEEDBACK_SLOTS];
        self.bricks = (0..pyramid.level_count()).map(|_| None).collect();
        self.planner = Some(Planner::new(device, &self.layouts, slots));
        self.census = Some(Census::new(device, &self.layouts, pyramid));
        self.light = Some(Light::new(
            device,
            &self.layouts,
            pyramid.level(0).world_extent(),
        ));
        self.scene = Some(crate::table::SceneTable::new(
            device,
            &self.layouts,
            channels,
        ));
        self.atlas = Some(atlas);
        self.pyramid = Some(pyramid.clone());
        self.channels = channels;
        self.time = 0;
        self.counts.fill(0);
        self.meshes.clear();
        self.epoch += 1;
        self.pin = 0;
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
            time: now,
            content,
            pin,
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
        let Some((slot, evicted)) = atlas.allocate(*pin, *now, time) else {
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
            *pin,
        );
        if let Some(table) = bricks[level].as_mut()
            && table.time() == Some(time)
        {
            table.set_record(queue, index, BrickRecord::new(slot + 1, maxima));
        }
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
            prefetch,
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
            let scene_target =
                SceneTarget::new(device, &self.layouts.depth, size, self.supersample);
            self.present_group = Some(self.present.bind(device, scene_target.color()));
            self.target = Some(scene_target);
        }
        let render_volume = supersampled(volume_region, self.supersample);
        let render_section = supersampled(section_region, self.supersample);
        self.time = time;
        self.prefetch = prefetch;
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
            planner,
            census,
            volume,
            section: section_pass,
            meshes,
            markers,
            pyramid,
            channels,
            supersample,
            feedback,
            frames,
            counts,
            view_bytes,
            epoch,
            content,
            settled,
            light,
            info,
            ..
        } = self;
        let supersample = *supersample;
        let (Some(pyramid), Some(atlas), Some(scene_table), Some(planner), Some(census)) = (
            pyramid.as_ref(),
            atlas.as_mut(),
            scene_table.as_mut(),
            planner.as_mut(),
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
        assert!(
            inside(render_volume, scene_target.render())
                && inside(render_section, scene_target.render()),
            "the supersampled panes lie on the render target"
        );
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
            supersample,
        );
        scene_table.write(
            device,
            queue,
            pyramid,
            scene.channels,
            frame_data,
            section_data,
        );
        let range = section_range(
            pyramid,
            section_level,
            camera,
            section,
            section_aspect,
            region,
        );
        if unsettled {
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
        }
        meshes.set(device, queue, layouts, scene);
        meshes.write_uniforms(queue, &frame_data, scene.shading);
        markers.prepare(device, queue, layouts, scene.markers);
        let scene_group = scene_table.group();
        if unsettled {
            census.encode(&mut encoder, scene_group);
            planner.encode(&mut encoder, scene_group, &planner.volume, &planner.section);
        }
        if scene.shading.shadows > 0.0
            && let Some(light) = light.as_ref()
        {
            light.encode(
                &mut encoder,
                scene_group,
                volume_table.group(),
                atlas.group(),
            );
        }
        meshes.cull_volume(&mut encoder, scene_group);
        meshes.cull_section(&mut encoder, scene_group);
        markers.cull_volume(&mut encoder, scene_group);
        markers.cull_section(&mut encoder, scene_group);
        present.background(&mut encoder, scene_target.color(), render_volume);
        present.write(queue, supersample);
        clear_depth(&mut encoder, scene_target.depth());
        meshes.draw_volume(
            &mut encoder,
            scene_target.color(),
            scene_target.depth(),
            render_volume,
        );
        markers.draw_volume(
            &mut encoder,
            scene_target.color(),
            scene_target.depth(),
            render_volume,
            scene_group,
        );
        let light_group = light
            .as_ref()
            .map(Light::group)
            .expect("the light is ready");
        volume.draw(
            &mut encoder,
            crate::volume::VolumeFrame {
                target: scene_target.color(),
                region: render_volume,
                scene: scene_group,
                level: volume_table.group(),
                atlas: atlas.group(),
                depth_group: scene_target.depth_group(),
                light: light_group,
            },
        );
        section_pass.draw(
            &mut encoder,
            crate::section::SectionFrame {
                target: scene_target.color(),
                region: render_section,
                scene: scene_group,
                level: section_table.group(),
                atlas: atlas.group(),
            },
        );
        meshes.draw_section(
            &mut encoder,
            scene_target.color(),
            scene_target.depth(),
            render_section,
            scene_group,
        );
        markers.draw_section(
            &mut encoder,
            scene_target.color(),
            render_section,
            scene_group,
        );
        present.present(&mut encoder, target, present_group);
        if unsettled
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
        info.volume.level = volume_level;
        info.section.level = section_level;
        info.bytes = atlas.bytes();
        info.geometry = meshes.bytes();
        *info
    }

    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        let mut latest = None;
        while let Some((slot, bytes)) = self.take_feedback(device) {
            let Some(meta) = self.frames[slot].take() else {
                continue;
            };
            if let Some(requests) = self.decode(meta, &bytes) {
                latest = Some(requests);
            }
        }
        latest
    }

    fn take_feedback(&mut self, device: &Device) -> Option<(usize, Vec<u8>)> {
        self.feedback
            .as_mut()
            .and_then(|feedback| feedback.take(device))
    }

    pub fn requests_pending(&self) -> bool {
        self.feedback.as_ref().is_some_and(Readback::pending)
    }

    fn decode(&mut self, meta: Residency, bytes: &[u8]) -> Option<Requests> {
        if meta.epoch != self.epoch {
            return None;
        }
        let Self {
            pyramid,
            planner,
            atlas,
            counts,
            view_words,
            info,
            settled,
            pin,
            prefetch,
            ..
        } = self;
        let pyramid = pyramid.as_ref()?;
        let planner = planner.as_ref()?;
        let atlas = atlas.as_mut()?;
        let words = feedback_words(bytes);
        if words.len() < 2 * *view_words + CENSUS_LEVELS {
            return None;
        }
        let (volume_words, rest) = words.split_at(*view_words);
        let (section_words, census_words) = rest.split_at(*view_words);
        let volume = decode_plan(
            volume_words,
            planner.volume.capacity(),
            meta.time,
            meta.levels.volume,
            pyramid,
        )?;
        let section = decode_plan(
            section_words,
            planner.section.capacity(),
            meta.time,
            meta.levels.section,
            pyramid,
        )?;
        *pin += 1;
        atlas.touch(
            meta.time,
            meta.levels.volume,
            volume.indices.iter().copied(),
            *pin,
        );
        atlas.touch(
            meta.time,
            meta.levels.section,
            section.indices.iter().copied(),
            *pin,
        );
        counts.copy_from_slice(census_words);
        info.volume = volume.info;
        info.section = section.info;
        let settled_now = volume.requests.is_empty()
            && section.requests.is_empty()
            && volume.info.resident == volume.info.demand
            && section.info.resident == section.info.demand;
        *settled = settled_now.then_some(meta);
        let prefetched = prefetch_requests(*prefetch, atlas, meta.time, &volume, &section, pyramid);
        Some(Requests {
            time: meta.time,
            volume: volume.requests,
            section: section.requests,
            prefetch: prefetched,
            volume_view: volume.info,
            section_view: section.info,
        })
    }
}

fn channel_style(channels: &[crate::ChannelRow<'_>]) -> u64 {
    let mut key = channels.len() as u64;
    for channel in channels {
        let mut row = (channel.visible as u64) ^ ((channel.opacity.to_bits() as u64) << 1);
        row = row.rotate_left(17) ^ (channel.window.min().to_bits() as u64);
        row = row.rotate_left(17) ^ (channel.window.max().to_bits() as u64);
        for stop in channel.transfer.stops() {
            row = row.rotate_left(11) ^ (stop.position().to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.opacity().to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.color()[0].to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.color()[1].to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.color()[2].to_bits() as u64);
        }
        key = key.rotate_left(13) ^ row;
    }
    key
}

fn prefetch_requests(
    enabled: bool,
    atlas: &Atlas,
    time: u32,
    volume: &Demand,
    section: &Demand,
    pyramid: &Pyramid,
) -> Vec<BrickRequest> {
    if !enabled {
        return Vec::new();
    }
    let next = time + 1;
    if next >= pyramid.frame_count() {
        return Vec::new();
    }
    let resident = if volume.info.level == section.info.level {
        atlas.bricks(next, volume.info.level).count()
    } else {
        atlas.bricks(next, volume.info.level).count()
            + atlas.bricks(next, section.info.level).count()
    };
    let mut spare = atlas
        .slots()
        .saturating_sub(volume.info.demand + section.info.demand + resident)
        .min(PREFETCH_LIMIT);
    let mut requests = Vec::new();
    for (demand, level) in [(volume, volume.info.level), (section, section.info.level)] {
        if spare == 0 {
            break;
        }
        let grid = BrickGrid::new(pyramid.level(level).extent());
        for index in &demand.indices {
            if spare == 0 {
                break;
            }
            if atlas.slot(next, level, *index as usize).is_some() {
                continue;
            }
            requests.push(BrickRequest {
                time: next,
                level,
                coord: grid.coord(*index as usize),
            });
            spare -= 1;
        }
    }
    requests
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

fn supersampled(region: [u32; 4], supersample: u32) -> [u32; 4] {
    [
        region[0] * supersample,
        region[1] * supersample,
        region[2] * supersample,
        region[3] * supersample,
    ]
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
