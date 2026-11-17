mod allocate;
mod damage;
mod draw;
mod plan;
mod render;
mod residency;
mod resolve;
mod signature;
mod timing;
mod upload;
mod view;

use plan::Residence;
use residency::{FrameTarget, LevelKey, Levels, Mode, TargetCache, ViewKey, ViewState};
use signature::Signature;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use bytemuck::cast_slice;
use glam::Vec3;
use grana_display::Camera;
use grana_gpu::{Pipelines, Readback};
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
use crate::converge::{Convergence, Estimate, Sampling};
use crate::frame::{FrameData, VolumeView};
use crate::layout::Layouts;
use crate::mesh::MeshPasses;
use crate::occupancy::Occupancy;
use crate::oit::Oit;
use crate::oit_resolve::OitResolve;
use crate::overlay::Overlays;
use crate::pick::{self, PICK_ROW, PickId, PickTarget};
use crate::planner::{
    self, BAND, FitView, Ladder, Limits, Planner, Schedule, VolumeFit, bucket_scale, capacities,
    fit_measured, fit_volume, volume_dispatch,
};
use crate::present::Present;
use crate::sample;
use crate::schema::SceneFrame;
use crate::section::Sections;
use crate::shadow::{self, Shadow, ShadowGroups, ShadowSpace};
use crate::splat::SplatPasses;
use crate::target::{self, SceneTarget};
use crate::temporal::{FrameView, Temporal, TemporalPass};
use crate::volume::VolumePass;
use grana_volume::CHANNEL_LIMIT;

const DUMMY_VOXEL: [u16; PACK] = [0; PACK];

const FRAGMENT_BUDGET_DIVISOR: u64 = 2;

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
    pub converged: bool,
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

pub(super) fn carried_frame(time: u32, frames: u32) -> u32 {
    (time + 1) % frames
}

#[derive(Clone, Copy)]
pub(crate) struct SliceTargets<'a> {
    pub(crate) color: &'a TextureView,
    pub(crate) depth: &'a TextureView,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Interactive,
    Playback,
    Still,
    Export,
}

impl Display {
    pub fn awaits(self) -> bool {
        self == Self::Playback
    }

    pub fn assures(self) -> bool {
        self == Self::Export
    }

    pub fn prefetches(self) -> bool {
        self == Self::Playback
    }

    pub fn resolution(self, size: [u32; 2]) -> [u32; 2] {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a display covers a positive surface"
        );
        if self == Self::Interactive {
            [(size[0].div_ceil(2)).max(1), (size[1].div_ceil(2)).max(1)]
        } else {
            size
        }
    }
}

pub struct Frame<'a, 'b> {
    pub target: &'a TextureView,
    pub size: [u32; 2],
    pub resolution: [u32; 2],
    pub camera: &'a Camera,
    pub viewport: [u32; 4],
    pub time: u32,
    pub region: Region,
    pub clip: Clip,
    pub exposure: f32,
    pub sampling: Sampling,
    pub budget: Option<Duration>,
    pub scene: &'a SceneFrame<'b>,
    pub display: Display,
    pub level_cap: usize,
    pub temporal: Option<Temporal>,
}

pub struct Renderer {
    layouts: Layouts,
    present: Present,
    targets: TargetCache,
    scene: Option<crate::table::SceneTable>,
    atlas: Option<Atlas>,
    occupancy: Option<Occupancy>,
    occupancy_group: Option<BindGroup>,
    bake: Option<Bake>,
    baked: BakeQueue,
    upload: crate::upload::Upload,
    scratch: upload::UploadScratch,
    arena: Option<BrickArena>,
    level_group: Option<BindGroup>,
    bricks: Vec<Option<BrickTable>>,
    planner: Option<Planner>,
    shadow: Option<Shadow>,
    volume: VolumePass,
    clear: crate::clear::Clear,
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
    pyramid: Option<Pyramid>,
    channels: usize,
    time: u32,
    summary: Option<signature::SceneSummary>,
    motion: Option<u64>,
    budget: u64,
    slot_bytes: u64,
    residence: Residence,
    prefetch: bool,
    transparent: bool,
    exporting: bool,
    convergence: Option<Convergence>,
    temporal: TemporalPass,
    fade: Option<Fade>,
    sections: Sections,
    sliced: bool,
    overlay: Overlays,
    viewport: Option<[u32; 4]>,
    pick_target: Option<PickTarget>,
    pick_readback: Option<Readback>,
    pick_pending: bool,
    sequence: u64,
    info: FrameInfo,
    pipelines: Pipelines,
}

impl Renderer {
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        budget: u64,
        pipelines: &Pipelines,
    ) -> Self {
        assert!(budget > 0, "a renderer needs a residency budget");
        let layouts = Layouts::new(device);
        let dummy = dummy_texture(device, queue);
        let dummy_normal = dummy_normal(device);
        let dummy_oit = Oit::new(device, &layouts, [1, 1], 1, 1);
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
        let clear = crate::clear::Clear::new(device, pipelines);
        let surface = crate::surface::Pipeline::new(device, &layouts, pipelines);
        let temporal = TemporalPass::new(device, &layouts, pipelines);
        let overlay = Overlays::new(device, pipelines);
        Self {
            present,
            targets: TargetCache::new(),
            scene: None,
            atlas: None,
            occupancy: None,
            occupancy_group: None,
            bake: None,
            baked: BakeQueue::new(),
            upload: crate::upload::Upload::new(),
            scratch: upload::UploadScratch::new(),
            arena: None,
            level_group: None,
            bricks: Vec::new(),
            planner: None,
            shadow: None,
            volume,
            clear,
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
            pyramid: None,
            channels: 0,
            time: 0,
            summary: None,
            motion: None,
            budget,
            slot_bytes: 0,
            residence: Residence::new(),
            prefetch: false,
            transparent: false,
            exporting: false,
            convergence: None,
            temporal,
            fade: None,
            sections,
            sliced: false,
            overlay,
            viewport: None,
            pick_target: None,
            pick_readback: None,
            pick_pending: false,
            sequence: 0,
            info: FrameInfo::default(),
            pipelines: pipelines.clone(),
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
        self.residence.generation()
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

    pub fn holds(&self, pyramid: &Pyramid) -> bool {
        self.pyramid.as_ref().is_some_and(|held| {
            held.levels() == pyramid.levels()
                && held.frame_count() == pyramid.frame_count()
                && held.channel_count() == pyramid.channel_count()
                && held.brick_bytes() == pyramid.brick_bytes()
        })
    }

    pub fn invalidate(&mut self, pyramid: &Pyramid) {
        assert!(
            self.holds(pyramid),
            "an invalidated dataset keeps the resident layout"
        );
        self.pyramid = Some(pyramid.clone());
        self.upload.discard();
        self.residence.invalidate();
        if let Some(atlas) = self.atlas.as_mut() {
            atlas.reset();
        }
        self.baked.forget();
        self.info = FrameInfo::default();
    }

    pub fn begin_export(&mut self) {
        self.targets.clear();
        if let Some(convergence) = self.convergence.as_mut() {
            convergence.forget();
        }
        self.residence.begin_export();
        self.exporting = true;
        self.fade = None;
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
        self.upload.discard();
        assert!(
            (1..=CHANNEL_LIMIT).contains(&channels),
            "a dataset holds one to {CHANNEL_LIMIT} channels"
        );
        assert!(
            !pyramid.levels().is_empty(),
            "a pyramid holds at least one level"
        );
        let holdable = pyramid
            .levels()
            .iter()
            .map(|level| BrickGrid::new(level.extent()).count() as u64)
            .sum::<u64>();
        let texels = (0..channels.div_ceil(PACK))
            .map(|pack| pack_texels(pack_components(channels, pack)))
            .sum::<usize>();
        let source_bytes = (PADDED_SIDE as u64).pow(3) * size_of::<u16>() as u64 * texels as u64;
        let slot_bytes = source_bytes + crate::atlas::STRUCTURE_BYTES;
        let requested = (self.budget / slot_bytes).max(1).min(holdable);
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
        let bake = Bake::new(
            device,
            &self.layouts,
            atlas.structure(),
            slots,
            &self.pipelines,
        );
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
        self.residence.setup(device, slots, pyramid.level_count());
        self.bricks = (0..pyramid.level_count()).map(|_| None).collect();
        let mut planner = Planner::new(device, &self.layouts, slots, &self.pipelines);
        planner.bind_arena(device, &self.layouts, &arena);
        self.planner = Some(planner);
        self.shadow = Some(Shadow::new(device, &self.layouts, &self.pipelines));
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
        self.summary = None;
        if let Some(meshes) = self.meshes.as_mut() {
            meshes.clear();
        }
        self.sections.clear();
        self.sliced = false;
        self.targets.clear();
        self.viewport = None;
        if let Some(convergence) = self.convergence.as_mut() {
            convergence.forget();
        }
        self.pick_readback = None;
        self.pick_pending = false;
        self.exporting = false;
        self.fade = None;
        self.motion = None;
        self.info = FrameInfo::default();
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
            frame.resolution[0] > 0 && frame.resolution[1] > 0,
            "the render surface is not empty"
        );
        assert!(
            frame.resolution[0] <= frame.size[0] && frame.resolution[1] <= frame.size[1],
            "the render surface lies inside the frame surface"
        );
        assert!(
            frame.exposure.is_finite() && frame.exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        let size = frame.resolution;
        let (mut target, created) = self
            .targets
            .take(device, &self.layouts, &self.present, size);
        if created {
            if let Some(convergence) = self.convergence.as_mut() {
                convergence.forget();
            }
            self.residence.forget_settled();
        }
        self.residence.set_cap(frame.level_cap);
        let info = self.render(device, queue, &mut target, frame);
        self.targets.put(target);
        info
    }

    pub fn poll_requests(&mut self, device: &Device) -> Option<Requests> {
        let pyramid = self.pyramid.as_ref()?;
        let planner = self.planner.as_ref()?;
        let atlas = self.atlas.as_mut()?;
        let requests =
            self.residence
                .poll(device, pyramid, planner, atlas, self.time, self.prefetch)?;
        self.info.view = requests.view;
        Some(requests)
    }

    pub fn requests_pending(&self) -> bool {
        self.residence.pending()
    }

    pub fn reserve_target(&mut self, device: &Device, size: [u32; 2]) {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a reserved target covers a positive surface"
        );
        self.targets
            .reserve(device, &self.layouts, &self.present, size);
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
            splats,
            sections,
            pick_target,
            pick_readback,
            pick_pending,
            sliced,
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
        Some(match pick::decode(&bytes) {
            pick::Raw::None => PickId::None,
            pick::Raw::Style(index) => PickId::Style(index),
            pick::Raw::Splat(index) => match self
                .splats
                .as_ref()
                .and_then(|splats| splats.resolve(index))
            {
                Some((key, level, index)) => PickId::Splat { key, level, index },
                None => PickId::None,
            },
        })
    }

    pub fn pick_pending(&self) -> bool {
        self.pick_pending
    }

    pub fn convergence(&mut self, device: &Device) -> Option<(u32, f32)> {
        let drawn = self.info.samples;
        self.convergence.as_mut().and_then(|convergence| {
            convergence
                .poll(device, drawn)
                .map(|estimate| (estimate.samples(), estimate.error()))
        })
    }

    pub fn wide_picture(
        &mut self,
        device: &Device,
        queue: &Queue,
        exposure: f32,
        transparent: bool,
    ) -> (Texture, [u32; 2]) {
        let target = self
            .targets
            .current()
            .expect("a drawn frame holds a target");
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
        self.present.quantize(
            &mut encoder,
            &view,
            &target.present_group,
            [0, 0, size[0], size[1]],
        );
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
