use std::num::NonZeroU32;

use grana_gpu::layout::{sampler, storage, texture, uniform};
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Device,
    PipelineLayout, PipelineLayoutDescriptor, SamplerBindingType, ShaderStages,
    StorageTextureAccess, TextureFormat, TextureSampleType, TextureViewDimension,
};

use crate::atlas::PACK_LIMIT;

pub(crate) struct Layouts {
    pub scene: BindGroupLayout,
    pub level: BindGroupLayout,
    pub level_write: BindGroupLayout,
    pub plan_storage: BindGroupLayout,
    pub plan_data: BindGroupLayout,
    pub atlas: BindGroupLayout,
    pub depth: BindGroupLayout,
    pub depth_samples: BindGroupLayout,
    pub shadow: BindGroupLayout,
    pub shadow_write: BindGroupLayout,
    pub styles: BindGroupLayout,
    pub objects: BindGroupLayout,
    pub pick: BindGroupLayout,
    pub commands: BindGroupLayout,
    pub marker_cull: BindGroupLayout,
    pub marker_render: BindGroupLayout,
    pub census_level: BindGroupLayout,
    pub oit_write: BindGroupLayout,
    pub oit_coverage: BindGroupLayout,
    pub oit_read: BindGroupLayout,
    pub volume: PipelineLayout,
    pub section: PipelineLayout,
    pub cull: PipelineLayout,
    pub plan_pipeline: PipelineLayout,
    pub mesh_cull: PipelineLayout,
    pub marker_volume: PipelineLayout,
    pub marker_oit: PipelineLayout,
    pub marker_tail: PipelineLayout,
    pub marker_section: PipelineLayout,
    pub marker_cull_pipeline: PipelineLayout,
    pub census: PipelineLayout,
    pub shadow_pipeline: PipelineLayout,
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
                    count: NonZeroU32::new(PACK_LIMIT as u32),
                },
                sampler(1, ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE)),
            ],
        });
        let shadow = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.shadow.layout"),
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
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Depth,
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_write = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.shadow.write.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::StorageTexture {
                    access: StorageTextureAccess::WriteOnly,
                    format: TextureFormat::Rgba8Unorm,
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
        let depth_samples = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.depth.samples.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Depth,
                    view_dimension: TextureViewDimension::D2,
                    multisampled: true,
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
        let pick = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.pick.layout"),
            entries: &[storage(0, ShaderStages::FRAGMENT, true)],
        });
        let marker_cull = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.marker.cull.layout"),
            entries: &[
                storage(0, ShaderStages::COMPUTE, true),
                storage(1, ShaderStages::COMPUTE, false),
                storage(2, ShaderStages::COMPUTE, false),
                storage(3, ShaderStages::COMPUTE, false),
                storage(4, ShaderStages::COMPUTE, false),
                storage(5, ShaderStages::COMPUTE, false),
                storage(6, ShaderStages::COMPUTE, false),
                storage(7, ShaderStages::COMPUTE, false),
                storage(8, ShaderStages::COMPUTE, false),
            ],
        });
        let marker_render = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.marker.render.layout"),
            entries: &[
                storage(0, ShaderStages::VERTEX, true),
                storage(1, ShaderStages::VERTEX, true),
            ],
        });
        let oit_write = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.oit.write.layout"),
            entries: &[
                storage(0, ShaderStages::FRAGMENT, false),
                storage(1, ShaderStages::FRAGMENT, false),
                storage(2, ShaderStages::FRAGMENT, false),
                storage(3, ShaderStages::FRAGMENT, false),
            ],
        });
        let oit_coverage = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.oit.coverage.layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Uint,
                    view_dimension: TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let oit_read = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.oit.read.layout"),
            entries: &[
                storage(0, ShaderStages::FRAGMENT, true),
                storage(1, ShaderStages::FRAGMENT, true),
                storage(2, ShaderStages::FRAGMENT, true),
                texture(3, ShaderStages::FRAGMENT),
                texture(4, ShaderStages::FRAGMENT),
                texture(5, ShaderStages::FRAGMENT),
            ],
        });
        let volume = pipeline_layout(
            device,
            "grana.volume.layout",
            &[&scene, &level, &atlas, &depth, &shadow, &oit_read],
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
        let marker_oit = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.marker.oit.layout"),
            bind_group_layouts: &[
                Some(&scene),
                Some(&marker_render),
                None,
                None,
                Some(&oit_write),
            ],
            immediate_size: 0,
        });
        let marker_tail = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.marker.tail.layout"),
            bind_group_layouts: &[
                Some(&scene),
                Some(&marker_render),
                None,
                None,
                Some(&oit_write),
                None,
                Some(&oit_coverage),
            ],
            immediate_size: 0,
        });
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
        let shadow_pipeline = pipeline_layout(
            device,
            "grana.shadow.pipeline.layout",
            &[&scene, &level, &atlas, &shadow_write],
        );
        Self {
            scene,
            level,
            level_write,
            plan_storage,
            plan_data,
            atlas,
            depth,
            depth_samples,
            shadow,
            shadow_write,
            styles,
            objects,
            pick,
            commands,
            marker_cull,
            marker_render,
            census_level,
            oit_write,
            oit_coverage,
            oit_read,
            volume,
            section,
            cull,
            plan_pipeline,
            mesh_cull,
            marker_volume,
            marker_oit,
            marker_tail,
            marker_section,
            marker_cull_pipeline,
            census,
            shadow_pipeline,
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
