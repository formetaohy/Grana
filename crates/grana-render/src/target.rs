use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, CommandEncoder, Device,
    Extent3d, LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

pub(crate) const SCENE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const ACCUMULATION_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
pub(crate) const CUT_ID_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
pub(crate) const PICK_FORMAT: TextureFormat = TextureFormat::Rg32Uint;

pub(crate) struct SceneTarget {
    color: TextureView,
    depth: TextureView,
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
                resource: wgpu::BindingResource::TextureView(&depth),
            }],
        });
        Self {
            color,
            depth,
            depth_group,
        }
    }

    pub fn color(&self) -> &TextureView {
        &self.color
    }

    pub fn depth(&self) -> &TextureView {
        &self.depth
    }

    pub fn depth_group(&self) -> &BindGroup {
        &self.depth_group
    }
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

pub(crate) fn clear(encoder: &mut CommandEncoder, color: &TextureView, depth: &TextureView) {
    let _ = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.scene.clear.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: color,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(Operations {
                load: LoadOp::Clear(1.0),
                store: StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
}
