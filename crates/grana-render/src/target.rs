use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Device,
    Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
    TextureViewDescriptor,
};

pub(crate) const SCENE_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const ACCUMULATION_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(crate) const DEPTH_FORMAT: TextureFormat = TextureFormat::Depth32Float;
pub(crate) const CUT_ID_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
pub(crate) const PICK_FORMAT: TextureFormat = TextureFormat::Rg32Uint;
pub(crate) const TAIL_DISTANCE_FORMAT: TextureFormat = TextureFormat::R16Float;
pub(crate) const LAYER_COUNT: usize = 4;

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
                resource: BindingResource::TextureView(&depth),
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

pub(crate) struct LayerTargets {
    colors: Vec<TextureView>,
    depths: Vec<TextureView>,
    tail_color: TextureView,
    tail_distance: TextureView,
}

impl LayerTargets {
    pub fn new(device: &Device, size: [u32; 2], label: &'static str) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a transparent layer needs a non empty surface"
        );
        let extent = Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let colors = (0..LAYER_COUNT)
            .map(|_| {
                device
                    .create_texture(&TextureDescriptor {
                        label: Some("grana.layer.color"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: SCENE_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&TextureViewDescriptor {
                        label: Some(label),
                        ..TextureViewDescriptor::default()
                    })
            })
            .collect();
        let depths = (0..LAYER_COUNT)
            .map(|_| {
                device
                    .create_texture(&TextureDescriptor {
                        label: Some("grana.layer.depth"),
                        size: extent,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: DEPTH_FORMAT,
                        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&TextureViewDescriptor {
                        label: Some(label),
                        ..TextureViewDescriptor::default()
                    })
            })
            .collect();
        let tail_color = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.tail.color"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: SCENE_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor {
                label: Some(label),
                ..TextureViewDescriptor::default()
            });
        let tail_distance = device
            .create_texture(&TextureDescriptor {
                label: Some("grana.tail.distance"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TAIL_DISTANCE_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&TextureViewDescriptor {
                label: Some(label),
                ..TextureViewDescriptor::default()
            });
        Self {
            colors,
            depths,
            tail_color,
            tail_distance,
        }
    }

    pub fn color(&self, index: usize) -> &TextureView {
        &self.colors[index]
    }

    pub fn depth(&self, index: usize) -> &TextureView {
        &self.depths[index]
    }

    pub fn tail_color(&self) -> &TextureView {
        &self.tail_color
    }

    pub fn tail_distance(&self) -> &TextureView {
        &self.tail_distance
    }
}

pub(crate) fn layer_group(
    device: &Device,
    layout: &BindGroupLayout,
    layers: &LayerTargets,
) -> BindGroup {
    let entries = (0..LAYER_COUNT)
        .map(|index| BindGroupEntry {
            binding: index as u32,
            resource: BindingResource::TextureView(layers.color(index)),
        })
        .chain((0..LAYER_COUNT).map(|index| BindGroupEntry {
            binding: (LAYER_COUNT + index) as u32,
            resource: BindingResource::TextureView(layers.depth(index)),
        }))
        .chain([
            BindGroupEntry {
                binding: (2 * LAYER_COUNT) as u32,
                resource: BindingResource::TextureView(layers.tail_color()),
            },
            BindGroupEntry {
                binding: (2 * LAYER_COUNT + 1) as u32,
                resource: BindingResource::TextureView(layers.tail_distance()),
            },
        ])
        .collect::<Vec<_>>();
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.layer.group"),
        layout,
        entries: &entries,
    })
}

pub(crate) fn peel_group(
    device: &Device,
    layout: &BindGroupLayout,
    previous: &TextureView,
    opaque: &TextureView,
) -> BindGroup {
    device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.peel.group"),
        layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(previous),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(opaque),
            },
        ],
    })
}

pub(crate) fn depth_placeholder(device: &Device, queue: &wgpu::Queue, value: f32) -> TextureView {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.depth.placeholder"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some("grana.depth.placeholder.view"),
        ..TextureViewDescriptor::default()
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("grana.depth.placeholder.encoder"),
    });
    let _ = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("grana.depth.placeholder.pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(value),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    });
    queue.submit([encoder.finish()]);
    view
}
