mod allocate;
mod draw;
mod plan;
mod render;
mod residency;
mod resolve;
mod resources;
mod signature;
mod timing;
mod view;

use plan::Residence;
use residency::{FrameTarget, Levels, TargetCache, ViewKey};
use resources::Resources;
use signature::Signature;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bytemuck::cast_slice;
use glam::Vec3;
use grana_display::Camera;
use grana_gpu::{Claim, Consumer, Ledger, Pipelines, Readback};
use grana_volume::{BrickGrid, BrickRequest, Clip, Damage, Level, PackedBrick, Pyramid, Region};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, CommandEncoder, Device, Extent3d, LoadOp,
    Operations, Queue, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, Sampler, SamplerDescriptor, StoreOp, TexelCopyBufferLayout,
    TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::atlas::Atlas;
use crate::bake::{Bake, BakeQueue};
use crate::brick::{BrickArena, BrickTable};
use crate::budget;
use crate::frame::{FrameData, VolumeView};
use crate::layout::Layouts;
use crate::mesh::MeshPasses;
use crate::oit::Oit;
use crate::oit_resolve::OitResolve;
use crate::overlay::Overlays;
use crate::pick::{self, PICK_ROW, PickId, PickTarget};
use crate::planner::{self, Planner, Schedule, TargetView, bucket_scale, volume_target};
use crate::present::Present;
use crate::reserve;
use crate::schema::SceneFrame;
use crate::section::Sections;
use crate::shadow::{self, Shadow, ShadowGroups, ShadowSpace};
use crate::splat::SplatPasses;
use crate::statistics::{Statistics, Transfer};
use crate::target::SceneTarget;
use crate::temporal;
use crate::volume::VolumePass;
use grana_volume::{CHANNEL_LIMIT, PACK};

const DUMMY_VOXEL: [u16; PACK] = [0; PACK];

const FADE_DRAWS: f32 = 10.0;

const PICK_LIFETIME: u64 = 2;

const UNIT: f32 = 65535.0;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Fade {
    pub(super) weight: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewInfo {
    pub level: usize,
    pub visible: usize,
    pub demand: usize,
    pub resident: usize,
    pub unavailable: usize,
}

impl ViewInfo {
    pub fn complete(self) -> bool {
        self.resident.saturating_add(self.unavailable) >= self.demand
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub view: ViewInfo,
    pub drawn: bool,
    pub age: u32,
    pub history: bool,
    pub rebuilt: bool,
    pub settled: bool,
    pub truncated: bool,
    pub fading: bool,
    pub converged: bool,
    pub exhausted: bool,
    pub error: f32,
}

impl FrameInfo {
    pub fn refined(self) -> bool {
        self.converged || self.exhausted
    }
}

#[derive(Default)]
pub struct Requests {
    pub time: u32,
    pub epoch: u64,
    pub volume: Vec<BrickRequest>,
    pub prefetch: Vec<BrickRequest>,
    pub unavailable: Vec<BrickRequest>,
    pub view: ViewInfo,
}

pub(super) fn carried_frame(time: u32, frames: u32) -> u32 {
    (time + 1) % frames
}

#[derive(Clone, Copy)]
enum Reset<'a> {
    Dataset,
    Residency(&'a Queue),
}

#[derive(Clone, Copy)]
pub(crate) struct SliceTargets<'a> {
    pub(crate) color: &'a TextureView,
    pub(crate) depth: &'a TextureView,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Immediate,
    Awaited,
    Assured,
}

impl Delivery {
    pub fn awaits(self) -> bool {
        self == Self::Awaited
    }

    pub fn assures(self) -> bool {
        self == Self::Assured
    }

    pub fn prefetches(self) -> bool {
        self == Self::Awaited
    }

    pub fn shows_stale(self) -> bool {
        self == Self::Immediate
    }
}

pub struct Frame<'a, 'b> {
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub camera: &'a Camera,
    pub viewport: [u32; 4],
    pub time: u32,
    pub region: Region,
    pub clip: Clip,
    pub exposure: f32,
    pub scene: &'a SceneFrame<'b>,
    pub delivery: Delivery,
}

pub struct Renderer {
    layouts: Layouts,
    present: Present,
    targets: TargetCache,
    resources: Option<Resources>,
    volume: VolumePass,
    surface: crate::surface::Pipeline,
    oit_resolve: OitResolve,
    gtao: crate::ao::AoPipeline,
    meshes: Option<MeshPasses>,
    splats: Option<SplatPasses>,
    dummy: TextureView,
    dummy_normal: TextureView,
    dummy_oit: Oit,
    dummy_ao: BindGroup,
    dummy_volume: crate::target::Unreached,
    atlas_sampler: Sampler,
    time: u32,
    motion: Option<u64>,
    budget: u64,
    residency_target: u64,
    total: u64,
    ceiling: u64,
    frames: u64,
    mesh_budget: u64,
    demand: budget::Demand,
    residence: Residence,
    prefetch: bool,
    transparent: bool,
    transparency: Option<(u64, bool)>,
    exporting: bool,
    wanted: usize,
    deficit: usize,
    export_ceiling: u64,
    seed: Vec<(u32, usize, Vec<u32>)>,
    reclaim: bool,
    degraded: Option<String>,
    temporal: temporal::Resolve,
    convergence: temporal::Convergence,
    fade: Option<Fade>,
    sections: Sections,
    sliced: bool,
    overlay: Overlays,
    viewport: Option<[u32; 4]>,
    present_region: Option<[u32; 4]>,
    pick_target: Option<PickTarget>,
    pick_readback: Option<Readback>,
    pick_pending: bool,
    pick_issued: u64,
    pick_ranges: Option<Vec<crate::splat::Range>>,
    sequence: u64,
    info: FrameInfo,
    plans: u64,
    waits: u64,
    uploads: u64,
    transfer: Transfer,
    claim: Claim,
    pipelines: Pipelines,
}

impl Renderer {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        ledger: &Arc<Ledger>,
        budget: u64,
        pipelines: &Pipelines,
    ) -> Self {
        let claim = ledger.claim(Consumer::Render, budget).unwrap_or_else(|| {
            panic!(
                "the renderer needs {:.1} MiB but the device allowance holds {:.1} MiB",
                budget as f64 / (1u64 << 20) as f64,
                ledger.total() as f64 / (1u64 << 20) as f64
            )
        });
        let layouts = Layouts::new(device);
        let dummy = dummy_texture(device, queue);
        let dummy_normal = dummy_normal(device);
        let dummy_oit = Oit::new(device, &layouts, crate::fragment::Fragment::singleton());
        let dummy_ao = crate::ao::dummy(device, queue, &layouts);
        let dummy_volume = dummy_volume(device, queue);
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
        let sections = Sections::new(device, &layouts, pipelines);
        let present = Present::new(device, format, pipelines);
        let volume = VolumePass::new(device, &layouts, pipelines);
        let surface = crate::surface::Pipeline::new(device, &layouts, pipelines);
        let temporal = temporal::Resolve::new(device, &layouts, pipelines);
        let convergence = temporal::Convergence::new(device, pipelines);
        let overlay = Overlays::new(device, pipelines);
        Self {
            present,
            targets: TargetCache::new(),
            resources: None,
            volume,
            surface,
            oit_resolve: OitResolve::new(device, &layouts, pipelines),
            gtao: crate::ao::AoPipeline::new(device, &layouts, pipelines),
            meshes: None,
            splats: None,
            layouts,
            dummy,
            dummy_normal,
            dummy_oit,
            dummy_ao,
            dummy_volume,
            atlas_sampler,
            time: 0,
            motion: None,
            budget,
            residency_target: budget,
            total: budget,
            ceiling: budget,
            frames: budget,
            mesh_budget: 0,
            demand: budget::Demand::full(),
            residence: Residence::new(),
            prefetch: false,
            transparent: false,
            transparency: None,
            exporting: false,
            wanted: 0,
            deficit: 0,
            export_ceiling: budget,
            seed: Vec::new(),
            reclaim: false,
            degraded: None,
            temporal,
            convergence,
            fade: None,
            sections,
            sliced: false,
            overlay,
            viewport: None,
            present_region: None,
            pick_target: None,
            pick_readback: None,
            pick_pending: false,
            pick_issued: 0,
            pick_ranges: None,
            sequence: 0,
            info: FrameInfo::default(),
            plans: 0,
            waits: 0,
            uploads: 0,
            transfer: Transfer::default(),
            claim,
            pipelines: pipelines.clone(),
        }
    }
    pub fn format(&self) -> TextureFormat {
        self.present.format()
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    pub fn splat_budget(&self) -> u64 {
        budget::splat_reservation(self.total, self.demand)
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn slot_bytes(&self) -> u64 {
        self.resources.as_ref().map_or(0, |held| held.slot_bytes)
    }

    pub fn generation(&self) -> u64 {
        self.residence.generation()
    }

    pub fn wanted(&self) -> usize {
        self.wanted
    }

    pub fn claimed(&self) -> u64 {
        self.claim.bytes()
    }

    pub fn slots(&self) -> usize {
        self.resources.as_ref().map_or(0, Resources::slots)
    }

    pub fn first_level(&self) -> usize {
        self.resources.as_ref().map_or(0, Resources::first_level)
    }

    pub fn capacity(
        &self,
        device: &Device,
        pyramid: &Pyramid,
        total: u64,
        detail: crate::Detail,
        frame: budget::FrameBudget,
        ceiling: u64,
    ) -> Result<usize, String> {
        assert!(total > 0, "a renderer needs a texture budget");
        let plan = reserve::Plan::of(
            pyramid,
            reserve::Allocation::Export {
                slots: detail.bricks + 1,
                ceiling,
            },
            reserve::Footprint {
                frames: budget::Frames::of(frame.size, frame.demand).bytes(),
                splats: budget::splat_reservation(total, frame.demand),
                support: self.support(),
            },
            device.limits().max_texture_dimension_3d,
        )?;
        Ok(plan.slots().capacity())
    }

    pub fn reserve(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        total: u64,
        detail: crate::Detail,
        frame: budget::FrameBudget,
        ceiling: u64,
    ) -> Result<(), String> {
        assert!(total > 0, "a renderer needs a texture budget");
        self.total = total;
        self.ceiling = total;
        self.wanted = detail.bricks + 1;
        self.export_ceiling = ceiling;
        self.deficit = 0;
        self.reclaim = false;
        self.degraded = None;
        self.demand = frame.demand;
        self.frames = budget::Frames::of(frame.size, frame.demand).bytes();
        self.install(
            device,
            pyramid,
            reserve::Allocation::Export {
                slots: self.wanted,
                ceiling,
            },
            Reset::Dataset,
        )
        .map(|_| ())
    }

    pub fn seed_hollow(&mut self, seed: Vec<(u32, usize, Vec<u32>)>) {
        self.seed = seed;
        self.apply_seed();
    }

    fn apply_seed(&mut self) {
        let Self {
            resources, seed, ..
        } = self;
        let Some(resources) = resources.as_mut() else {
            return;
        };
        for (time, level, indices) in seed {
            if *level >= resources.pyramid.level_count() {
                continue;
            }
            for index in indices {
                resources
                    .atlas
                    .assign_hollow(*time, *level, *index as usize);
            }
        }
    }

    pub fn degraded(&self) -> Option<&str> {
        self.degraded.as_deref()
    }

    fn degrade(&mut self, slot_bytes: u64) {
        if self.degraded.is_some() {
            return;
        }
        let need = self.wanted as u64 * slot_bytes;
        self.degraded = Some(format!(
            "this export shows {} bricks at its full detail and needs {:.1} MiB for them, more than the graphics device leaves for it; lower the export resolution",
            self.wanted,
            need as f64 / (1u64 << 20) as f64
        ));
    }

    fn grow(&mut self, device: &Device, queue: &Queue) {
        let Some((slots, slot_bytes, pyramid)) = self
            .resources
            .as_ref()
            .map(|held| (held.slots(), held.slot_bytes, held.pyramid.clone()))
        else {
            return;
        };
        let wanted = (slots + self.deficit.max(1)).min(self.wanted);
        if wanted <= slots {
            self.degrade(slot_bytes);
            return;
        }
        match self.install(
            device,
            &pyramid,
            reserve::Allocation::Export {
                slots: wanted,
                ceiling: self.export_ceiling,
            },
            Reset::Residency(queue),
        ) {
            Ok(installed) if installed > slots => {}
            _ => self.degrade(slot_bytes),
        }
    }

    fn install(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        allocation: reserve::Allocation,
        reset: Reset,
    ) -> Result<usize, String> {
        let max_dimension = device.limits().max_texture_dimension_3d;
        let least = reserve::least(pyramid, reserve::slot_bytes(pyramid));
        let mut allocation = allocation;
        let mut shrunk = false;
        let mut held = self.resources.take();
        let carried = held
            .as_ref()
            .filter(|resources| same_layout(&resources.pyramid, pyramid))
            .map(|resources| {
                (
                    resources.atlas.hollow_words(),
                    resources.atlas.unavailable_words(),
                )
            });
        let mut peak = held.as_ref().map_or(0, Resources::bytes);
        let outcome = loop {
            let plan = match reserve::Plan::of(pyramid, allocation, self.footprint(), max_dimension)
            {
                Ok(plan) => plan,
                Err(detail) => break Err(detail),
            };
            if !self.claim.resize(peak.saturating_add(plan.total())) {
                let Some(resident) = held.take() else {
                    break Err(unaffordable(plan.total()));
                };
                drop(resident);
                peak = 0;
                if !self.claim.resize(plan.total()) {
                    break Err(unaffordable(plan.total()));
                }
            }
            match Resources::new(
                device,
                &self.layouts,
                &self.pipelines,
                &self.atlas_sampler,
                &self.dummy,
                pyramid,
                &plan,
            ) {
                Ok(resources) => break Ok((plan, resources)),
                Err(resources::Shortfall::Memory) => {
                    let Some(next) = allocation.shrink(least) else {
                        break Err(format!(
                            "the graphics device cannot hold the smallest brick residency of {:.1} MiB",
                            least as f64 / (1u64 << 20) as f64
                        ));
                    };
                    grana_log::warn!(
                        target: "grana::render",
                        budget = plan.residency(),
                        "the graphics device is short of memory, so the brick residency shrinks"
                    );
                    shrunk = true;
                    allocation = next;
                }
                Err(resources::Shortfall::Detail(detail)) => break Err(detail),
            }
        };
        let (plan, mut resources) = match outcome {
            Ok(held) => held,
            Err(detail) => {
                self.resources = held;
                if !self.claim.resize(peak) {
                    return Err(unaffordable(peak));
                }
                return Err(detail);
            }
        };
        if let Reset::Residency(queue) = reset
            && let Some(old) = held
                .as_ref()
                .filter(|resources| same_layout(&resources.pyramid, pyramid))
        {
            resources.carry(device, queue, old);
        }
        self.residence.setup(device, resources.slots(), pyramid);
        assert_eq!(
            resources.bytes() + self.residence.bytes(),
            plan.dataset(),
            "the renderer holds exactly the dataset bytes its plan charges"
        );
        let installed = resources.slots();
        self.budget = plan.residency();
        self.residency_target = plan.target();
        self.mesh_budget = plan.surface();
        self.resources = Some(resources);
        if let Some((hollow, unavailable)) = carried {
            let resources = self.resources.as_mut().expect("the dataset is installed");
            resources.atlas.restore_hollow(hollow);
            resources.atlas.restore_unavailable(unavailable);
        }
        self.apply_seed();
        if let Some(meshes) = self.meshes.as_mut() {
            meshes.set_budget(self.mesh_budget);
        }
        drop(held);
        if !self.claim.resize(plan.total()) {
            return Err(unaffordable(plan.total()));
        }
        if shrunk {
            self.ceiling = self.ceiling.min(self.budget);
        }
        match reset {
            Reset::Dataset => {
                self.targets.clear();
                self.meshes = None;
                self.sections.clear();
            }
            Reset::Residency(_) => return Ok(installed),
        }
        self.time = 0;
        self.sliced = false;
        self.viewport = None;
        self.present_region = None;
        self.pick_readback = None;
        self.pick_pending = false;
        self.fade = None;
        self.motion = None;
        self.info = FrameInfo::default();
        Ok(installed)
    }

    fn settle(&mut self) -> Result<(), String> {
        let held = self
            .budget
            .saturating_add(self.frames)
            .saturating_add(self.splat_budget())
            .saturating_add(self.support());
        if held == self.claim.bytes() {
            return Ok(());
        }
        if !self.claim.resize(held) {
            return Err(format!(
                "the renderer holds {:.1} MiB of textures and the running analyses leave no room for it",
                held as f64 / (1u64 << 20) as f64
            ));
        }
        Ok(())
    }

    fn fit(
        &mut self,
        device: &Device,
        queue: &Queue,
        size: [u32; 2],
        demand: budget::Demand,
    ) -> Result<(), String> {
        if self.exporting {
            return Ok(());
        }
        self.demand = budget::Demand {
            splats: self.demand.splats || demand.splats,
            ..demand
        };
        let split = budget::split(
            self.total,
            self.ceiling,
            size,
            self.demand,
            self.targets.held(size),
            self.splat_budget(),
        )?;
        self.frames = split.frames;
        self.settle()?;
        if split
            .residency
            .abs_diff(self.residency_target)
            .saturating_mul(4)
            > self.total
        {
            let pyramid = self.resources.as_ref().map(|held| held.pyramid.clone());
            if let Some(pyramid) = pyramid {
                self.install(
                    device,
                    &pyramid,
                    reserve::Allocation::Display(split.residency),
                    Reset::Residency(queue),
                )?;
            }
        }
        Ok(())
    }

    fn support(&self) -> u64 {
        self.present.bytes()
            + self.overlay.bytes()
            + self.sections.bytes()
            + self.convergence.bytes()
    }

    fn footprint(&self) -> reserve::Footprint {
        reserve::Footprint {
            frames: self.frames,
            splats: self.splat_budget(),
            support: self.support(),
        }
    }

    fn fragment_allowance(&self, size: [u32; 2]) -> u64 {
        budget::fragment_allowance(self.frames, size, self.demand)
    }

    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
    }

    pub fn holds(&self, pyramid: &Pyramid) -> bool {
        self.resources
            .as_ref()
            .is_some_and(|held| same_layout(&held.pyramid, pyramid))
    }

    pub fn invalidate(&mut self, pyramid: &Pyramid) {
        assert!(
            self.holds(pyramid),
            "an invalidated dataset keeps the resident layout"
        );
        let resources = self
            .resources
            .as_mut()
            .expect("an invalidated dataset is resident");
        resources.pyramid = pyramid.clone();
        resources.invalidate();
        self.residence.invalidate();
        self.info = FrameInfo::default();
    }

    pub fn begin_export(&mut self) {
        self.targets.clear();
        self.residence.begin_export();
        self.exporting = true;
        self.fade = None;
        self.motion = None;
        self.info = FrameInfo::default();
    }

    pub fn restart(&mut self) {
        for target in self.targets.iter_mut() {
            target.layers = None;
            target.content = None;
            target.detail = None;
            target.temporal.invalidate();
            target.phase = 0;
        }
        self.convergence.invalidate();
        self.motion = None;
        self.fade = None;
        self.info = FrameInfo::default();
    }

    pub fn set_exporting(&mut self, exporting: bool) {
        if self.exporting != exporting {
            self.fade = None;
        }
        self.exporting = exporting;
    }

    pub fn set_dataset(
        &mut self,
        device: &Device,
        pyramid: &Pyramid,
        budget: u64,
        size: [u32; 2],
    ) -> Result<(), String> {
        assert!(budget > 0, "a renderer needs a texture budget");
        self.ceiling = if budget > self.total {
            budget
        } else {
            self.ceiling.min(budget)
        };
        self.total = budget;
        self.wanted = 0;
        self.deficit = 0;
        self.seed.clear();
        self.reclaim = false;
        self.exporting = false;
        self.degraded = None;
        let split = budget::split(
            self.total,
            self.ceiling,
            size,
            self.demand,
            0,
            self.splat_budget(),
        )?;
        self.frames = split.frames;
        self.install(
            device,
            pyramid,
            reserve::Allocation::Display(split.residency),
            Reset::Dataset,
        )?;
        self.settle()?;
        Ok(())
    }

    pub fn draw(
        &mut self,
        device: &Device,
        queue: &Queue,
        frame: Frame<'_, '_>,
    ) -> Result<FrameInfo, String> {
        assert!(
            frame.size[0] > 0 && frame.size[1] > 0,
            "the surface is not empty"
        );
        assert!(
            frame.exposure.is_finite() && frame.exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        let size = frame.size;
        if self.reclaim {
            self.reclaim = false;
            self.grow(device, queue);
        }
        let transparent = self.transparency(&frame);
        let shot = render::Shot::of(&frame, transparent);
        self.fit(device, queue, size, shot.demand())?;
        let (mut target, created) = self
            .targets
            .take(device, &self.layouts, &self.present, size);
        if created {
            self.residence.forget_settled();
        }
        let info = self.render(device, queue, &mut target, frame, shot);
        self.targets.put(target);
        assert!(
            self.targets.bytes() <= self.frames,
            "the frame charge covers the frame target the renderer holds"
        );
        info
    }

    pub fn poll_requests(
        &mut self,
        device: &Device,
        pending: &HashSet<BrickRequest>,
    ) -> Result<Option<Requests>, String> {
        let time = self.time;
        let prefetch = self.prefetch;
        let Some(resources) = self.resources.as_mut() else {
            return Ok(None);
        };
        let requests = self.residence.poll(plan::Poll {
            device,
            pyramid: &resources.pyramid,
            planner: &resources.planner,
            atlas: &mut resources.atlas,
            time,
            prefetch,
            pending,
        })?;
        let Some(requests) = requests else {
            return Ok(None);
        };
        self.info.view = requests.view;
        Ok(Some(requests))
    }

    pub fn requests_pending(&self) -> bool {
        self.residence.pending()
    }

    pub fn upload(&mut self, bricks: &[(BrickRequest, PackedBrick)]) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        let uploaded = resources.upload(bricks, &mut self.residence, self.time);
        self.uploads += uploaded.written as u64;
        if !uploaded.denied.is_empty() {
            self.residence.denied(&uploaded.denied);
            if self.exporting {
                self.deficit = self.deficit.max(uploaded.denied.len());
                let slot_bytes = self.resources.as_ref().map_or(0, |held| held.slot_bytes);
                if self
                    .resources
                    .as_ref()
                    .is_some_and(|held| held.slots() < self.wanted)
                {
                    self.reclaim = true;
                } else {
                    self.degrade(slot_bytes);
                }
            }
        }
        let mut unplaced = uploaded.dropped;
        unplaced.extend(uploaded.denied);
        self.residence.discard(&unplaced);
    }

    pub fn abandon(&mut self, requests: &[BrickRequest]) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.abandon(requests, &mut self.residence, self.time);
    }

    pub fn retry(&mut self, requests: &[BrickRequest]) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.forget_unavailable(requests);
        self.residence.revalidate();
    }

    pub fn retry_all(&mut self) {
        let time = self.time;
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        resources.forget_unavailable_time(time);
        self.residence.revalidate();
    }

    pub fn statistics(&self) -> Statistics {
        let evictions = self
            .resources
            .as_ref()
            .map_or(0, |resources| resources.atlas.evictions());
        let unavailable = self.info.view.unavailable as u64;
        let (prefetched, prefetch_hits) = self.residence.prefetch_counts();
        let (residency, atlas, records, surfaces) =
            self.resources
                .as_ref()
                .map_or((self.budget, 0, 0, self.mesh_budget), |resources| {
                    (
                        self.budget
                            .saturating_add(self.frames)
                            .saturating_add(self.splat_budget())
                            .saturating_add(self.support()),
                        resources.atlas.bytes(),
                        resources.arena.bytes(),
                        self.mesh_budget,
                    )
                });
        Statistics {
            plans: self.plans,
            waits: self.waits,
            uploads: self.uploads,
            evictions,
            copies: self.transfer.copies,
            staged: self.transfer.staged,
            payload: self.transfer.payload,
            prefetched,
            prefetch_hits,
            frames: self.frames,
            splats: self.splats.as_ref().map_or(0, SplatPasses::bytes),
            splat_instances: self.splats.as_ref().map_or(0, SplatPasses::instances),
            residency,
            atlas,
            records,
            surfaces,
            unavailable,
        }
    }

    pub fn damage(&mut self, damages: &[Damage]) {
        let Some(resources) = self.resources.as_mut() else {
            return;
        };
        let scopes = resources.scopes(damages);
        if scopes.is_empty() {
            return;
        }
        for target in self.targets.iter_mut() {
            target.layers = None;
            target.content = None;
            target.temporal.invalidate();
        }
        self.convergence.invalidate();
        resources.damage(&scopes, &mut self.residence, self.time);
        self.residence.bump();
    }

    pub fn request_pick(&mut self, device: &Device, queue: &Queue, point: [u32; 2]) -> bool {
        if self.pick_pending {
            return false;
        }
        let Some(region) = self.viewport else {
            return false;
        };
        let inside = point[0] >= region[0]
            && point[0] < region[0] + region[2]
            && point[1] >= region[1]
            && point[1] < region[1] + region[3];
        if !inside {
            return false;
        }
        let offset = [point[0] - region[0], point[1] - region[1]];
        let Some(resources) = self.resources.as_ref() else {
            return false;
        };
        let group = resources.scene.group();
        let Self {
            meshes,
            splats,
            sections,
            pick_target,
            pick_readback,
            pick_pending,
            pick_issued,
            pick_ranges,
            sequence,
            sliced,
            ..
        } = self;
        let target = PickTarget::ensure(device, pick_target);
        let readback = pick_readback
            .get_or_insert_with(|| Readback::new(device, "grana.pick.readback", PICK_ROW, 1));
        let Some(slot) = readback.reserve() else {
            return false;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.pick"),
        });
        if *sliced {
            {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("grana.pick.section.pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: target.view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
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
                sections.draw_pick(&mut pass, group);
            }
            {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("grana.pick.pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: target.view(),
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations {
                            load: LoadOp::Load,
                            store: StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                        view: target.depth(),
                        depth_ops: Some(Operations {
                            load: LoadOp::Clear(1.0),
                            store: StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
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
                if let Some(splats) = splats.as_ref() {
                    splats.draw_pick_slice(&mut pass, group);
                }
            }
        } else {
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
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: target.depth(),
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(1.0),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
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
            if let Some(meshes) = meshes.as_ref() {
                meshes.draw_pick(&mut pass);
            }
            if let Some(splats) = splats.as_ref() {
                splats.draw_pick(&mut pass, group);
            }
        }
        readback.stage_pixel(&mut encoder, slot, target.color(), PICK_ROW);
        queue.submit([encoder.finish()]);
        readback.register();
        *pick_ranges = splats.as_ref().map(SplatPasses::ranges);
        *pick_pending = true;
        *pick_issued = *sequence;
        true
    }

    pub fn poll_pick(&mut self, device: &Device) -> Result<Option<PickId>, String> {
        if !self.pick_pending {
            return Ok(None);
        }
        if self.sequence.wrapping_sub(self.pick_issued) > PICK_LIFETIME {
            self.pick_pending = false;
            self.pick_readback = None;
            self.pick_ranges = None;
            return Ok(Some(PickId::Expired));
        }
        let Some(readback) = self.pick_readback.as_mut() else {
            return Ok(None);
        };
        let Some(raw) = readback.take_with(device, |_, bytes| pick::decode(bytes))? else {
            return Ok(None);
        };
        self.pick_pending = false;
        Ok(Some(match raw {
            pick::Raw::None => PickId::None,
            pick::Raw::Style(index) => PickId::Style(index),
            pick::Raw::Splat(index) => match self.pick_ranges.take().as_deref() {
                Some(ranges) => match SplatPasses::resolve(ranges, index) {
                    Some((key, level, index)) => PickId::Splat { key, level, index },
                    None => PickId::None,
                },
                None => PickId::None,
            },
        }))
    }

    pub fn pick_pending(&self) -> bool {
        self.pick_pending
    }

    pub fn present_only(
        &mut self,
        device: &Device,
        queue: &Queue,
        surface: &TextureView,
        exposure: f32,
        transparent: bool,
    ) -> bool {
        assert!(
            exposure.is_finite() && exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        let Some(viewport) = self.viewport else {
            return false;
        };
        let Some(region) = self.present_region else {
            return false;
        };
        let Some(target) = self.targets.current() else {
            return false;
        };
        if target.camera.is_none() {
            return false;
        }
        self.present.write(
            queue,
            exposure,
            transparent,
            None,
            resolve::coverage(region, target.size),
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.present.only"),
        });
        self.present
            .present(&mut encoder, surface, &target.present_group, viewport);
        queue.submit([encoder.finish()]);
        true
    }

    pub fn present_picture(
        &mut self,
        device: &Device,
        queue: &Queue,
        picture: &TextureView,
        size: [u32; 2],
        exposure: f32,
        transparent: bool,
    ) {
        let target = self
            .targets
            .current()
            .expect("a drawn frame holds a target");
        assert_eq!(
            target.size, size,
            "a presented picture covers the drawn frame"
        );
        self.present.write_wide(queue, exposure, transparent);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.picture"),
        });
        self.present.quantize(
            &mut encoder,
            picture,
            &target.present_group,
            [0, 0, size[0], size[1]],
        );
        queue.submit([encoder.finish()]);
    }

    pub fn picture<R>(
        &mut self,
        device: &Device,
        queue: &Queue,
        exposure: f32,
        transparent: bool,
        read: impl FnOnce(crate::Pixels<'_>) -> R,
    ) -> Result<R, String> {
        let size = self
            .targets
            .current()
            .expect("a drawn frame holds a target")
            .size;
        let texture = crate::image::target(device, size);
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.present_picture(device, queue, &view, size, exposure, transparent);
        crate::image::read(device, queue, &texture, size, read)
    }
}

fn unaffordable(bytes: u64) -> String {
    format!(
        "the graphics device cannot hold the {:.1} MiB of textures this dataset needs together with the running analyses",
        bytes as f64 / (1u64 << 20) as f64
    )
}

fn same_layout(held: &Pyramid, pyramid: &Pyramid) -> bool {
    held.levels() == pyramid.levels()
        && held.frame_count() == pyramid.frame_count()
        && held.channel_count() == pyramid.channel_count()
        && held.brick_bytes() == pyramid.brick_bytes()
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

fn dummy_normal(device: &Device) -> TextureView {
    crate::target::attachment(
        device,
        "grana.normal.dummy",
        [1, 1],
        crate::target::NORMAL_FORMAT,
        TextureUsages::TEXTURE_BINDING,
    )
}

fn dummy_volume(device: &Device, queue: &Queue) -> crate::target::Unreached {
    crate::target::Unreached::new(device, queue)
}
