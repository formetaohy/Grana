use crate::{Camera, RenderParams, TransferFunction, volume_texture::VolumeTexture};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use grana_volume::{Extent, Spacing, Volume, VoxelData};
use half::f16;
use wgpu::{
    BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BindingType, Buffer, BufferBindingType, BufferUsages, Color, ColorTargetState,
    ColorWrites, CommandEncoder, Device, Extent3d, FragmentState, LoadOp, MultisampleState,
    Operations, Origin3d, PipelineLayoutDescriptor, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    Sampler, SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource,
    ShaderStages, StoreOp, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};

const LUT_SIZE: u32 = 1024;
const MAX_STEPS: f32 = 2048.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    inv_view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    box_min: [f32; 4],
    box_size: [f32; 4],
    shading: [f32; 4],
    screen: [f32; 4],
    march: [f32; 4],
    light: [f32; 4],
}

pub struct Renderer {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    uniforms: Buffer,
    volume: VolumeTexture,
    volume_sampler: Sampler,
    lut: Texture,
    lut_view: TextureView,
    lut_sampler: Sampler,
    bind_group: BindGroup,
    world_size: Vec3,
    voxel_size: Vec3,
}

impl Renderer {
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.volume.shader"),
            source: ShaderSource::Wgsl(include_str!("volume.wgsl").into()),
        });

        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.volume.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.volume.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("grana.volume.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grana.volume.uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let volume_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.volume.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        let lut = device.create_texture(&TextureDescriptor {
            label: Some("grana.volume.lut"),
            size: Extent3d {
                width: LUT_SIZE,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lut_view = lut.create_view(&TextureViewDescriptor {
            label: Some("grana.volume.lut.view"),
            ..Default::default()
        });
        let lut_sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("grana.volume.lut.sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let volume = VolumeTexture::new(
            device,
            queue,
            &Volume::new(
                Extent::new(1, 1, 1),
                Spacing::new(1.0, 1.0, 1.0),
                VoxelData::U8(vec![0]),
            ),
        );

        let bind_group = Self::create_bind_group(
            device,
            &layout,
            &uniforms,
            &volume,
            &volume_sampler,
            &lut_view,
            &lut_sampler,
        );

        Self {
            pipeline,
            layout,
            uniforms,
            volume,
            volume_sampler,
            lut,
            lut_view,
            lut_sampler,
            bind_group,
            world_size: Vec3::ONE,
            voxel_size: Vec3::ONE,
        }
    }

    pub fn set_volume(&mut self, device: &Device, queue: &Queue, volume: &Volume) {
        self.volume = VolumeTexture::new(device, queue, volume);
        self.world_size = Vec3::from(volume.world_extent());
        self.voxel_size = Vec3::new(
            volume.spacing().x(),
            volume.spacing().y(),
            volume.spacing().z(),
        );
        self.bind_group = Self::create_bind_group(
            device,
            &self.layout,
            &self.uniforms,
            &self.volume,
            &self.volume_sampler,
            &self.lut_view,
            &self.lut_sampler,
        );
    }

    pub fn set_transfer_function(&self, queue: &Queue, transfer: &TransferFunction) {
        let lut = transfer.lut(LUT_SIZE as usize);
        let mut quantized = Vec::with_capacity(lut.len() * 4);
        for [red, green, blue, opacity] in lut {
            for channel in [red, green, blue, opacity] {
                quantized.push(f16::from_f32(channel).to_bits());
            }
        }
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.lut,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(&quantized),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(LUT_SIZE * 8),
                rows_per_image: Some(1),
            },
            Extent3d {
                width: LUT_SIZE,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn draw(
        &self,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        region: [u32; 4],
        camera: &Camera,
        params: &RenderParams,
    ) {
        let [width, height] = [region[2].max(1), region[3].max(1)];
        let aspect = width as f32 / height as f32;
        let scene_radius = 0.5 * self.world_size.length();
        let view_projection = camera.view_projection(aspect, scene_radius);
        let eye = camera.eye();
        let light = camera.light_direction();

        let uniforms = Uniforms {
            inv_view_projection: view_projection.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 0.0],
            box_min: [
                -0.5 * self.world_size.x,
                -0.5 * self.world_size.y,
                -0.5 * self.world_size.z,
                0.0,
            ],
            box_size: [self.world_size.x, self.world_size.y, self.world_size.z, 0.0],
            shading: [
                params.shading.ambient,
                params.shading.diffuse,
                params.shading.specular,
                params.shading.shininess,
            ],
            screen: [
                width as f32,
                height as f32,
                (camera.fov_y() * 0.5).tan(),
                self.voxel_size.min_element(),
            ],
            march: [
                params.step_scale,
                (self.volume.level_count() - 1) as f32,
                params.opacity,
                MAX_STEPS,
            ],
            light: [light.x, light.y, light.z, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("grana.volume.pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_viewport(
            region[0] as f32,
            region[1] as f32,
            width as f32,
            height as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(region[0], region[1], width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    fn create_bind_group(
        device: &Device,
        layout: &BindGroupLayout,
        uniforms: &Buffer,
        volume: &VolumeTexture,
        volume_sampler: &Sampler,
        lut_view: &TextureView,
        lut_sampler: &Sampler,
    ) -> BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grana.volume.bind_group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(volume.view()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(volume_sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(lut_view),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: BindingResource::Sampler(lut_sampler),
                },
            ],
        })
    }
}
