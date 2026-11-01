use std::path::{Path, PathBuf};

use glam::Vec2;
use grana_ui::{Border, Color, Corners, Fill, Glass, Painter, Rect, Shadow, Ui, Vec2 as UiVec2};
use wgpu::{
    BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingResource, BindingType, ColorTargetState, ColorWrites, CommandEncoderDescriptor, Device,
    Extent3d, FragmentState, LoadOp, Operations, PipelineLayoutDescriptor, PrimitiveState, Queue,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, RenderPipelineDescriptor,
    SamplerBindingType, SamplerDescriptor, ShaderModuleDescriptor, ShaderSource, ShaderStages,
    StoreOp, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
    TextureView, TextureViewDescriptor, TextureViewDimension, VertexState,
};

const BACKDROP: &[u8] = include_bytes!("../assets/backdrop.png");
const REFERENCE: &[u8] = include_bytes!("../assets/reference.png");
const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;
const USAGE: &str = "usage: grana-glass-demo [--out <render.png>]";

fn main() {
    let mut out = None;
    let mut rim = Glass::REFERENCE.rim;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--out" => out = arguments.next().map(PathBuf::from),
            "--rim" => {
                rim = arguments
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .expect("--rim takes a number")
            }
            _ => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
        }
    }
    let backdrop = decode(BACKDROP).expect("the bundled backdrop decodes");
    assert_eq!(
        (backdrop.width, backdrop.height),
        (WIDTH, HEIGHT),
        "the bundled backdrop matches the reference canvas"
    );
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .expect("a graphics adapter is available");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("grana.glass.demo"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
    }))
    .expect("the device is available");
    let format = TextureFormat::Rgba8Unorm;
    let mut ui = Ui::new(&device, &queue, format);
    ui.resize([WIDTH, HEIGHT]);
    upload_scene(&device, &queue, ui.scene_view(), &backdrop.pixels);
    ui.begin(UiVec2::new(WIDTH as f32, HEIGHT as f32), 1.0, 0.0);
    ui.root(|ui| {
        let bounds = Rect::from_min_size(Vec2::ZERO, UiVec2::new(WIDTH as f32, HEIGHT as f32));
        let mut painter = ui.painter();
        painter.scene(bounds, Corners::ZERO, [0.0, 0.0, 1.0, 1.0]);
        paint_reference(&mut painter, rim);
    });
    let _ = ui.end();
    let target = device.create_texture(&TextureDescriptor {
        label: Some("grana.glass.demo.target"),
        size: Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("grana.glass.demo"),
    });
    ui.paint(&mut encoder, &view);
    queue.submit([encoder.finish()]);
    let (rgba, size, _) = ui.renderer_mut().read_color();
    if let Some(path) = out {
        write_rgba(&path, size[0], size[1], &rgba);
    }
    let reference = decode(REFERENCE).expect("the bundled reference decodes");
    assert_eq!(
        (reference.width, reference.height),
        (size[0], size[1]),
        "the bundled reference matches the render size"
    );
    if !compare(&reference.pixels, &rgba, size[0], size[1]) {
        std::process::exit(1);
    }
}

fn paint_reference(painter: &mut Painter<'_>, rim: f32) {
    let capsule = Rect::from_min_size(UiVec2::new(268.0, 206.0), UiVec2::new(424.0, 188.0));
    painter.outlined(
        capsule,
        Corners::capsule(),
        Fill::Glass(Glass {
            rim,
            ..Glass::REFERENCE
        }),
        Border {
            width: 0.0,
            color: Color::TRANSPARENT,
        },
        Shadow::new(Color::rgba(0.0, 0.0, 0.0, 0.25), 40.0, 12.0),
    );
}

fn upload_scene(device: &Device, queue: &Queue, view: &TextureView, pixels: &[u8]) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("grana.glass.demo.backdrop"),
        size: Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(WIDTH * 4),
            rows_per_image: Some(HEIGHT),
        },
        Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("grana.glass.demo.sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    let pipeline = backdrop_pipeline(device);
    let view_source = texture.create_view(&TextureViewDescriptor::default());
    let layout = pipeline.get_bind_group_layout(0);
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("grana.glass.demo.group"),
        layout: &layout,
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&view_source),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::Sampler(&sampler),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("grana.glass.demo.backdrop"),
    });
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("grana.glass.demo.backdrop.pass"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(wgpu::Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..3, 0..1);
    drop(pass);
    queue.submit([encoder.finish()]);
}

fn backdrop_pipeline(device: &Device) -> RenderPipeline {
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("grana.glass.demo.backdrop.shader"),
        source: ShaderSource::Wgsl(BACKDROP_SHADER.into()),
    });
    let bindings = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: Some("grana.glass.demo.backdrop.bindings"),
        entries: &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: Some("grana.glass.demo.backdrop.layout"),
        bind_group_layouts: &[Some(&bindings)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some("grana.glass.demo.backdrop.pipeline"),
        layout: Some(&layout),
        vertex: VertexState {
            module: &shader,
            entry_point: Some("vs_fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

const BACKDROP_SHADER: &str = r"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> VertexOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let corner = corners[index];
    var out: VertexOut;
    out.position = vec4<f32>(corner, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSampleLevel(source, source_sampler, in.uv, 0.0).rgb, 1.0);
}
";

struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn decode(bytes: &[u8]) -> Option<Image> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let pixels = match info.color_type {
        png::ColorType::Rgb => buffer[..info.buffer_size()]
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::Rgba => buffer[..info.buffer_size()].to_vec(),
        _ => return None,
    };
    Some(Image {
        width: info.width,
        height: info.height,
        pixels,
    })
}

fn write_rgba(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    let file = std::fs::File::create(path).expect("the render image is writable");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("the render image header");
    writer
        .write_image_data(pixels)
        .expect("the render image data");
    writer.finish().expect("the render image finishes");
}

fn compare(reference: &[u8], pixels: &[u8], width: u32, height: u32) -> bool {
    let mut sum = [0.0f64; 3];
    let mut maximum = [0u32; 3];
    let mut within_two = 0u64;
    let mut counts = [0u64; 4];
    for index in 0..(width * height) as usize {
        let mut pixel_max = 0;
        for channel in 0..3 {
            let a = u32::from(pixels[index * 4 + channel]);
            let b = u32::from(reference[index * 4 + channel]);
            let difference = a.abs_diff(b);
            sum[channel] += f64::from(difference);
            maximum[channel] = maximum[channel].max(difference);
            pixel_max = pixel_max.max(difference);
        }
        if pixel_max <= 2 {
            within_two += 1;
        }
        match pixel_max {
            0 => counts[0] += 1,
            1..=2 => counts[1] += 1,
            3..=8 => counts[2] += 1,
            _ => counts[3] += 1,
        }
    }
    let total = f64::from(width * height);
    let mean = sum.map(|value| value / total);
    println!(
        "mean r {:.3} g {:.3} b {:.3}  max r {} g {} b {}",
        mean[0], mean[1], mean[2], maximum[0], maximum[1], maximum[2]
    );
    println!(
        "within 2: {:.2}%  exact {:.2}%  <=2 {:.2}%  3..8 {:.2}%  >8 {:.2}%",
        100.0 * within_two as f64 / total,
        100.0 * counts[0] as f64 / total,
        100.0 * counts[1] as f64 / total,
        100.0 * counts[2] as f64 / total,
        100.0 * counts[3] as f64 / total
    );
    let passes = mean.iter().all(|value| *value <= 1.0) && within_two as f64 / total >= 0.9;
    if passes {
        println!("the liquid glass material matches the reference");
    } else {
        println!("the liquid glass material deviates from the reference");
    }
    passes
}
