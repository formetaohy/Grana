use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, CommandEncoder, Device,
    Extent3d, LoadOp, MultisampleState, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp, TextureDescriptor, TextureDimension, TextureFormat,
    TextureUsages, TextureView, TextureViewDescriptor,
};

pub(crate) const SCENE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const ACCUMULATION_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
pub(crate) const CUT_ID_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
pub(crate) const PICK_FORMAT: TextureFormat = TextureFormat::Rg32Uint;
pub(crate) const SCENE_SAMPLES: u32 = 4;

pub(crate) fn scene_multisample() -> MultisampleState {
    MultisampleState {
        count: SCENE_SAMPLES,
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}

pub(crate) struct SceneTarget {
    color: TextureView,
    samples: TextureView,
    depth: TextureView,
    resolved: TextureView,
    depth_group: BindGroup,
}

impl SceneTarget {
    pub fn new(device: &Device, depth_layout: &BindGroupLayout, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "the scene target needs a non empty surface"
        );
        let extent = Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let samples = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.scene.samples"),
                size: extent,
                mip_level_count: 1,
                sample_count: SCENE_SAMPLES,
                dimension: TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let color = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.scene.color"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let depth = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.scene.depth"),
                size: extent,
                mip_level_count: 1,
                sample_count: SCENE_SAMPLES,
                dimension: TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let resolved = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.scene.depth.resolved"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor::default());
        let depth_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.scene.depth.group"),
            layout: depth_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&resolved),
            }],
        });
        Self {
            color,
            samples,
            depth,
            resolved,
            depth_group,
        }
    }

    pub fn color(&self) -> &TextureView {
        &self.color
    }

    pub fn samples(&self) -> &TextureView {
        &self.samples
    }

    pub fn depth(&self) -> &TextureView {
        &self.depth
    }

    pub fn resolved_depth(&self) -> &TextureView {
        &self.resolved
    }

    pub fn depth_group(&self) -> &BindGroup {
        &self.depth_group
    }
}

pub(crate) fn resolve(encoder: &mut CommandEncoder, samples: &TextureView, color: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.scene.resolve.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: samples,
            depth_slice: None,
            resolve_target: Some(color),
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Discard,
            },
        })],
        ..Default::default()
    });
}

pub(crate) fn accumulation(device: &Device, size: [u32; 2]) -> TextureView {
    device
        .create_texture(&TextureDescriptor {
            label: Some("grana.scene.accumulation"),
            size: Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: ACCUMULATION_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&TextureViewDescriptor::default())
}
