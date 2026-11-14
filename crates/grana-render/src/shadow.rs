use glam::{Mat4, Vec3};
use grana_gpu::Pipelines;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindingResource, CommandEncoder,
    CompareFunction, ComputePipeline, ComputePipelineDescriptor, Device, Extent3d,
    SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};

use crate::layout::Layouts;

const OBJECT_SIDE: u32 = 2048;
const SIDE: [u32; 2] = [112, 256];
const WORKGROUP: u32 = 4;
const DEPTH_BIAS: f32 = 2.0 / OBJECT_SIDE as f32;
const NORMAL_OFFSET: f32 = 2.0;

const TAU_SHADER: &str = concat!(
    "enable wgpu_binding_array;\n",
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
    "\n",
    include_str!("shader/volume_sample.wgsl"),
    "\n",
    include_str!("shader/tau_codec.wgsl"),
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
}

#[derive(Clone, Copy)]
pub(crate) struct ShadowGroups<'a> {
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub atlas: &'a BindGroup,
}

pub fn resolution(world: Vec3, voxel: Vec3) -> [u32; 3] {
    let reach = (world.length() / voxel.min_element()).round();
    let side = reach.clamp(SIDE[0] as f32, SIDE[1] as f32) as u32;
    [side, side, side]
}

pub(crate) struct Shadow {
    objects: TextureView,
    read: BindGroup,
    tau_write: BindGroup,
    tau_pipeline: ComputePipeline,
    dims: [u32; 3],
    tau_key: Option<u64>,
}

impl Shadow {
    pub fn new(device: &Device, layouts: &Layouts, dims: [u32; 3], pipelines: &Pipelines) -> Self {
        let tau = volume(device, "grana.shadow.tau", dims);
        let objects = depth(device, "grana.shadow.objects", OBJECT_SIDE);
        let sampler = linear(device);
        let compare = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.shadow.compare"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            compare: Some(CompareFunction::LessEqual),
            ..Default::default()
        });
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
                    resource: BindingResource::Sampler(&sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&objects),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::Sampler(&compare),
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
            tau_pipeline,
            dims,
            tau_key: None,
        }
    }

    pub fn group(&self) -> &BindGroup {
        &self.read
    }

    pub fn objects(&self) -> &TextureView {
        &self.objects
    }

    pub fn encode_tau(&mut self, encoder: &mut CommandEncoder, groups: ShadowGroups<'_>, key: u64) {
        if self.tau_key == Some(key) {
            return;
        }
        self.tau_key = Some(key);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.shadow.tau.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.tau_pipeline);
        pass.set_bind_group(0, groups.scene, &[]);
        pass.set_bind_group(1, groups.level, &[]);
        pass.set_bind_group(2, groups.atlas, &[]);
        pass.set_bind_group(3, &self.tau_write, &[]);
        pass.dispatch_workgroups(
            self.dims[0].div_ceil(WORKGROUP),
            self.dims[1].div_ceil(WORKGROUP),
            1,
        );
    }
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
