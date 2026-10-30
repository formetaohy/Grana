use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Device, Extent3d, ShaderStages,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension,
};

pub(crate) const SCENE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;

pub(crate) fn depth_layout(device: &Device) -> BindGroupLayout {
    device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("grana.scene.depth.layout"),
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
    })
}

pub(crate) struct SceneTarget {
    color: TextureView,
    depth: TextureView,
    depth_group: BindGroup,
    size: [u32; 2],
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
                resource: BindingResource::TextureView(&depth),
            }],
        });
        Self {
            color,
            depth,
            depth_group,
            size,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
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
