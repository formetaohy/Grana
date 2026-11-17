use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, Device, Extent3d, Queue,
    RenderPass, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages, TextureView, TextureViewDescriptor,
};
pub(crate) const SCENE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const WIDE_FORMAT: TextureFormat = TextureFormat::Rgba32Float;
pub(crate) const NORMAL_FORMAT: TextureFormat = TextureFormat::Rg16Float;
pub(crate) const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
pub(crate) const PICK_FORMAT: TextureFormat = TextureFormat::Rg32Uint;
pub(crate) const VOLUME_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const VOLUME_SURFACE_FORMAT: TextureFormat = TextureFormat::Rg32Float;
pub(crate) const VOLUME_NORMAL_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

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
        let color = attachment(
            device,
            "grana.scene.color",
            size,
            SCENE_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let depth = attachment(
            device,
            "grana.scene.depth",
            size,
            DEPTH_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
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

pub(crate) struct NormalTarget {
    view: TextureView,
}

impl NormalTarget {
    pub fn new(device: &Device, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a normal target covers a positive surface"
        );
        let view = attachment(
            device,
            "grana.scene.normal",
            size,
            NORMAL_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        Self { view }
    }

    pub fn view(&self) -> &TextureView {
        &self.view
    }
}

pub(crate) struct Volume {
    radiance: TextureView,
    surface: TextureView,
    normal: TextureView,
    group: BindGroup,
}

impl Volume {
    pub fn new(device: &Device, layout: &BindGroupLayout, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a volume target covers a positive surface"
        );
        let extent = size;
        let radiance = attachment(
            device,
            "grana.volume.radiance",
            extent,
            VOLUME_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let surface = attachment(
            device,
            "grana.volume.surface",
            extent,
            VOLUME_SURFACE_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let normal = attachment(
            device,
            "grana.volume.normal",
            extent,
            VOLUME_NORMAL_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.volume.group"),
            layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&radiance),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&surface),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&normal),
                },
            ],
        });
        Self {
            radiance,
            surface,
            normal,
            group,
        }
    }

    pub fn radiance(&self) -> &TextureView {
        &self.radiance
    }

    pub fn surface(&self) -> &TextureView {
        &self.surface
    }

    pub fn normal(&self) -> &TextureView {
        &self.normal
    }

    pub fn group(&self) -> &BindGroup {
        &self.group
    }
}

pub(crate) struct Unreached {
    _surface: Texture,
    _normal: Texture,
    surface: TextureView,
    normal: TextureView,
}

impl Unreached {
    pub fn new(device: &Device, queue: &Queue) -> Self {
        let surface = device.create_texture(&TextureDescriptor {
            label: Some("grana.volume.surface.unreached"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: VOLUME_SURFACE_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let normal = device.create_texture(&TextureDescriptor {
            label: Some("grana.volume.normal.unreached"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: VOLUME_NORMAL_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write(queue, &surface, &[0.0f32, 1.0], 8);
        write(queue, &normal, &[0u8; 8], 8);
        let surface_view = surface.create_view(&TextureViewDescriptor::default());
        let normal_view = normal.create_view(&TextureViewDescriptor::default());
        Self {
            _surface: surface,
            _normal: normal,
            surface: surface_view,
            normal: normal_view,
        }
    }

    pub fn surface(&self) -> &TextureView {
        &self.surface
    }

    pub fn normal(&self) -> &TextureView {
        &self.normal
    }
}

fn write(queue: &Queue, texture: &Texture, bytes: &[impl bytemuck::Pod], row: u32) {
    queue.write_texture(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(bytes),
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(row),
            rows_per_image: Some(1),
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
}

pub(crate) fn image(
    device: &Device,
    label: &'static str,
    size: [u32; 2],
    format: TextureFormat,
    usage: TextureUsages,
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

pub(crate) fn view(texture: &Texture) -> TextureView {
    texture.create_view(&TextureViewDescriptor::default())
}

pub(crate) fn attachment(
    device: &Device,
    label: &'static str,
    size: [u32; 2],
    format: TextureFormat,
    usage: TextureUsages,
) -> TextureView {
    view(&image(device, label, size, format, usage))
}

pub(crate) fn viewport(pass: &mut RenderPass<'_>, region: [u32; 4]) {
    let width = region[2].max(1);
    let height = region[3].max(1);
    pass.set_viewport(
        region[0] as f32,
        region[1] as f32,
        width as f32,
        height as f32,
        0.0,
        1.0,
    );
    pass.set_scissor_rect(region[0], region[1], width, height);
}
