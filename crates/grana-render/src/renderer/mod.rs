mod feedback;
mod render;
mod residency;
mod signature;

use residency::{FrameTarget, Levels, Regions, Residency};
use signature::Signature;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::cast_slice;
use glam::Vec3;
use grana_gpu::Readback;
use grana_volume::{
    BRICK_SIDE, BrickData, BrickGrid, BrickRequest, Clip, Level, PADDED_SIDE, Pyramid, Region,
};
use wgpu::{
    BindGroup, BindGroupLayout, CommandEncoder, Device, LoadOp, Operations, Queue,
    RenderPassColorAttachment, RenderPassDepthStencilAttachment, RenderPassDescriptor, Sampler,
    SamplerDescriptor, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::atlas::{Atlas, PACK, ResidentBrick};
use crate::brick::{BrickRecord, BrickTable};
use crate::census::{CENSUS_LEVELS, Census};
use crate::cut::CutLayer;
use crate::depth::DepthResolve;
use crate::frame::{FrameData, SectionData, VolumeView};
use crate::layout::Layouts;
use crate::marker::MarkerPasses;
use crate::mesh::MeshPasses;
use crate::oit::Oit;
use crate::pick::{self, PICK_ROW, PickId, PickPane, PickTarget};
use crate::planner::{
    self, Demand, Planner, Schedule, SectionRange, bucket_scale, decode_plan,
    resolve_section_level, resolve_volume_level, section_range, select_section_level,
    select_volume_level, volume_dispatch,
};
use crate::present::Present;
use crate::sample;
use crate::schema::SceneFrame;
use crate::section::SectionPass;
use crate::shadow::{self, Shadow, ShadowGroups, ShadowSpace};
use crate::slice::SlicePass;
use crate::target::{self, SceneTarget};
use crate::volume::VolumePass;
use grana_display::{Camera, SectionCamera};
use grana_volume::CHANNEL_LIMIT;

const DUMMY_VOXEL: [u16; PACK] = [0; PACK];

const FEEDBACK_SLOTS: usize = 4;

const SECTION_BUDGET_DIVISOR: usize = 2;

const FRAGMENT_BUDGET_DIVISOR: u64 = 2;

const PREFETCH_LIMIT: usize = 1024;

const PREFETCH_SHARE: usize = 4;

const CENSUS_BYTES: u64 = (CENSUS_LEVELS * size_of::<u32>()) as u64;

const UNIT: f32 = 65535.0;

const INTERACTIVE_SCALE: f32 = 0.5;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewInfo {
    pub level: usize,
    pub visible: usize,
    pub demand: usize,
    pub resident: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CutInfo {
    pub objects: u32,
    pub draws: u32,
    pub targets: u64,
    pub indices: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OitInfo {
    pub layers: u32,
    pub dropped: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub volume: ViewInfo,
    pub section: ViewInfo,
    pub cut: CutInfo,
    pub oit: OitInfo,
    pub bytes: u64,
    pub geometry: u64,
    pub fragments: u64,
    pub samples: u32,
    pub msaa: u32,
    pub settled: bool,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    #[default]
    Full,
    Interactive,
    Export,
}

impl Quality {
    pub fn scale(self) -> f32 {
        match self {
            Self::Full | Self::Export => 1.0,
            Self::Interactive => INTERACTIVE_SCALE,
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Full => 0,
            Self::Interactive => 1,
            Self::Export => 2,
        }
    }
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
    pub clip: Clip,
    pub exposure: f32,
    pub samples: u32,
    pub quality: Quality,
    pub scene: &'a SceneFrame<'b>,
    pub prefetch: bool,
}

pub struct Renderer {
    layouts: Layouts,
    present: Present,
    targets: [Option<FrameTarget>; 3],
    scene: Option<crate::table::SceneTable>,
    atlas: Option<Atlas>,
    bricks: Vec<Option<BrickTable>>,
    planner: Option<Planner>,
    census: Option<Census>,
    shadow: Option<Shadow>,
    volume: VolumePass,
    section: SectionPass,
    cut: CutLayer,
    slice: SlicePass,
    meshes: MeshPasses,
    markers: MarkerPasses,
    dummy: TextureView,
    dummy_oit: Oit,
    atlas_sampler: Sampler,
    pyramid: Option<Pyramid>,
    channels: usize,
    time: u32,
    budget: u64,
    feedback: Option<Readback>,
    frames: Vec<Option<Residency>>,
    counts: Vec<u32>,
    view_words: usize,
    view_bytes: u64,
    epoch: u64,
    content: u64,
    pin: u64,
    prefetch: bool,
    transparent: bool,
    object_key: Option<u64>,
    settled: Option<Residency>,
    planned: Option<Residency>,
    regions: Option<Regions>,
    render_scale: f32,
    pick_target: Option<PickTarget>,
    pick_readback: Option<Readback>,
    pick_pending: bool,
    oit_readback: Readback,
    dropped: u32,
    info: FrameInfo,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat, budget: u64) -> Self {
        assert!(budget > 0, "a renderer needs a residency budget");
        let layouts = Layouts::new(device);
        let dummy = dummy_texture(device, queue);
        let dummy_oit = Oit::new(device, &layouts, [1, 1], [0, 0, 1, 1], 1);
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
            targets: [None, None, None],
            scene: None,
            atlas: None,
            bricks: Vec::new(),
            planner: None,
            census: None,
            shadow: None,
            volume: VolumePass::new(device, &layouts),
            section: SectionPass::new(device, &layouts),
            cut: CutLayer::new(device, &layouts),
            slice: SlicePass::new(device, &layouts),
            meshes: MeshPasses::new(device, &layouts, budget / 2),
            markers: MarkerPasses::new(device, &layouts),
            layouts,
            dummy,
            dummy_oit,
            atlas_sampler,
            pyramid: None,
            channels: 0,
            time: 0,
            budget,
            feedback: None,
            frames: vec![None; FEEDBACK_SLOTS],
            counts: vec![0; CENSUS_LEVELS],
            view_words: 0,
            view_bytes: 0,
            epoch: 0,
            content: 0,
            pin: 0,
            prefetch: false,
            transparent: false,
            object_key: None,
            settled: None,
            planned: None,
            regions: None,
            render_scale: 1.0,
            pick_target: None,
            pick_readback: None,
            pick_pending: false,
            oit_readback: Readback::new(device, "grana.oit.dropped", 4, FEEDBACK_SLOTS),
            dropped: 0,
            info: FrameInfo::default(),
        }
    }

    pub fn format(&self) -> TextureFormat {
        self.present.format()
    }

    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
    }

    pub fn begin_export(&mut self) {
        self.targets[Quality::Export.index()] = None;
        self.settled = None;
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
        let packs = channels.div_ceil(PACK);
        let slot_bytes =
            (PADDED_SIDE as u64).pow(3) * size_of::<[u16; PACK]>() as u64 * packs as u64;
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
        let finest = pyramid.level(0);
        let world = Vec3::from(finest.world_extent());
        let spacing = finest.spacing();
        let voxel = Vec3::new(spacing.x(), spacing.y(), spacing.z());
        self.shadow = Some(Shadow::new(
            device,
            &self.layouts,
            shadow::resolution(world, voxel),
        ));
        self.object_key = None;
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
        self.targets = [None, None, None];
        self.regions = None;
        self.pick_readback = None;
        self.pick_pending = false;
        self.epoch += 1;
        self.pin = 0;
        self.settled = None;
        self.planned = None;
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
        if atlas.slot(time, level, index).is_some() || atlas.is_hollow(time, level, index) {
            return true;
        }
        let mut maxima = [[0.0f32; 4]; CHANNEL_LIMIT / 4];
        for channel in 0..*channels {
            let intensity = pyramid.channels()[channel].intensity();
            maxima[channel / 4][channel % 4] = data.channels()[channel].unit_max(intensity);
        }
        if maxima.iter().flatten().all(|maximum| *maximum <= 0.0) {
            atlas.assign_hollow(time, level, index);
            if let Some(table) = bricks[level].as_mut()
                && table.time() == Some(time)
            {
                table.set_record(queue, index, BrickRecord::HOLLOW);
            }
            *content += 1;
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
            other.set_record(queue, brick.index, BrickRecord::MISSING);
        }
        let voxels = data.channels()[0].voxel_count();
        let mut units = vec![0u16; voxels * PACK];
        for pack in 0..(*channels).div_ceil(PACK) {
            units.fill(0);
            for component in 0..PACK {
                let channel = pack * PACK + component;
                if channel >= *channels {
                    break;
                }
                let intensity = pyramid.channels()[channel].intensity();
                data.channels()[channel]
                    .write_units_strided(intensity, &mut units, PACK, component);
            }
            atlas.write(queue, pack, slot, &units);
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
        assert!(
            frame.size[0] > 0 && frame.size[1] > 0,
            "the surface is not empty"
        );
        assert!(
            frame.exposure.is_finite() && frame.exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        assert!(frame.samples > 0, "a target resolves at least one sample");
        let quality = frame.quality;
        let size = crate::scale::size(frame.size, quality.scale());
        let index = quality.index();
        if self.targets[index]
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            self.targets[index] =
                Some(FrameTarget::new(device, &self.layouts, &self.present, size));
            self.settled = None;
        }
        self.render_scale = quality.scale();
        let mut target = self.targets[index]
            .take()
            .expect("the frame target is ready");
        let info = self.render(device, queue, &mut target, frame);
        self.targets[index] = Some(target);
        info
    }

    pub fn request_pick(
        &mut self,
        device: &Device,
        queue: &Queue,
        pane: PickPane,
        point: [u32; 2],
    ) -> bool {
        if self.pick_pending {
            return false;
        }
        let Some(regions) = self.regions else {
            return false;
        };
        let point = crate::scale::point(point, self.render_scale);
        let region = match pane {
            PickPane::Volume => regions.volume,
            PickPane::Section => regions.section,
        };
        let inside = point[0] >= region[0]
            && point[0] < region[0] + region[2]
            && point[1] >= region[1]
            && point[1] < region[1] + region[3];
        if !inside {
            return false;
        }
        let offset = [point[0] - region[0], point[1] - region[1]];
        let Self {
            scene,
            meshes,
            markers,
            slice,
            pick_target,
            pick_readback,
            pick_pending,
            ..
        } = self;
        let Some(scene) = scene.as_ref() else {
            return false;
        };
        let target = PickTarget::ensure(device, pick_target);
        let readback = pick_readback
            .get_or_insert_with(|| Readback::new(device, "grana.pick.readback", PICK_ROW, 1));
        let Some(slot) = readback.reserve() else {
            return false;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.pick"),
        });
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.pick.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target.view(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: match pane {
                PickPane::Volume => Some(RenderPassDepthStencilAttachment {
                    view: target.depth(),
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                PickPane::Section => None,
            },
            ..Default::default()
        });
        pass.set_viewport(
            -(offset[0] as f32),
            -(offset[1] as f32),
            region[2] as f32,
            region[3] as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(0, 0, 1, 1);
        match pane {
            PickPane::Volume => {
                meshes.draw_pick(&mut pass);
                markers.draw_pick_volume(&mut pass, scene.group());
            }
            PickPane::Section => {
                markers.draw_pick_section_behind(&mut pass, scene.group());
                slice.draw_pick(&mut pass, scene.group());
                markers.draw_pick_section_front(&mut pass, scene.group());
            }
        }
        drop(pass);
        readback.stage_pixel(&mut encoder, slot, target.color(), PICK_ROW);
        queue.submit([encoder.finish()]);
        readback.register();
        *pick_pending = true;
        true
    }

    pub fn poll_pick(&mut self, device: &Device) -> Option<PickId> {
        if !self.pick_pending {
            return None;
        }
        let readback = self.pick_readback.as_mut()?;
        let (_, bytes) = readback.take(device)?;
        self.pick_pending = false;
        Some(pick::decode(&bytes))
    }

    pub fn pick_pending(&self) -> bool {
        self.pick_pending
    }
}

pub(super) fn ensure_brick(
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

pub(super) fn dummy_texture(device: &Device, queue: &Queue) -> TextureView {
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
        format: TextureFormat::Rgba16Unorm,
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
            bytes_per_row: Some(size_of::<[u16; PACK]>() as u32),
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
