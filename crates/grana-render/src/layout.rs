use std::num::NonZeroU32;

use grana_gpu::layout::{sampler, storage, texture, uniform};
use wgpu::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, Device,
    PipelineLayout, PipelineLayoutDescriptor, ShaderStages, StorageTextureAccess, TextureFormat,
    TextureSampleType, TextureViewDimension,
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
    pub styles: BindGroupLayout,
    pub objects: BindGroupLayout,
    pub pick: BindGroupLayout,
    pub commands: BindGroupLayout,
    pub splat_cull: BindGroupLayout,
    pub splat_rows: BindGroupLayout,
    pub splat_pick_slot: BindGroupLayout,
    pub oit_write: BindGroupLayout,
    pub oit_read: BindGroupLayout,
    pub source: BindGroupLayout,
    pub occupancy: BindGroupLayout,
    pub bake: BindGroupLayout,
    pub bake_pipeline: PipelineLayout,
    pub volume: PipelineLayout,
    pub cull: PipelineLayout,
    pub plan_pipeline: PipelineLayout,
    pub mesh_cull: PipelineLayout,
    pub splat_slice: PipelineLayout,
    pub splat_oit: PipelineLayout,
    pub splat_pick: PipelineLayout,
    pub splat_cull_pipeline: PipelineLayout,
    pub section: BindGroupLayout,
    pub section_pipeline: PipelineLayout,
    pub oit_resolve: PipelineLayout,
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
                texture(5, ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE)),
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
        let source = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.source.layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: NonZeroU32::new(PACK_LIMIT as u32),
                },
                sampler(1, ShaderStages::COMPUTE),
            ],
        });
        let occupancy = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.occupancy.layout"),
            entries: &[storage(0, ShaderStages::FRAGMENT, true)],
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
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT.union(ShaderStages::COMPUTE),
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D3,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
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
        let pick = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.pick.layout"),
            entries: &[storage(0, ShaderStages::FRAGMENT, true)],
        });
        let splat_cull = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.splat.cull.layout"),
            entries: &[
                storage(0, ShaderStages::COMPUTE, true),
                storage(1, ShaderStages::COMPUTE, false),
                storage(2, ShaderStages::COMPUTE, true),
                storage(5, ShaderStages::COMPUTE, false),
            ],
        });
        let splat_rows = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.splat.rows.layout"),
            entries: &[
                storage(0, ShaderStages::VERTEX, true),
                storage(1, ShaderStages::VERTEX, true),
                storage(2, ShaderStages::VERTEX, true),
            ],
        });
        let splat_pick_slot = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.splat.pick.slot.layout"),
            entries: &[uniform(0, ShaderStages::FRAGMENT)],
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
        let oit_read = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.oit.read.layout"),
            entries: &[
                storage(0, ShaderStages::FRAGMENT, true),
                storage(1, ShaderStages::FRAGMENT, true),
                storage(2, ShaderStages::FRAGMENT, true),
                texture(3, ShaderStages::FRAGMENT),
                texture(4, ShaderStages::FRAGMENT),
            ],
        });
        let volume = pipeline_layout(
            device,
            "grana.volume.layout",
            &[&scene, &level, &atlas, &depth, &oit_read, &occupancy],
        );
        let bake = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.bake.layout"),
            entries: &[
                storage(0, ShaderStages::COMPUTE, true),
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::StorageTexture {
                        access: StorageTextureAccess::WriteOnly,
                        format: TextureFormat::Rgba16Float,
                        view_dimension: TextureViewDimension::D3,
                    },
                    count: None,
                },
            ],
        });
        let bake_pipeline = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.bake.pipeline.layout"),
            bind_group_layouts: &[
                Some(&scene),
                Some(&level),
                Some(&source),
                None,
                None,
                Some(&bake),
            ],
            immediate_size: 0,
        });
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
        let splat_slice = pipeline_layout(device, "grana.splat.layout", &[&scene, &splat_rows]);
        let splat_oit = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.splat.oit.layout"),
            bind_group_layouts: &[
                Some(&scene),
                Some(&splat_rows),
                None,
                None,
                Some(&oit_write),
            ],
            immediate_size: 0,
        });
        let splat_pick = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.splat.pick.layout"),
            bind_group_layouts: &[Some(&scene), Some(&splat_rows), Some(&splat_pick_slot)],
            immediate_size: 0,
        });
        let splat_cull_pipeline = pipeline_layout(
            device,
            "grana.splat.cull.pipeline.layout",
            &[&scene, &splat_cull],
        );
        let section = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.section.layout"),
            entries: &[storage(0, ShaderStages::VERTEX, true)],
        });
        let section_pipeline =
            pipeline_layout(device, "grana.section.pipeline.layout", &[&scene, &section]);
        let oit_resolve = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("grana.oit.resolve.layout"),
            bind_group_layouts: &[Some(&scene), None, None, None, Some(&oit_read)],
            immediate_size: 0,
        });
        Self {
            scene,
            level,
            level_write,
            plan_storage,
            plan_data,
            atlas,
            depth,
            styles,
            objects,
            pick,
            commands,
            splat_cull,
            splat_rows,
            splat_pick_slot,
            oit_write,
            oit_read,
            source,
            occupancy,
            bake,
            bake_pipeline,
            volume,
            cull,
            plan_pipeline,
            mesh_cull,
            splat_slice,
            splat_oit,
            splat_pick,
            splat_cull_pipeline,
            section,
            section_pipeline,
            oit_resolve,
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
