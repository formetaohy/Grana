mod feedback;
mod render;
mod residency;
mod signature;
mod upload;

use residency::{
    FrameTarget, LevelKey, Levels, Mode, Residency, ResidencyDemand, Resident, ViewState,
};
use signature::Signature;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::cast_slice;
use glam::Vec3;
use grana_display::Camera;
use grana_gpu::Readback;
use grana_volume::{BrickGrid, BrickRequest, Clip, Level, PADDED_SIDE, Pyramid, Region};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, CommandEncoder, Device, Extent3d, LoadOp,
    Operations, Queue, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, Sampler, SamplerDescriptor, StoreOp, TexelCopyBufferLayout,
    TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor, TextureDimension,
    TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::atlas::{Atlas, PACK, pack_components, pack_texels};
use crate::bake::{Bake, BakeQueue};
use crate::brick::{BrickArena, BrickTable};
use crate::converge::Convergence;
use crate::frame::{FrameData, VolumeView};
use crate::layout::Layouts;
use crate::marker::MarkerPasses;
use crate::mesh::MeshPasses;
use crate::occupancy::Occupancy;
use crate::oit::Oit;
use crate::oit_resolve::OitResolve;
use crate::pick::{self, PICK_ROW, PickId, PickTarget};
use crate::planner::{
    self, BAND, Demand, FitView, Ladder, Limits, Planner, Schedule, VolumeFit, bucket_scale,
    capacities, decode_plan, fit_measured, fit_volume, volume_dispatch,
};
use crate::present::Present;
use crate::sample;
use crate::schema::{SceneFrame, Versions};
use crate::section::Sections;
use crate::shadow::{self, Shadow, ShadowGroups, ShadowSpace};
use crate::target::{self, SceneTarget};
use crate::temporal::{FrameView, Temporal, TemporalPass};
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
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInfo {
    pub view: ViewInfo,
    pub samples: u32,
    pub seeded: bool,
    pub history: bool,
    pub rebuilt: bool,
    pub settled: bool,
    pub truncated: bool,
    pub fading: bool,
}

#[derive(Default)]
pub struct Requests {
    pub time: u32,
    pub volume: Vec<BrickRequest>,
    pub prefetch: Vec<BrickRequest>,
    pub view: ViewInfo,
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

pub struct Frame<'a, 'b> {
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub camera: &'a Camera,
    pub viewport: [u32; 4],
    pub time: u32,
    pub region: Region,
    pub clip: Clip,
    pub exposure: f32,
    pub samples: u32,
    pub scene: &'a SceneFrame<'b>,
    pub prefetch: bool,
    pub await_residency: bool,
    pub level_cap: usize,
    pub assured: bool,
    pub temporal: Option<Temporal>,
}

pub struct Renderer {
    layouts: Layouts,
    present: Present,
    target: Option<FrameTarget>,
    scene: Option<crate::table::SceneTable>,
    atlas: Option<Atlas>,
    occupancy: Option<Occupancy>,
    occupancy_group: Option<BindGroup>,
    bake: Option<Bake>,
    baked: BakeQueue,
    staging: upload::UploadScratch,
    arena: Option<BrickArena>,
    level_group: Option<BindGroup>,
    bricks: Vec<Option<BrickTable>>,
    planner: Option<Planner>,
    shadow: Option<Shadow>,
    volume: VolumePass,
    oit_resolve: OitResolve,
    gtao: crate::ao::AoPipeline,
    meshes: MeshPasses,
    markers: MarkerPasses,
    dummy: TextureView,
    dummy_oit: Oit,
    dummy_ao: BindGroup,
    atlas_sampler: Sampler,
    pyramid: Option<Pyramid>,
    channels: usize,
    time: u32,
    awaited: Option<u32>,
    awaiting: bool,
    volume_level: Option<usize>,
    volume_measure: Vec<Option<usize>>,
    volume_rejection: Option<(u64, usize)>,
    fit_cache: Option<(u64, VolumeFit)>,
    motion: Option<u64>,
    level_cap: Option<usize>,
    budget: u64,
    slot_bytes: u64,
    feedback: Option<Readback>,
    frames: Vec<Option<Residency>>,
    view_words: usize,
    view_bytes: u64,
    epoch: u64,
    resident: Option<Resident>,
    channels_key: Option<u64>,
    generation: u64,
    pin: u64,
    prefetch: bool,
    prefetched: HashSet<BrickRequest>,
    prefetch_key: Option<u64>,
    prefetch_issued: Vec<BrickRequest>,
    prefetch_rate: f32,
    transparent: bool,
    exporting: bool,
    convergence: Convergence,
    temporal: TemporalPass,
    fade: Option<Fade>,
    object_key: Option<u64>,
    sections: Sections,
    sliced: bool,
    settled: Option<Residency>,
    planned: Option<Residency>,
    demand: Option<ResidencyDemand>,
    viewport: Option<[u32; 4]>,
    pick_target: Option<PickTarget>,
    pick_readback: Option<Readback>,
    pick_pending: bool,
    oit_readback: Readback,
    sequence: u64,
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
        let sections = Sections::new(device, &layouts);
        Self {
            present: Present::new(device, format),
            target: None,
            scene: None,
            atlas: None,
            occupancy: None,
            occupancy_group: None,
            bake: None,
            baked: BakeQueue::new(),
            staging: upload::UploadScratch::new(),
            arena: None,
            level_group: None,
            bricks: Vec::new(),
            planner: None,
            shadow: None,
            volume: VolumePass::new(device, &layouts),
            oit_resolve: OitResolve::new(device, &layouts),
            gtao: crate::ao::AoPipeline::new(device, &layouts),
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
            awaited: None,
            awaiting: false,
            volume_level: None,
            volume_measure: Vec::new(),
            volume_rejection: None,
            fit_cache: None,
            motion: None,
            level_cap: None,
            budget,
            slot_bytes: 0,
            feedback: None,
            frames: vec![None; FEEDBACK_SLOTS],
            view_words: 0,
            view_bytes: 0,
            epoch: 0,
            resident: None,
            channels_key: None,
            generation: 0,
            pin: 0,
            prefetch: false,
            prefetched: HashSet::new(),
            prefetch_key: None,
            prefetch_issued: Vec::new(),
            prefetch_rate: 1.0,
            transparent: false,
            exporting: false,
            convergence: Convergence::new(device),
            temporal: TemporalPass::new(device, queue),
            fade: None,
            object_key: None,
            sections,
            sliced: false,
            settled: None,
            planned: None,
            demand: None,
            viewport: None,
            pick_target: None,
            pick_readback: None,
            pick_pending: false,
            oit_readback: Readback::new(device, "grana.oit.dropped", 4, FEEDBACK_SLOTS),
            sequence: 0,
            info: FrameInfo::default(),
        }
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

    pub fn generation(&self) -> u64 {
        self.generation
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
        self.resident = None;
        self.convergence.forget();
        self.settled = None;
        self.demand = None;
        self.exporting = true;
        self.fade = None;
        self.level_cap = None;
        self.volume_level = None;
        self.volume_rejection = None;
        self.planned = None;
        self.awaited = None;
        self.awaiting = false;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.prefetch_rate = 1.0;
        self.frames.fill(None);
        self.motion = None;
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
        let source_bytes = (PADDED_SIDE as u64).pow(3) * size_of::<u16>() as u64 * texels as u64;
        let slot_bytes = source_bytes + crate::atlas::STRUCTURE_BYTES;
        let requested = (self.budget / slot_bytes).max(1).min(finest);
        let atlas = Atlas::new(
            device,
            crate::atlas::AtlasBinding {
                layout: &self.layouts.atlas,
                source: &self.layouts.source,
                dummy: &self.dummy,
                sampler: &self.atlas_sampler,
            },
            channels,
            requested,
            device.limits().max_texture_dimension_3d,
        );
        let slots = atlas.slots();
        let occupancy = Occupancy::new(device, slots);
        let bake = Bake::new(device, &self.layouts, atlas.structure(), slots);
        let limit = (self.budget / RECORD_SHARE / crate::brick::RECORD_BYTES) as usize;
        let arena = BrickArena::new(device, pyramid, limit)
            .expect("the record budget holds the coarsest level");
        let level_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.level.group"),
            layout: &self.layouts.level,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: arena.buffer().as_entire_binding(),
            }],
        });
        let occupancy_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.occupancy.group"),
            layout: &self.layouts.occupancy,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: occupancy.buffer().as_entire_binding(),
            }],
        });
        self.view_words = planner::plan_words(slots);
        self.view_bytes = (self.view_words * size_of::<u32>()) as u64;
        self.feedback = Some(Readback::new(
            device,
            "grana.feedback",
            ((BAND + 1) * self.view_words) as u64 * size_of::<u32>() as u64,
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
        self.occupancy = Some(occupancy);
        self.occupancy_group = Some(occupancy_group);
        self.bake = Some(bake);
        self.baked.forget();
        self.arena = Some(arena);
        self.level_group = Some(level_group);
        self.pyramid = Some(pyramid.clone());
        self.channels = channels;
        self.slot_bytes = slot_bytes;
        self.time = 0;
        self.awaited = None;
        self.awaiting = false;
        self.volume_level = None;
        self.volume_measure = vec![None; pyramid.level_count()];
        self.volume_rejection = None;
        self.fit_cache = None;
        self.meshes.clear();
        self.sections.clear();
        self.sliced = false;
        self.target = None;
        self.viewport = None;
        self.convergence.forget();
        self.pick_readback = None;
        self.pick_pending = false;
        self.epoch += 1;
        self.resident = None;
        self.channels_key = None;
        self.pin = 0;
        self.prefetched.clear();
        self.prefetch_key = None;
        self.prefetch_issued.clear();
        self.prefetch_rate = 1.0;
        self.generation = 0;
        self.settled = None;
        self.planned = None;
        self.demand = None;
        self.exporting = false;
        self.fade = None;
        self.motion = None;
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
        let info = self.render(device, queue, &mut target, frame);
        self.target = Some(target);
        info
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
        let Self {
            scene,
            meshes,
            markers,
            sections,
            shadow,
            pick_target,
            pick_readback,
            pick_pending,
            sliced,
            ..
        } = self;
        let Some(scene) = scene.as_ref() else {
            return false;
        };
        let shadow_group = shadow
            .as_ref()
            .map(Shadow::group)
            .expect("the shadow is ready");
        let target = PickTarget::ensure(device, pick_target);
        let readback = pick_readback
            .get_or_insert_with(|| Readback::new(device, "grana.pick.readback", PICK_ROW, 1));
        let Some(slot) = readback.reserve() else {
            return false;
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.pick"),
        });
        let group = scene.group();
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
                markers.draw_pick_volume(&mut pass, group, shadow_group);
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
            meshes.draw_pick(&mut pass);
            markers.draw_pick_volume(&mut pass, group, shadow_group);
        }
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

    pub fn wide_picture(
        &mut self,
        device: &Device,
        queue: &Queue,
        exposure: f32,
        transparent: bool,
    ) -> (Texture, [u32; 2]) {
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
            usage: TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::COPY_SRC
                | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.present.write_wide(queue, exposure, transparent);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("grana.picture"),
        });
        self.present
            .quantize(&mut encoder, &view, &target.present_group);
        queue.submit([encoder.finish()]);
        (texture, size)
    }

    pub fn picture(
        &mut self,
        device: &Device,
        queue: &Queue,
        exposure: f32,
        transparent: bool,
    ) -> crate::image::Image16 {
        let (texture, size) = self.wide_picture(device, queue, exposure, transparent);
        crate::image::read16(device, queue, &texture, size)
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
