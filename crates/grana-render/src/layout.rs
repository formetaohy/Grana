use std::num::NonZeroU32;

use grana_gpu::layout::{sampler, storage, texture, uniform};
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Device,
    PipelineLayout, PipelineLayoutDescriptor, ShaderStages, StorageTextureAccess, TextureFormat,
    TextureSampleType, TextureViewDimension,
};

use grana_volume::CHANNEL_LIMIT;

pub(crate) struct Layouts {
    pub scene: BindGroupLayout,
    pub level: BindGroupLayout,
    pub level_write: BindGroupLayout,
    pub plan_storage: BindGroupLayout,
    pub plan_data: BindGroupLayout,
    pub atlas: BindGroupLayout,
    pub depth: BindGroupLayout,
    pub light: BindGroupLayout,
    pub light_write: BindGroupLayout,
    pub styles: BindGroupLayout,
    pub objects: BindGroupLayout,
    pub commands: BindGroupLayout,
    pub marker_cull: BindGroupLayout,
    pub marker_render: BindGroupLayout,
    pub census_level: BindGroupLayout,
    pub volume: PipelineLayout,
    pub section: PipelineLayout,
    pub cull: PipelineLayout,
    pub plan_pipeline: PipelineLayout,
    pub mesh_cull: PipelineLayout,
    pub marker_volume: PipelineLayout,
    pub marker_section: PipelineLayout,
    pub marker_cull_pipeline: PipelineLayout,
    pub census: PipelineLayout,
    pub light_pipeline: PipelineLayout,
}

impl Layouts {
    pub fn new(device: &Device) -> Self {
        let scene = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.scene.layout"),
            entries: &[
                uniform(
                    0,
                    ShaderStages::VERTEX_FRAGMENT.union(ShaderStages::COMPUTE),
                ),
                uniform(
                    1,
                    ShaderStages::VERTEX_FRAGMENT.union(ShaderStages::COMPUTE),
                ),
                storage(2, ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE), true),
                texture(3, ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE)),
                sampler(4, ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE)),
            ],
        });
        let level = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.level.layout"),
            entries: &[storage(
                0,
                ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE),
                true,
            )],
        });
        let level_write = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.level.write.layout"),
            entries: &[
                storage(0, ShaderStages::COMPUTE, false),
                uniform(1, ShaderStages::COMPUTE),
            ],
        });
        let plan_storage = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.plan.storage.layout"),
            entries: &[storage(0, ShaderStages::COMPUTE, false)],
        });
        let plan_data = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.plan.data.layout"),
            entries: &[
                uniform(0, ShaderStages::COMPUTE),
                storage(1, ShaderStages::COMPUTE, false),
            ],
        });
        let census_level = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.census.level.layout"),
            entries: &[
                uniform(0, ShaderStages::COMPUTE),
                storage(1, ShaderStages::COMPUTE, false),
            ],
        });
        let atlas = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.atlas.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE),
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: NonZeroU32::new(CHANNEL_LIMIT as u32),
                },
                sampler(1, ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE)),
            ],
        });
        let light = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.light.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler(1, ShaderStages::FRAGMENT),
            ],
        });
        let light_write = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.light.write.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::StorageTexture {
                    access: StorageTextureAccess::WriteOnly,
                    format: TextureFormat::Rgba16Float,
                    view_dimension: TextureViewDimension::D3,
                },
                count: None,
            }],
        });
        let depth = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.depth.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Depth,
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let styles = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.styles.layout"),
            entries: &[storage(
                0,
                ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE),
                true,
            )],
        });
        let objects = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.objects.layout"),
            entries: &[storage(
                0,
                ShaderStages::VERTEX.union(ShaderStages::COMPUTE),
                true,
            )],
        });
        let commands = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.commands.layout"),
            entries: &[storage(0, ShaderStages::COMPUTE, false)],
        });
        let marker_cull = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.marker.cull.layout"),
            entries: &[
                storage(0, ShaderStages::COMPUTE, true),
                storage(1, ShaderStages::COMPUTE, false),
                storage(2, ShaderStages::COMPUTE, false),
            ],
        });
        let marker_render = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.marker.render.layout"),
            entries: &[
                storage(0, ShaderStages::VERTEX, true),
                storage(1, ShaderStages::VERTEX, true),
            ],
        });
        let volume = pipeline_layout(
            device,
            "grana.volume.layout",
            &[&scene, &level, &atlas, &depth, &light],
        );
        let section = pipeline_layout(device, "grana.section.layout", &[&scene, &level, &atlas]);
        let cull = pipeline_layout(
            device,
            "grana.cull.layout",
            &[&scene, &level_write, &plan_storage],
        );
        let plan_pipeline = pipeline_layout(device, "grana.plan.layout", &[&plan_data]);
        let mesh_cull = pipeline_layout(
            device,
            "grana.mesh.cull.layout",
            &[&scene, &objects, &styles, &commands],
        );
        let marker_volume =
            pipeline_layout(device, "grana.marker.layout", &[&scene, &marker_render]);
        let marker_section = pipeline_layout(
            device,
            "grana.marker.section.layout",
            &[&scene, &marker_render],
        );
        let marker_cull_pipeline = pipeline_layout(
            device,
            "grana.marker.cull.pipeline.layout",
            &[&scene, &marker_cull],
        );
        let census = pipeline_layout(device, "grana.census.layout", &[&scene, &census_level]);
        let light_pipeline = pipeline_layout(
            device,
            "grana.light.pipeline.layout",
            &[&scene, &level, &atlas, &light_write],
        );
        Self {
            scene,
            level,
            level_write,
            plan_storage,
            plan_data,
            atlas,
            depth,
            light,
            light_write,
            styles,
            objects,
            commands,
            marker_cull,
            marker_render,
            census_level,
            volume,
            section,
            cull,
            plan_pipeline,
            mesh_cull,
            marker_volume,
            marker_section,
            marker_cull_pipeline,
            census,
            light_pipeline,
        }
    }
}

fn pipeline_layout(device: &Device, label: &str, layouts: &[&BindGroupLayout]) -> PipelineLayout {
    device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts
            .iter()
            .map(|layout| Some(*layout))
            .collect::<Vec<_>>(),
        immediate_size: 0,
    })
}
