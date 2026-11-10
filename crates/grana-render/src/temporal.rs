use bytemuck::{Pod, Zeroable};
use grana_display::PANE_COUNT;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
    BlendOperation, BlendState, Buffer, BufferDescriptor, BufferUsages, ColorWrites,
    CommandEncoder, Device, Extent3d, FragmentState, LoadOp, Operations, PipelineLayout,
    PipelineLayoutDescriptor, PrimitiveState, Queue, RenderPassColorAttachment,
    RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor, ShaderModuleDescriptor,
    ShaderSource, ShaderStages, StoreOp, Texture, TextureDescriptor, TextureDimension,
    TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor, TextureViewDimension,
    VertexState,
};

use crate::frame::FrameData;
use crate::target::{self, ACCUMULATION_FORMAT, COUNT_FORMAT, DEPTH_FORMAT};

const CAPTURE: &str = include_str!("shader/temporal_capture.wgsl");
const SEED: &str = include_str!("shader/temporal_seed.wgsl");

const HISTORY_LIMIT: f32 = 32.0;
const MOTION_REACH: f32 = 64.0;

const ADD: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temporal {
    history: f32,
}

impl Temporal {
    pub fn new(history: f32) -> Self {
        assert!(
            history.is_finite() && history > 0.0 && history <= HISTORY_LIMIT,
            "a temporal history spans up to {HISTORY_LIMIT} samples"
        );
        Self { history }
    }

    pub fn history(self) -> f32 {
        self.history
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaneCamera {
    view_projection: [[f32; 4]; 4],
    inverse: [[f32; 4]; 4],
    eye: [f32; 4],
    lens: [f32; 4],
    projection: [f32; 4],
}

impl PaneCamera {
    fn of(data: &FrameData) -> Self {
        Self {
            view_projection: data.view_projection,
            inverse: data.inv_view_projection,
            eye: data.eye,
            lens: data.lens,
            projection: data.projection,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct FramePanes {
    pub(crate) cameras: [Option<PaneCamera>; PANE_COUNT],
    pub(crate) regions: [[u32; 4]; PANE_COUNT],
}

impl FramePanes {
    pub(crate) fn collect(
        &mut self,
        panes: &[Option<crate::PaneFrame<'_>>; PANE_COUNT],
        data: &[Option<FrameData>; PANE_COUNT],
    ) {
        for index in 0..PANE_COUNT {
            let volume = matches!(panes[index], Some(crate::PaneFrame::Volume { .. }));
            self.cameras[index] = match (volume, &data[index]) {
                (true, Some(data)) => Some(PaneCamera::of(data)),
                _ => None,
            };
            self.regions[index] = panes[index].map_or([0; 4], |pane| pane.region());
        }
    }
}

pub(crate) struct History {
    color_view: TextureView,
    geometry_view: TextureView,
    panes: [Option<PaneCamera>; PANE_COUNT],
    regions: [[u32; 4]; PANE_COUNT],
    size: [u32; 2],
    valid: bool,
}

impl History {
    fn new(device: &Device, size: [u32; 2]) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a temporal history covers a positive surface"
        );
        let color = attachment(device, "grana.temporal.color", size, ACCUMULATION_FORMAT);
        let color_view = target::view(&color);
        let geometry = attachment(device, "grana.temporal.geometry", size, ACCUMULATION_FORMAT);
        let geometry_view = target::view(&geometry);
        Self {
            color_view,
            geometry_view,
            panes: [None; PANE_COUNT],
            regions: [[0; 4]; PANE_COUNT],
            size,
            valid: false,
        }
    }

    fn invalidate(&mut self) {
        self.valid = false;
        self.panes = [None; PANE_COUNT];
        self.regions = [[0; 4]; PANE_COUNT];
    }

    pub(crate) fn ready(&self) -> bool {
        self.valid
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CaptureData {
    region: [u32; 4],
    local: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SeedData {
    current: [[f32; 4]; 4],
    current_inverse: [[f32; 4]; 4],
    previous: [[f32; 4]; 4],
    previous_inverse: [[f32; 4]; 4],
    region: [u32; 4],
    local: [u32; 4],
    flags: [f32; 4],
}

pub(crate) struct Inputs<'a> {
    pub(crate) accumulated: &'a TextureView,
    pub(crate) counts: &'a TextureView,
    pub(crate) fresh: &'a TextureView,
    pub(crate) depth: &'a TextureView,
    pub(crate) volumes: [Option<&'a TextureView>; PANE_COUNT],
    pub(crate) normals: [Option<&'a TextureView>; PANE_COUNT],
}

pub(crate) struct Reuse<'a> {
    pub(crate) history: &'a History,
    pub(crate) panes: &'a FramePanes,
    pub(crate) length: f32,
    pub(crate) tolerances: [f32; PANE_COUNT],
    pub(crate) inputs: &'a Inputs<'a>,
}

pub(crate) struct TemporalPass {
    capture: RenderPipeline,
    seed: RenderPipeline,
    capture_layout: BindGroupLayout,
    seed_layout: BindGroupLayout,
    capture_uniforms: [Buffer; PANE_COUNT],
    seed_uniforms: [Buffer; PANE_COUNT],
    dummy_float: TextureView,
}

impl TemporalPass {
    pub(crate) fn new(device: &Device, queue: &Queue) -> Self {
        let capture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.temporal.capture.layout"),
            entries: &[
                uniform(0),
                texture(1),
                texture(2),
                depth(3),
                texture(4),
                texture(5),
            ],
        });
        let seed_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("grana.temporal.seed.layout"),
            entries: &[
                uniform(0),
                texture(1),
                texture(2),
                texture(3),
                depth(4),
                texture(5),
                texture(6),
            ],
        });
        let capture_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.temporal.capture"),
            source: ShaderSource::Wgsl(CAPTURE.into()),
        });
        let seed_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.temporal.seed"),
            source: ShaderSource::Wgsl(SEED.into()),
        });
        let capture_pipeline =
            pipeline_layout(device, "grana.temporal.capture.pipeline", &capture_layout);
        let capture = pipeline(
            device,
            "grana.temporal.capture",
            &capture_shader,
            &capture_pipeline,
            [(ACCUMULATION_FORMAT, None), (ACCUMULATION_FORMAT, None)],
        );
        let seed_pipeline = pipeline_layout(device, "grana.temporal.seed.pipeline", &seed_layout);
        let seed = pipeline(
            device,
            "grana.temporal.seed",
            &seed_shader,
            &seed_pipeline,
            [(ACCUMULATION_FORMAT, Some(ADD)), (COUNT_FORMAT, Some(ADD))],
        );
        let capture_uniforms = std::array::from_fn(|_| {
            buffer(
                device,
                "grana.temporal.capture.uniforms",
                size_of::<CaptureData>(),
            )
        });
        let seed_uniforms = std::array::from_fn(|_| {
            buffer(
                device,
                "grana.temporal.seed.uniforms",
                size_of::<SeedData>(),
            )
        });
        let dummy_float = dummy(device, queue, "grana.temporal.dummy", ACCUMULATION_FORMAT);
        Self {
            capture,
            seed,
            capture_layout,
            seed_layout,
            capture_uniforms,
            seed_uniforms,
            dummy_float,
        }
    }

    pub(crate) fn ensure(&self, history: &mut Option<History>, device: &Device, size: [u32; 2]) {
        if history.as_ref().is_some_and(|history| history.size != size) {
            *history = None;
        }
        if history.is_none() {
            *history = Some(History::new(device, size));
        }
    }

    pub(crate) fn invalidate(history: &mut Option<History>) {
        if let Some(history) = history.as_mut() {
            history.invalidate();
        }
    }

    pub(crate) fn capture(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        history: &mut History,
        panes: &FramePanes,
        inputs: &Inputs<'_>,
    ) {
        for (index, region) in panes.regions.iter().enumerate() {
            if panes.cameras[index].is_none() || region[2] == 0 || region[3] == 0 {
                continue;
            }
            let region = *region;
            let data = CaptureData {
                region,
                local: [region[2], region[3], 0, 0],
            };
            queue.write_buffer(&self.capture_uniforms[index], 0, bytemuck::bytes_of(&data));
            let group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.temporal.capture.group"),
                layout: &self.capture_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: self.capture_uniforms[index].as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(inputs.accumulated),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::TextureView(inputs.counts),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: BindingResource::TextureView(inputs.depth),
                    },
                    BindGroupEntry {
                        binding: 4,
                        resource: BindingResource::TextureView(
                            inputs.volumes[index].unwrap_or(&self.dummy_float),
                        ),
                    },
                    BindGroupEntry {
                        binding: 5,
                        resource: BindingResource::TextureView(
                            inputs.normals[index].unwrap_or(&self.dummy_float),
                        ),
                    },
                ],
            });
            target_pass(
                encoder,
                "grana.temporal.capture.pass",
                &self.capture,
                &group,
                [&history.color_view, &history.geometry_view],
                region,
            );
        }
        history.panes = panes.cameras;
        history.regions = panes.regions;
        history.valid = true;
    }

    pub(crate) fn seed(
        &self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        reuse: Reuse<'_>,
    ) {
        let Reuse {
            history,
            panes,
            length,
            tolerances,
            inputs,
        } = reuse;
        for (index, region) in panes.regions.iter().enumerate() {
            let (Some(current), Some(previous)) = (panes.cameras[index], history.panes[index])
            else {
                continue;
            };
            if region[2] == 0 || region[3] == 0 {
                continue;
            }
            let tolerance = tolerances[index];
            assert!(
                tolerance.is_finite() && tolerance > 0.0,
                "a temporal pane reprojects within a positive reach"
            );
            let region = *region;
            let data = SeedData {
                current: current.view_projection,
                current_inverse: current.inverse,
                previous: previous.view_projection,
                previous_inverse: previous.inverse,
                region,
                local: [region[2], region[3], 0, 0],
                flags: [
                    length,
                    if current == previous { 1.0 } else { 0.0 },
                    tolerance,
                    MOTION_REACH,
                ],
            };
            queue.write_buffer(&self.seed_uniforms[index], 0, bytemuck::bytes_of(&data));
            let group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.temporal.seed.group"),
                layout: &self.seed_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: self.seed_uniforms[index].as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(&history.color_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::TextureView(&history.geometry_view),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: BindingResource::TextureView(inputs.fresh),
                    },
                    BindGroupEntry {
                        binding: 4,
                        resource: BindingResource::TextureView(inputs.depth),
                    },
                    BindGroupEntry {
                        binding: 5,
                        resource: BindingResource::TextureView(
                            inputs.volumes[index].unwrap_or(&self.dummy_float),
                        ),
                    },
                    BindGroupEntry {
                        binding: 6,
                        resource: BindingResource::TextureView(
                            inputs.normals[index].unwrap_or(&self.dummy_float),
                        ),
                    },
                ],
            });
            target_pass(
                encoder,
                "grana.temporal.seed.pass",
                &self.seed,
                &group,
                [inputs.accumulated, inputs.counts],
                region,
            );
        }
    }
}

pub(crate) fn tolerance(voxel: f32) -> f32 {
    assert!(
        voxel.is_finite() && voxel > 0.0,
        "a level voxel spans a positive length"
    );
    2.0 * voxel
}

fn target_pass(
    encoder: &mut CommandEncoder,
    label: &'static str,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    attachments: [&TextureView; 2],
    region: [u32; 4],
) {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[
            Some(RenderPassColorAttachment {
                view: attachments[0],
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            }),
            Some(RenderPassColorAttachment {
                view: attachments[1],
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            }),
        ],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, &[]);
    pass.set_viewport(
        region[0] as f32,
        region[1] as f32,
        region[2] as f32,
        region[3] as f32,
        0.0,
        1.0,
    );
    pass.set_scissor_rect(region[0], region[1], region[2], region[3]);
    pass.draw(0..3, 0..1);
}

fn uniform(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn texture(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Float { filterable: false },
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn depth(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::FRAGMENT,
        ty: BindingType::Texture {
            sample_type: TextureSampleType::Depth,
            view_dimension: TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn buffer(device: &Device, label: &'static str, size: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn attachment(
    device: &Device,
    label: &'static str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
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
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

fn dummy(
    device: &Device,
    queue: &Queue,
    label: &'static str,
    format: wgpu::TextureFormat,
) -> TextureView {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let data: &[u8] = match format {
        DEPTH_FORMAT => &1.0f32.to_le_bytes(),
        _ => &[0u8; 8],
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(data.len() as u32),
            rows_per_image: Some(1),
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    texture.create_view(&TextureViewDescriptor::default())
}

fn pipeline_layout(
    device: &Device,
    label: &'static str,
    layout: &BindGroupLayout,
) -> PipelineLayout {
    device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    })
}

fn pipeline(
    device: &Device,
    label: &'static str,
    shader: &wgpu::ShaderModule,
    layout: &PipelineLayout,
    targets: [(wgpu::TextureFormat, Option<BlendState>); 2],
) -> RenderPipeline {
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: targets[0].0,
                    blend: targets[0].1,
                    write_mask: ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: targets[1].0,
                    blend: targets[1].1,
                    write_mask: ColorWrites::ALL,
                }),
            ],
        }),
        multiview_mask: None,
        cache: None,
    })
}
