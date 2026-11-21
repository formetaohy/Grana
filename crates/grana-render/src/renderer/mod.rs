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
use residency::{FrameTarget, LevelKey, Levels, Mode, TargetCache, ViewKey, ViewState};
use resources::Resources;
use signature::Signature;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytemuck::cast_slice;
use glam::Vec3;
use grana_display::Camera;
use grana_gpu::{Pipelines, Readback};
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
use crate::frame::{FrameData, VolumeView};
use crate::layout::Layouts;
use crate::mesh::MeshPasses;
use crate::oit::Oit;
use crate::oit_resolve::OitResolve;
use crate::overlay::Overlays;
use crate::pick::{self, PICK_ROW, PickId, PickTarget};
use crate::planner::{
    self, BAND, FitView, Ladder, Limits, Measured, Planner, Schedule, VolumeFit, bucket_scale,
    capacities, fit_measured, fit_volume, volume_dispatch,
};
use crate::present::Present;
use crate::schema::SceneFrame;
use crate::section::Sections;
use crate::shadow::{self, Shadow, ShadowGroups, ShadowSpace};
use crate::splat::SplatPasses;
use crate::target::SceneTarget;
use crate::temporal;
use crate::volume::VolumePass;
use grana_volume::{CHANNEL_LIMIT, PACK, components, pack_count, texels};

const DUMMY_VOXEL: [u16; PACK] = [0; PACK];

const FRAGMENT_BUDGET: u64 = 2 << 30;

const RECORD_SHARE: u64 = 8;

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
    pub level_cap: usize,
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
    residence: Residence,
    prefetch: bool,
    transparent: bool,
    translucent: Option<(u64, bool)>,
    exporting: bool,
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
    fragments: u64,
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
            residence: Residence::new(),
            prefetch: false,
            transparent: false,
            translucent: None,
            exporting: false,
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
            fragments: FRAGMENT_BUDGET,
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
        self.resources.as_ref().map_or(0, |held| held.slot_bytes)
    }

    pub fn generation(&self) -> u64 {
        self.residence.generation()
    }

    pub fn slots(&self) -> usize {
        self.resources.as_ref().map_or(0, Resources::slots)
    }

    pub fn first_level(&self) -> usize {
        self.resources.as_ref().map_or(0, Resources::first_level)
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
        if self.slots() < slots {
            let slot_bytes = self
                .resources
                .as_ref()
                .map(|held| held.slot_bytes)
                .filter(|bytes| *bytes > 0)
                .expect("the dataset is resident before a reserve");
            let budget = crate::detail::residency_budget(
                slots,
                slot_bytes,
                device.limits().max_texture_dimension_3d,
                ceiling,
            )?;
            self.set_budget(budget);
            self.set_dataset(device, pyramid);
        }
        self.fragments = ceiling.saturating_sub(self.budget);
        Ok(())
    }

    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
    }

    pub fn holds(&self, pyramid: &Pyramid) -> bool {
        self.resources.as_ref().is_some_and(|held| {
            let held = &held.pyramid;
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

    pub fn set_dataset(&mut self, device: &Device, pyramid: &Pyramid) {
        let resources = Resources::new(
            device,
            &self.layouts,
            &self.pipelines,
            &self.atlas_sampler,
            &self.dummy,
            self.budget,
            pyramid,
        );
        self.residence
            .setup(device, resources.slots(), pyramid.level_count());
        self.resources = Some(resources);
        self.time = 0;
        if let Some(meshes) = self.meshes.as_mut() {
            meshes.clear();
        }
        self.sections.clear();
        self.sliced = false;
        self.targets.clear();
        self.viewport = None;
        self.present_region = None;
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
            frame.exposure.is_finite() && frame.exposure > 0.0,
            "an exposure scales radiance by a positive factor"
        );
        let size = frame.size;
        let (mut target, created) = self
            .targets
            .take(device, &self.layouts, &self.present, size);
        if created {
            self.residence.forget_settled();
        }
        self.residence.set_cap(frame.level_cap);
        let info = self.render(device, queue, &mut target, frame);
        self.targets.put(target);
        info
    }

    pub fn poll_requests(&mut self, device: &Device) -> Result<Option<Requests>, String> {
        let time = self.time;
        let prefetch = self.prefetch;
        let Some(resources) = self.resources.as_mut() else {
            return Ok(None);
        };
        let requests = self.residence.poll(
            device,
            &resources.pyramid,
            &resources.planner,
            &mut resources.atlas,
            time,
            prefetch,
        )?;
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
        resources.upload(bricks, &mut self.residence, self.time);
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
