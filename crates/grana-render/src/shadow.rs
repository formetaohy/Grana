use bytemuck::{Pod, Zeroable, cast_slice};
use glam::{Mat4, Vec3};
use grana_gpu::Pipelines;
use grana_volume::{BrickGrid, Pyramid};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, Buffer, BufferDescriptor,
    BufferUsages, CommandEncoder, CompareFunction, ComputePipeline, ComputePipelineDescriptor,
    Device, Extent3d, Queue, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, Texture,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

use crate::brick::BrickTable;
use crate::layout::Layouts;

const OBJECT_SIDE: u32 = 2048;
const SIDE: u32 = 256;
const WORKGROUP: u32 = 4;
const DEPTH_BIAS: f32 = 2.0 / OBJECT_SIDE as f32;
const NORMAL_OFFSET: f32 = 2.0;

const TAU_SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/occupancy.wgsl"),
    "\n",
    include_str!("shader/tau_codec.wgsl"),
    "\n",
    include_str!("shader/optical.wgsl"),
    "\n",
    include_str!("shader/tau.wgsl")
);

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowSpace {
    view: Mat4,
    inverse: Mat4,
    depth: f32,
    width: f32,
    height: f32,
    unit: f32,
}

impl ShadowSpace {
    pub fn new(light: Vec3, low: Vec3, high: Vec3, voxel: Vec3) -> Self {
        assert!(
            light.is_finite() && light.length() > 1.0e-6,
            "a shadow space follows a finite light"
        );
        assert!(
            low.is_finite() && high.is_finite(),
            "a shadow space spans a finite box"
        );
        assert!(
            low.cmplt(high).all(),
            "a shadow space spans a positive world"
        );
        let light = light.normalize();
        let hint = if light.y.abs() > 0.9 {
            Vec3::X
        } else {
            Vec3::Y
        };
        let across = light.cross(hint).normalize();
        let upward = light.cross(across).normalize();
        let centre = 0.5 * (low + high);
        let half = 0.5 * (high - low);
        let width = 2.0 * half.dot(across.abs());
        let height = 2.0 * half.dot(upward.abs());
        let depth = 2.0 * half.dot(light.abs());
        assert!(
            depth > 1.0e-6 && width > 1.0e-6 && height > 1.0e-6,
            "a shadow space spans a positive world"
        );
        let view = Mat4::from_cols_array_2d(&[
            [across.x / width, upward.x / height, -light.x / depth, 0.0],
            [across.y / width, upward.y / height, -light.y / depth, 0.0],
            [across.z / width, upward.z / height, -light.z / depth, 0.0],
            [
                0.5 - centre.dot(across) / width,
                0.5 - centre.dot(upward) / height,
                0.5 + centre.dot(light) / depth,
                1.0,
            ],
        ]);
        let unit = voxel.min_element();
        assert!(
            unit.is_finite() && unit > 0.0,
            "a shadow space follows a voxel"
        );
        Self {
            view,
            inverse: view.inverse(),
            depth,
            width,
            height,
            unit,
        }
    }

    pub fn view(&self) -> Mat4 {
        self.view
    }

    pub fn inverse(&self) -> Mat4 {
        self.inverse
    }

    pub fn reach(&self) -> [f32; 4] {
        let texel = self.width.max(self.height) / OBJECT_SIDE as f32;
        [
            self.depth,
            DEPTH_BIAS,
            NORMAL_OFFSET * self.unit,
            texel / self.depth,
        ]
    }

    pub fn digest(&self) -> u64 {
        let mut key = 0x51_7c_c1_b7_27_22_0a_95u64;
        for column in self.view.to_cols_array() {
            key = key.rotate_left(9) ^ (column.to_bits() as u64);
        }
        key
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn depth(&self) -> f32 {
        self.depth
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ShadowGroups<'a> {
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
    pub occupancy: &'a BindGroup,
}

pub(crate) struct TauStage<'a> {
    pub encoder: &'a mut CommandEncoder,
    pub queue: &'a Queue,
    pub groups: ShadowGroups<'a>,
    pub key: u64,
    pub level: usize,
    pub table: &'a BrickTable,
    pub space: &'a ShadowSpace,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct TauHead {
    count: u32,
    full: u32,
    pad: [u32; 2],
}

const _: () = {
    assert!(size_of::<TauHead>() == 16);
};

pub fn extent(space: &ShadowSpace, voxel: f32) -> [u32; 3] {
    assert!(
        voxel.is_finite() && voxel > 0.0,
        "a shadow volume resolves one voxel"
    );
    let texels = |reach: f32| (reach / voxel).ceil().max(1.0) as u32;
    [
        texels(space.width()).min(SIDE),
        texels(space.height()).min(SIDE),
        texels(space.depth()).min(SIDE),
    ]
}

fn bound(pyramid: &Pyramid) -> u32 {
    let finest = pyramid.level(0);
    let diagonal = finest
        .world_extent()
        .iter()
        .fold(0.0f32, |held, extent| held + extent * extent)
        .sqrt();
    let voxel = finest.voxel().into_iter().fold(f32::INFINITY, f32::min);
    ((diagonal / voxel).ceil().max(1.0) as u32).min(SIDE)
}

pub(crate) fn bytes(pyramid: &Pyramid) -> u64 {
    let objects = u64::from(OBJECT_SIDE).pow(2) * size_of::<f32>() as u64;
    let regions = crate::tau::REGION_LIMIT as u64 * size_of::<[u32; 4]>() as u64;
    let tau = u64::from(bound(pyramid)).pow(3) * size_of::<u32>() as u64;
    objects + regions + size_of::<TauHead>() as u64 + tau
}

pub(crate) struct Shadow {
    objects: TextureView,
    read: BindGroup,
    tau_write: BindGroup,
    tau_head: Buffer,
    tau_regions: Buffer,
    tau_group: BindGroup,
    tau_pipeline: ComputePipeline,
    sampler: wgpu::Sampler,
    compare: wgpu::Sampler,
    dims: [u32; 3],
    bound: u32,
    bytes: u64,
    tau_key: Option<u64>,
    dirty: Vec<Vec<u32>>,
    seen: Vec<Vec<bool>>,
    whole: Vec<bool>,
    counts: Vec<usize>,
    object_key: Option<u64>,
}

impl Shadow {
    pub fn new(
        device: &Device,
        layouts: &Layouts,
        pipelines: &Pipelines,
        pyramid: &Pyramid,
    ) -> Self {
        let counts = pyramid
            .levels()
            .iter()
            .map(|level| BrickGrid::new(level.extent()).count())
            .collect::<Vec<_>>();
        let bound = bound(pyramid);
        let objects = depth(device, "grana.shadow.objects", OBJECT_SIDE);
        let sampler = linear(device);
        let compare = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.shadow.compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(CompareFunction::LessEqual),
            ..Default::default()
        });
        let dims = [1, 1, 1];
        let (read, tau_write) = groups(device, layouts, dims, &sampler, &compare, &objects);
        let tau_head = device.create_buffer(&BufferDescriptor {
            label: Some("grana.shadow.tau.head"),
            size: size_of::<TauHead>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tau_regions = device.create_buffer(&BufferDescriptor {
            label: Some("grana.shadow.tau.regions"),
            size: (crate::tau::REGION_LIMIT * size_of::<[u32; 4]>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tau_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.shadow.tau.group"),
            layout: &layouts.tau,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: tau_head.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: tau_regions.as_entire_binding(),
                },
            ],
        });
        let tau_pipeline = pipeline(
            device,
            layouts,
            TAU_SHADER,
            "tau_main",
            "grana.shadow.tau",
            pipelines,
        );
        Self {
            objects,
            read,
            tau_write,
            tau_head,
            tau_regions,
            tau_group,
            tau_pipeline,
            sampler,
            compare,
            dims,
            bound,
            bytes: bytes(pyramid),
            tau_key: None,
            dirty: counts
                .iter()
                .map(|count| Vec::with_capacity(*count))
                .collect(),
            seen: counts
                .iter()
                .map(|count| vec![false; *count])
                .collect::<Vec<_>>(),
            whole: vec![false; counts.len()],
            counts,
            object_key: None,
        }
    }

    pub fn object_key(&self) -> Option<u64> {
        self.object_key
    }

    pub fn set_object_key(&mut self, key: u64) {
        self.object_key = Some(key);
    }

    pub fn resize(&mut self, device: &Device, layouts: &Layouts, dims: [u32; 3]) {
        let dims = dims.map(|extent| extent.clamp(1, self.bound));
        if self.dims == dims {
            return;
        }
        let (read, tau_write) = groups(
            device,
            layouts,
            dims,
            &self.sampler,
            &self.compare,
            &self.objects,
        );
        self.read = read;
        self.tau_write = tau_write;
        self.dims = dims;
        self.forget();
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn group(&self) -> &BindGroup {
        &self.read
    }

    pub fn objects(&self) -> &TextureView {
        &self.objects
    }

    pub fn clear_objects(&self, encoder: &mut CommandEncoder) {
        let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("grana.shadow.objects.clear"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.objects,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
    }

    pub fn mark(&mut self, level: usize, index: usize) {
        if self.tau_key.is_none() || self.whole[level] {
            return;
        }
        let seen = &mut self.seen[level];
        if seen[index] {
            return;
        }
        seen[index] = true;
        let dirty = &mut self.dirty[level];
        dirty.push(index as u32);
        if dirty.len() >= self.counts[level] {
            self.whole[level] = true;
            self.clear(level);
        }
    }

    pub fn forget(&mut self) {
        self.tau_key = None;
        for level in 0..self.dirty.len() {
            self.whole[level] = false;
            self.clear(level);
        }
    }

    fn clear(&mut self, level: usize) {
        let (dirty, seen) = (&mut self.dirty[level], &mut self.seen[level]);
        for index in dirty.drain(..) {
            seen[index as usize] = false;
        }
    }

    pub fn pending(&self, level: usize, key: u64) -> bool {
        self.tau_key != Some(key) || self.whole[level] || !self.dirty[level].is_empty()
    }

    pub fn encode_tau(&mut self, stage: TauStage<'_>) {
        let TauStage {
            encoder,
            queue,
            groups,
            key,
            level,
            table,
            space,
        } = stage;
        let mut rects = Vec::new();
        let mut full = std::mem::take(&mut self.whole[level]);
        if self.tau_key != Some(key) {
            self.forget();
            full = true;
        } else if !full {
            let indices = std::mem::take(&mut self.dirty[level]);
            for index in &indices {
                self.seen[level][*index as usize] = false;
            }
            rects = crate::tau::regions(table, &indices, space, self.dims);
            if rects.len() > crate::tau::REGION_LIMIT {
                full = true;
            }
        }
        if !full && rects.is_empty() {
            return;
        }
        self.tau_key = Some(key);
        let head = TauHead {
            count: if full { 0 } else { rects.len() as u32 },
            full: u32::from(full),
            pad: [0; 2],
        };
        queue.write_buffer(&self.tau_head, 0, bytemuck::bytes_of(&head));
        if !full {
            queue.write_buffer(&self.tau_regions, 0, cast_slice(&rects));
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.shadow.tau.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.tau_pipeline);
        pass.set_bind_group(0, groups.scene, &[]);
        pass.set_bind_group(1, groups.level, &[]);
        pass.set_bind_group(2, groups.atlas, &[]);
        pass.set_bind_group(3, &self.tau_write, &[]);
        pass.set_bind_group(4, &self.tau_group, &[]);
        pass.set_bind_group(5, groups.occupancy, &[]);
        pass.dispatch_workgroups(
            self.dims[0].div_ceil(WORKGROUP),
            self.dims[1].div_ceil(WORKGROUP),
            1,
        );
    }
}

fn groups(
    device: &Device,
    layouts: &Layouts,
    dims: [u32; 3],
    sampler: &wgpu::Sampler,
    compare: &wgpu::Sampler,
    objects: &TextureView,
) -> (BindGroup, BindGroup) {
    let tau = volume(device, "grana.shadow.tau", dims);
    let read = device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.shadow.group"),
        layout: &layouts.shadow,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&tau),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(sampler),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::TextureView(objects),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::Sampler(compare),
            },
        ],
    });
    let tau_write = device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.shadow.tau.write"),
        layout: &layouts.shadow_write,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&tau),
        }],
    });
    (read, tau_write)
}

fn volume(device: &Device, label: &str, dims: [u32; 3]) -> TextureView {
    let texture: Texture = device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: dims[0],
            height: dims[1],
            depth_or_array_layers: dims[2],
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D3,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(wgpu::TextureViewDimension::D3),
        ..Default::default()
    })
}

fn depth(device: &Device, label: &str, side: u32) -> TextureView {
    let texture: Texture = device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        ..Default::default()
    })
}

fn linear(device: &Device) -> wgpu::Sampler {
    device.create_sampler(&SamplerDescriptor {
        label: Some("grana.shadow.sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

fn pipeline(
    device: &Device,
    layouts: &Layouts,
    source: &str,
    entry: &str,
    label: &str,
    pipelines: &Pipelines,
) -> ComputePipeline {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(source.into()),
    });
    device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&layouts.shadow_pipeline),
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: pipelines.cache(),
    })
}
