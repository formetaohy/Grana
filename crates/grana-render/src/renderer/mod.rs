mod feedback;
mod render;
mod residency;
mod signature;
mod upload;

use residency::{
    Content, FrameTarget, LevelKey, Levels, Mode, PaneLevels, PaneRegion, PaneResidency, Regions,
    Residency, SectionLevels, VolumeLevels,
};
use signature::{PaneSignature, Signature};
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::cast_slice;
use glam::Vec3;
use grana_display::{Camera, ChannelMask, PANE_COUNT, PaneKind, Quality, SectionCamera};
use grana_gpu::Readback;
use grana_volume::{
    BRICK_SIDE, BrickCoord, BrickGrid, BrickRequest, Clip, Level, PADDED_SIDE, Pyramid, Region,
};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, CommandEncoder, Device,
    Extent3d, LoadOp, Operations, Queue, RenderPassColorAttachment,
    RenderPassDepthStencilAttachment, RenderPassDescriptor, Sampler, SamplerDescriptor, StoreOp,
    TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::atlas::{Atlas, PACK, pack_components, pack_texels};
use crate::brick::{BrickArena, BrickTable};
use crate::composite::CompositePass;
use crate::converge::Convergence;
use crate::cut::CutLayer;
use crate::frame::{FrameData, SectionData, SharedView, VolumeView};
use crate::layout::Layouts;
use crate::marker::MarkerPasses;
use crate::mesh::MeshPasses;
use crate::oit::Oit;
use crate::pick::{self, PICK_ROW, PickId, PickTarget};
use crate::planner::{
    self, BAND, Demand, FitView, Ladder, Limits, Planner, Schedule, SectionKey, SectionMeasure,
    SectionPick, SectionProbe, SectionRange, bucket_scale, capacities, decode_plan, fit_volume,
    resolve_section_level, section_range, select_section_level, volume_dispatch,
};
use crate::present::Present;
use crate::profile::{Budget, FrameProfile, Marks, Profiler, STAGE_COUNT, Stage};
use crate::sample;
use crate::schema::SceneFrame;
use crate::section::SectionPass;
use crate::shadow::{self, Shadow, ShadowGroups, ShadowSpace};
use crate::slice::SlicePass;
use crate::summary::Summary;
use crate::target::{self, SceneTarget};
use crate::volume::VolumePass;
use grana_volume::CHANNEL_LIMIT;

const DUMMY_VOXEL: [u16; PACK] = [0; PACK];

const FEEDBACK_SLOTS: usize = 4;

const FRAGMENT_BUDGET_DIVISOR: u64 = 2;

const PREFETCH_LIMIT: usize = 1024;

const PREFETCH_SHARE: usize = 4;

const RECORD_SHARE: u64 = 8;

const FADE_DRAWS: f32 = 10.0;

const UNIT: f32 = 65535.0;

const FRAME_TARGET: f32 = 14.0;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Fade {
    pub(super) weight: f32,
    pub(super) samples: u32,
}

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
    pub indices: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OitInfo {
    pub layers: u32,
    pub dropped: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct FrameInfo {
    pub panes: [ViewInfo; PANE_COUNT],
    pub volume_scale: [u32; PANE_COUNT],
    pub cut: CutInfo,
    pub oit: OitInfo,
    pub bytes: u64,
    pub geometry: u64,
    pub fragments: u64,
    pub samples: u32,
    pub settled: bool,
    pub truncated: bool,
    pub fading: bool,
    pub profile: FrameProfile,
}

impl Default for FrameInfo {
    fn default() -> Self {
        Self {
            panes: [ViewInfo::default(); PANE_COUNT],
            volume_scale: [1; PANE_COUNT],
            cut: CutInfo::default(),
            oit: OitInfo::default(),
            bytes: 0,
            geometry: 0,
            fragments: 0,
            samples: 0,
            settled: false,
            truncated: false,
            fading: false,
            profile: FrameProfile::default(),
        }
    }
}

#[derive(Default)]
pub struct Requests {
    pub time: u32,
    pub volume: Vec<BrickRequest>,
    pub section: Vec<BrickRequest>,
    pub prefetch: Vec<BrickRequest>,
    pub panes: [ViewInfo; PANE_COUNT],
}

#[derive(Clone, Copy)]
pub(crate) struct VolumeTargets<'a> {
    pub(crate) color: &'a TextureView,
    pub(crate) depth: &'a TextureView,
}

#[derive(Clone, Copy)]
pub(crate) struct Lighting<'a> {
    pub(crate) shadow: &'a BindGroup,
    pub(crate) ao: &'a BindGroup,
}

#[derive(Clone, Copy, Debug)]
pub struct Lens<'a> {
    pub camera: &'a Camera,
    pub aspect: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum PaneFrame<'a> {
    Volume {
        camera: &'a Camera,
        lens: Lens<'a>,
        region: [u32; 4],
    },
    Section {
        section: &'a SectionCamera,
        region: [u32; 4],
    },
}

impl PaneFrame<'_> {
    pub fn kind(&self) -> PaneKind {
        match self {
            Self::Volume { .. } => PaneKind::Volume,
            Self::Section { .. } => PaneKind::Section,
        }
    }

    pub fn region(&self) -> [u32; 4] {
        match self {
            Self::Volume { region, .. } | Self::Section { region, .. } => *region,
        }
    }
}

pub struct Frame<'a, 'b> {
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub panes: [Option<PaneFrame<'a>>; PANE_COUNT],
    pub time: u32,
    pub region: Region,
    pub clip: Clip,
    pub exposure: f32,
    pub samples: u32,
    pub scene: &'a SceneFrame<'b>,
    pub pane_channels: [ChannelMask; PANE_COUNT],
    pub pane_quality: [Quality; PANE_COUNT],
    pub prefetch: bool,
    pub level_cap: usize,
    pub assured: bool,
}

pub struct Renderer {
    layouts: Layouts,
    present: Present,
    target: Option<FrameTarget>,
    scene: Option<crate::table::SceneTable>,
    atlas: Option<Atlas>,
    summary: Option<Summary>,
    staging: upload::UploadScratch,
    arena: Option<BrickArena>,
    level_group: Option<BindGroup>,
    bricks: Vec<Option<BrickTable>>,
    planner: Option<Planner>,
    shadow: Option<Shadow>,
    volume: VolumePass,
    composite: CompositePass,
    gtao: crate::ao::AoPipeline,
    section: SectionPass,
    cut: [CutLayer; PANE_COUNT],
    slice: [SlicePass; PANE_COUNT],
    meshes: MeshPasses,
    markers: MarkerPasses,
    dummy: TextureView,
    dummy_oit: Oit,
    dummy_ao: BindGroup,
    atlas_sampler: Sampler,
    pyramid: Option<Pyramid>,
    channels: usize,
    time: u32,
    volume_level: [Option<usize>; PANE_COUNT],
    section_level: [Option<usize>; PANE_COUNT],
    motion: [Option<u64>; PANE_COUNT],
    section_probe: [SectionProbe; PANE_COUNT],
    section_measure: [Option<(u64, SectionMeasure)>; PANE_COUNT],
    section_sequence: u64,
    probe_mark: [u64; PANE_COUNT],
    level_cap: Option<usize>,
    budget: u64,
    slot_bytes: u64,
    feedback: Option<Readback>,
    frames: Vec<Option<Residency>>,
    view_words: usize,
    view_bytes: u64,
    epoch: u64,
    content: u64,
    pin: u64,
    prefetch: bool,
    prefetched: HashSet<BrickRequest>,
    prefetch_key: Option<u64>,
    prefetch_issued: Vec<BrickRequest>,
    prefetch_rate: f32,
    transparent: bool,
    exporting: bool,
    convergence: Convergence,
    fade: Option<Fade>,
    object_key: Option<u64>,
    settled: Option<Residency>,
    planned: Option<Residency>,
    regions: Option<Regions>,
    pick_target: Option<PickTarget>,
    pick_readback: Option<Readback>,
    pick_pending: bool,
    oit_readback: [Readback; PANE_COUNT],
    dropped: [u32; PANE_COUNT],
    profile: Option<Profiler>,
    frame_budget: Budget,
    measured: [f32; STAGE_COUNT],
    sampled: bool,
    info: FrameInfo,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat, budget: u64) -> Self {
        assert!(budget > 0, "a renderer needs a residency budget");
        let layouts = Layouts::new(device);
        let dummy = dummy_texture(device, queue);
        let dummy_oit = Oit::new(device, &layouts, [1, 1], 1, 1);
        let dummy_ao = crate::ao::dummy(device, queue, &layouts);
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
            target: None,
            scene: None,
            atlas: None,
            summary: None,
            staging: upload::UploadScratch::new(),
            arena: None,
            level_group: None,
            bricks: Vec::new(),
            planner: None,
            shadow: None,
            volume: VolumePass::new(device, &layouts),
            composite: CompositePass::new(device, &layouts),
            gtao: crate::ao::AoPipeline::new(device, &layouts),
            section: SectionPass::new(device, &layouts),
            cut: std::array::from_fn(|_| CutLayer::new(device, &layouts)),
            slice: std::array::from_fn(|_| SlicePass::new(device, &layouts)),
            meshes: MeshPasses::new(device, &layouts, budget / 2),
            markers: MarkerPasses::new(device, &layouts),
            layouts,
            dummy,
            dummy_oit,
            dummy_ao,
            atlas_sampler,
            pyramid: None,
            channels: 0,
            time: 0,
            volume_level: [None; PANE_COUNT],
            section_level: [None; PANE_COUNT],
            motion: [None; PANE_COUNT],
            section_probe: std::array::from_fn(|_| SectionProbe::default()),
            section_measure: std::array::from_fn(|_| None),
            section_sequence: 0,
            probe_mark: [0; PANE_COUNT],
            level_cap: None,
            budget,
            slot_bytes: 0,
            feedback: None,
            frames: vec![None; FEEDBACK_SLOTS],
            view_words: 0,
            view_bytes: 0,
            epoch: 0,
            content: 0,
            pin: 0,
            prefetch: false,
            prefetched: HashSet::new(),
            prefetch_key: None,
            prefetch_issued: Vec::new(),
            prefetch_rate: 1.0,
            transparent: false,
            exporting: false,
            convergence: Convergence::new(device),
            fade: None,
            object_key: None,
            settled: None,
            planned: None,
            regions: None,
            pick_target: None,
            pick_readback: None,
            pick_pending: false,
            oit_readback: std::array::from_fn(|_| {
                Readback::new(device, "grana.oit.dropped", 4, FEEDBACK_SLOTS)
            }),
            dropped: [0; PANE_COUNT],
            profile: Profiler::new(device, queue),
            frame_budget: Budget::new(FRAME_TARGET),
            measured: [0.0; STAGE_COUNT],
            sampled: false,
            info: FrameInfo::default(),
        }
    }

    pub fn profiled(&self) -> bool {
        self.profile.is_some()
    }

    pub fn format(&self) -> TextureFormat {
        self.present.format()
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    pub fn slot_bytes(&self) -> u64 {
        self.slot_bytes
    }

    pub fn slots(&self) -> usize {
        self.atlas.as_ref().map_or(0, Atlas::slots)
    }

    pub fn first_level(&self) -> usize {
        self.arena.as_ref().map_or(0, BrickArena::first)
    }

    pub fn set_budget(&mut self, budget: u64) {
        assert!(budget > 0, "a renderer needs a residency budget");
        self.budget = budget;
    }

    pub fn reserve(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        slots: usize,
        ceiling: u64,
    ) -> Result<(), String> {
        if self.slots() >= slots {
            return Ok(());
        }
        assert!(
            self.slot_bytes > 0,
            "the dataset is resident before a reserve"
        );
        let budget = crate::detail::residency_budget(
            slots,
            self.slot_bytes,
            device.limits().max_texture_dimension_3d,
            ceiling,
        )?;
        self.set_budget(budget);
        self.set_dataset(device, pyramid);
        Ok(())
    }

    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
    }

    pub fn begin_export(&mut self) {
        self.target = None;
        self.convergence.forget();
        self.settled = None;
        self.exporting = true;
        self.fade = None;
        self.level_cap = None;
        self.volume_level = [None; PANE_COUNT];
        self.section_level = [None; PANE_COUNT];
        self.section_probe = std::array::from_fn(|_| SectionProbe::default());
        self.section_measure = std::array::from_fn(|_| None);
        self.section_sequence = 0;
        self.probe_mark = [0; PANE_COUNT];
        self.planned = None;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.prefetch_rate = 1.0;
        self.sampled = false;
        self.frames.fill(None);
        self.motion = [None; PANE_COUNT];
        self.info = FrameInfo::default();
    }

    pub fn set_exporting(&mut self, exporting: bool) {
        if self.exporting != exporting {
            self.fade = None;
        }
        self.exporting = exporting;
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
        let texels = (0..channels.div_ceil(PACK))
            .map(|pack| pack_texels(pack_components(channels, pack)))
            .sum::<usize>();
        let slot_bytes = (PADDED_SIDE as u64).pow(3) * size_of::<u16>() as u64 * texels as u64;
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
        let summary = Summary::new(device, slots);
        let limit = (self.budget / RECORD_SHARE / crate::brick::RECORD_BYTES) as usize;
        let arena = BrickArena::new(device, pyramid, limit)
            .expect("the record budget holds the coarsest level");
        let level_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.level.group"),
            layout: &self.layouts.level,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: arena.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: summary.buffer().as_entire_binding(),
                },
            ],
        });
        self.view_words = planner::plan_words(slots);
        self.view_bytes = (self.view_words * size_of::<u32>()) as u64;
        self.feedback = Some(Readback::new(
            device,
            "grana.feedback",
            (PANE_COUNT * (BAND + 1) * self.view_words) as u64 * size_of::<u32>() as u64,
            FEEDBACK_SLOTS,
        ));
        self.frames = vec![None; FEEDBACK_SLOTS];
        self.bricks = (0..pyramid.level_count()).map(|_| None).collect();
        let mut planner = Planner::new(device, &self.layouts, slots);
        planner.bind_arena(device, &self.layouts, &arena);
        self.planner = Some(planner);
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
        self.summary = Some(summary);
        self.arena = Some(arena);
        self.level_group = Some(level_group);
        self.pyramid = Some(pyramid.clone());
        self.channels = channels;
        self.slot_bytes = slot_bytes;
        self.time = 0;
        self.volume_level = [None; PANE_COUNT];
        self.section_level = [None; PANE_COUNT];
        self.section_probe = std::array::from_fn(|_| SectionProbe::default());
        self.section_measure = std::array::from_fn(|_| None);
        self.section_sequence = 0;
        self.probe_mark = [0; PANE_COUNT];
        self.meshes.clear();
        for slice in &mut self.slice {
            slice.reset();
        }
        self.target = None;
        self.regions = None;
        self.convergence.forget();
        self.pick_readback = None;
        self.pick_pending = false;
        self.epoch += 1;
        self.pin = 0;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.prefetch_rate = 1.0;
        self.settled = None;
        self.planned = None;
        self.exporting = false;
        self.fade = None;
        self.motion = [None; PANE_COUNT];
        self.info = FrameInfo::default();
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
        assert!(
            frame.panes.iter().any(Option::is_some),
            "a frame draws at least one pane"
        );
        let size = frame.size;
        if self
            .target
            .as_ref()
            .is_none_or(|target| target.size != size)
        {
            self.target = Some(FrameTarget::new(device, &self.layouts, &self.present, size));
            self.convergence.forget();
            self.settled = None;
        }
        let mut target = self.target.take().expect("the frame target is ready");
        self.level_cap = Some(frame.level_cap);
        let mut info = self.render(device, queue, &mut target, frame);
        self.target = Some(target);
        if let Some(profile) = self.profile.as_mut()
            && let Some(measured) = profile.poll(device)
        {
            if self.sampled && info.settled && !self.exporting {
                self.frame_budget.observe(measured.total);
            }
            self.measured = measured.stages;
        }
        info.profile = FrameProfile {
            stages: self.measured,
            total: self.measured.iter().sum(),
            target: self.frame_budget.target(),
            penalty: self.frame_budget.penalty(),
        };
        info
    }

    pub fn request_pick(
        &mut self,
        device: &Device,
        queue: &Queue,
        pane: usize,
        point: [u32; 2],
    ) -> bool {
        assert!(pane < PANE_COUNT, "a pick names one of the panes");
        if self.pick_pending {
            return false;
        }
        let Some(regions) = self.regions else {
            return false;
        };
        let Some(placed) = regions.panes[pane] else {
            return false;
        };
        let region = placed.region;
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
            depth_stencil_attachment: match placed.kind {
                PaneKind::Volume => Some(RenderPassDepthStencilAttachment {
                    view: target.depth(),
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                PaneKind::Section => None,
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
        let group = scene.group(pane);
        match placed.kind {
            PaneKind::Volume => {
                meshes.draw_pick(pane, &mut pass);
                markers.draw_pick_volume(pane, &mut pass, group);
            }
            PaneKind::Section => {
                markers.draw_pick_section_behind(pane, &mut pass, group);
                slice[pane].draw_pick(&mut pass, group);
                markers.draw_pick_section_front(pane, &mut pass, group);
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

    pub fn convergence(&mut self, device: &Device) -> Option<(u32, f32)> {
        self.convergence.read(device)
    }

    pub fn picture(
        &mut self,
        device: &Device,
        queue: &Queue,
        exposure: f32,
        samples: u32,
        transparent: bool,
    ) -> crate::image::Image16 {
        let target = self.target.as_ref().expect("a drawn frame holds a target");
        let size = target.size;
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("grana.picture"),
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: crate::target::WIDE_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.present
            .write_wide(queue, exposure, samples, transparent);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.picture"),
        });
        self.present
            .quantize(&mut encoder, &view, &target.present_group);
        queue.submit([encoder.finish()]);
        crate::image::read16(device, queue, &texture, size)
    }
}

pub(super) fn neighbour(rendered: Option<usize>, level: usize, levels: usize) -> usize {
    match rendered {
        Some(rendered) if rendered != level => rendered,
        _ if level + 1 < levels => level + 1,
        _ if level > 0 => level - 1,
        _ => level,
    }
}

pub(super) fn ensure_brick(
    bricks: &mut [Option<BrickTable>],
    source: Level,
    level: usize,
    atlas: [u32; 3],
    arena: &BrickArena,
) {
    if bricks[level].is_none() {
        bricks[level] = Some(BrickTable::new(level, source, atlas, arena.base(level)));
    }
}

pub(super) fn dummy_texture(device: &Device, queue: &Queue) -> TextureView {
    let texture: Texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.atlas.dummy"),
        size: Extent3d {
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
