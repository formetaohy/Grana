use std::ops::{Bound, Range, RangeBounds};

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::scene::{Geometry, GlassId, Scene};

pub struct Frame<'a> {
    pub content: &'a wgpu::TextureView,
    pub target: &'a wgpu::TextureView,
    pub size: (u32, u32),
    pub scale: f32,
    pub content_changed: bool,
    pub copy: bool,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct Globals {
    viewport: [f32; 4],
    misc: [f32; 4],
    light: [f32; 4],
    touch: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct ShapeGpu {
    center_half: [f32; 4],
    params: [f32; 4],
    stretch: [f32; 4],
    extra: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct GroupGpu {
    bounds: [f32; 4],
    info: [u32; 4],
    geo: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct MaterialGpu {
    optics: [f32; 4],
    lighting: [f32; 4],
    edge: [f32; 4],
    shadow: [f32; 4],
    light_a: [f32; 4],
    light_b: [f32; 4],
    dark_a: [f32; 4],
    dark_b: [f32; 4],
    tint: [f32; 4],
    lens: [f32; 4],
}

type Bounds = [f32; 4];

struct Layer {
    groups: Range<u32>,
    bounds: Bounds,
    z: i32,
}

#[derive(Default)]
struct Packed {
    shapes: Vec<ShapeGpu>,
    materials: Vec<MaterialGpu>,
    groups: Vec<GroupGpu>,
    layers: Vec<Layer>,
    reach: f32,
}

struct Member {
    id: GlassId,
    geo: Geometry,
    z: i32,
    container: u64,
    order: u64,
}

fn union(a: Bounds, b: Bounds) -> Bounds {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

fn expand(b: Bounds, by: f32) -> Bounds {
    [b[0] - by, b[1] - by, b[2] + by, b[3] + by]
}

fn intersect_bounds(a: Bounds, b: Bounds) -> Bounds {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn intersects(a: Bounds, b: Bounds) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && outer[2] >= inner[2] && outer[3] >= inner[3]
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

type Layers = (Bound<i32>, Bound<i32>);

fn cluster(scene: &Scene, layers: Layers) -> Vec<Vec<Member>> {
    let mut members: Vec<Member> = scene
        .nodes
        .iter()
        .filter(|(_, n)| {
            !scene.material_of(n).is_identity()
                && n.presence.value > 1e-3
                && layers.contains(&scene.z_of(n))
        })
        .map(|(id, n)| Member {
            id,
            geo: scene.geometry(n),
            z: scene.z_of(n),
            container: n
                .glass
                .container
                .map_or(0, |c| slotmap::Key::data(&c).as_ffi()),
            order: n.order,
        })
        .collect();
    members.sort_by_key(|m| (m.z, m.container, m.order));

    let mut parent: Vec<usize> = (0..members.len()).collect();

    for (i, a) in members.iter().enumerate() {
        let Some(spacing) = scene.nodes[a.id]
            .glass
            .container
            .and_then(|c| scene.containers.get(c))
            .map(|c| c.spacing)
        else {
            continue;
        };
        for (j, b) in members.iter().enumerate().skip(i + 1) {
            if b.container != a.container {
                break;
            }
            let gap = (a.geo.center - b.geo.center).abs() - (a.geo.reach() + b.geo.reach());
            if gap.max_element() < spacing {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }

    let mut groups: Vec<Vec<Member>> = Vec::new();
    let mut slot = vec![usize::MAX; members.len()];
    for (i, m) in members.into_iter().enumerate() {
        let root = find(&mut parent, i);
        if slot[root] == usize::MAX {
            slot[root] = groups.len();
            groups.push(Vec::new());
        }
        groups[slot[root]].push(m);
    }

    groups.sort_by_key(|g| (g[0].z, g.iter().map(|m| m.order).max().unwrap_or(0)));
    groups
}

fn pack(scene: &Scene, layers: Layers, scale: f32, viewport: Vec2, out: &mut Packed) {
    out.shapes.clear();
    out.materials.clear();
    out.groups.clear();
    out.layers.clear();
    out.reach = 0.0;
    let s = scale;
    let screen = [0.0, 0.0, viewport.x, viewport.y];

    for members in cluster(scene, layers) {
        let lead = &scene.nodes[members[0].id];
        let spacing = lead
            .glass
            .container
            .and_then(|c| scene.containers.get(c))
            .map_or(0.0, |c| c.spacing);
        let presence = |m: &Member| scene.nodes[m.id].presence.value.clamp(0.0, 1.0);
        let p = members.iter().map(presence).fold(0.0, f32::max);
        let press = members
            .iter()
            .map(|m| scene.nodes[m.id].press.value)
            .fold(0.0, f32::max)
            .clamp(0.0, 1.5);

        let first = out.shapes.len() as u32;
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        let mut clip = screen;

        let mut margin: f32 = 2.0 * s;
        for member in &members {
            let g = member.geo;
            let pm = presence(member);
            let others = members
                .iter()
                .any(|o| o.id != member.id && presence(o) > 0.5);
            let shrink = (1.0 - pm) * g.half.min_element() * if others { 1.0 } else { 0.15 };
            let along = 1.0 + g.stretch;

            let resolved = scene
                .material_of(&scene.nodes[member.id])
                .resolve(g.half.min_element() * 2.0, &scene.appearance);
            let m = resolved.material;
            let depth = (m.depth * s).min(g.half.min_element() * s).max(1.0);
            out.shapes.push(ShapeGpu {
                center_half: [g.center.x * s, g.center.y * s, g.half.x * s, g.half.y * s],
                params: [g.radius * s, shrink * s, g.axis.x, g.axis.y],
                stretch: [along, 1.0 / along, g.exponent, 0.0],
                extra: [depth, 0.0, 0.0, 0.0],
            });
            out.materials.push(material_gpu(&resolved, p, press, s));
            margin = margin.max(m.shadow.offset_y.abs() * s + 1.5 * m.shadow.blur * s);
            out.reach = out
                .reach
                .max(depth * 1.2 * m.refraction + 8.0 * m.frost * 0.5 * s * p + 8.0);
            let c = g.center * s;
            let r = g.reach() * s;
            bounds = union(bounds, [c.x - r.x, c.y - r.y, c.x + r.x, c.y + r.y]);
            if let Some(region) = scene.nodes[member.id].glass.clip {
                clip = [
                    clip[0].max(region.x * s),
                    clip[1].max(region.y * s),
                    clip[2].min((region.x + region.width) * s),
                    clip[3].min((region.y + region.height) * s),
                ];
            }
        }

        let k = spacing * s;
        let bounds = clamp_bounds(
            intersect_bounds(expand(bounds, margin + k * 0.25 + 1.0), clip),
            screen,
        );
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            out.shapes.truncate(first as usize);
            out.materials.truncate(first as usize);
            continue;
        }

        let blend = out.materials[first as usize..]
            .windows(2)
            .any(|w| w[0] != w[1]);
        out.groups.push(GroupGpu {
            bounds,
            info: [first, members.len() as u32, u32::from(blend), 0],
            geo: [k, p, press, 0.0],
        });

        let index = out.groups.len() as u32 - 1;
        let z = members[0].z;
        match out.layers.last_mut() {
            Some(layer) if layer.z == z => {
                layer.groups.end = index + 1;
                layer.bounds = union(layer.bounds, bounds);
            }
            _ => out.layers.push(Layer {
                groups: index..index + 1,
                bounds,
                z,
            }),
        }
    }
}

fn material_gpu(
    resolved: &crate::material::Resolved,
    presence: f32,
    press: f32,
    scale: f32,
) -> MaterialGpu {
    let (m, p, s) = (resolved.material, presence, scale);
    let tone_a = |t: crate::Tone| [t.lift * p, t.luminance, t.luminance_mix * p, t.lighten * p];
    let tone_b = |t: crate::Tone| [t.darken_to, t.darken * p, 0.0, 0.0];
    let tint = m.tint.map_or([0.0; 4], |c| {
        let lab = c.to_oklab();

        [lab[0], lab[1], lab[2], c.a * 0.85 * p]
    });
    MaterialGpu {
        optics: [
            m.refraction * p,
            resolved.adapt,
            m.dispersion * p,
            m.frost * 0.5 * s * p,
        ],
        lighting: [
            m.splay,
            m.specular * p * (1.0 + 0.6 * press),
            m.rim * p,
            m.rim_shade * p,
        ],
        edge: [m.hairline * p, m.edge * p, resolved.contrast * p, m.dim * p],
        shadow: [
            m.shadow.offset_y * s,
            m.shadow.blur * 0.5 * s,
            m.shadow.opacity * p,
            0.0,
        ],
        light_a: tone_a(m.light_tone),
        light_b: tone_b(m.light_tone),
        dark_a: tone_a(m.dark_tone),
        dark_b: tone_b(m.dark_tone),
        tint,
        lens: [m.lens * p, 0.0, 0.0, 0.0],
    }
}

fn clamp_bounds(b: Bounds, screen: Bounds) -> Bounds {
    [
        b[0].max(screen[0]),
        b[1].max(screen[1]),
        b[2].min(screen[2]),
        b[3].min(screen[3]),
    ]
}

fn scissor(pass: &mut wgpu::RenderPass<'_>, b: Bounds, size: (u32, u32)) -> bool {
    let x0 = b[0].floor().max(0.0) as u32;
    let y0 = b[1].floor().max(0.0) as u32;
    let x1 = (b[2].ceil().max(0.0) as u32).min(size.0);
    let y1 = (b[3].ceil().max(0.0) as u32).min(size.1);
    if x1 <= x0 || y1 <= y0 {
        return false;
    }
    pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
    true
}

struct Pipelines {
    glass: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    down_first: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

struct Pyramid {
    size: (u32, u32),
    full: wgpu::TextureView,
    mips: Vec<wgpu::TextureView>,
    chain: Vec<wgpu::BindGroup>,
}

const PYRAMID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const MAX_LEVELS: u32 = 7;

impl Pyramid {
    fn levels(size: (u32, u32)) -> u32 {
        let min_side = size.0.min(size.1).max(2);
        (min_side.ilog2().saturating_sub(3)).clamp(1, MAX_LEVELS)
    }

    fn new(device: &wgpu::Device, pipelines: &Pipelines, size: (u32, u32)) -> Self {
        let levels = Self::levels(size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("grana-glass pyramid"),
            size: wgpu::Extent3d {
                width: size.0.div_ceil(2),
                height: size.1.div_ceil(2),
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PYRAMID_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mips: Vec<_> = (0..levels)
            .map(|i| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: i,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let chain = mips[..mips.len() - 1]
            .iter()
            .map(|view| down_bind_group(device, &pipelines.down, &pipelines.sampler, view))
            .collect();
        Self {
            size,
            full: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            mips,
            chain,
        }
    }

    fn build(
        &self,
        pipelines: &Pipelines,
        encoder: &mut wgpu::CommandEncoder,
        first: &wgpu::BindGroup,
        region: Bounds,
    ) {
        for (i, mip) in self.mips.iter().enumerate() {
            let f = (1u32 << (i + 1)) as f32;
            let mip_size = (
                (self.size.0.div_ceil(2) >> i).max(1),
                (self.size.1.div_ceil(2) >> i).max(1),
            );
            let b = expand(
                [region[0] / f, region[1] / f, region[2] / f, region[3] / f],
                2.0,
            );
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("grana-glass downsample"),
                color_attachments: &[Some(attachment(mip))],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !scissor(&mut pass, b, mip_size) {
                continue;
            }
            if i == 0 {
                pass.set_pipeline(&pipelines.down_first);
                pass.set_bind_group(0, first, &[]);
            } else {
                pass.set_pipeline(&pipelines.down);
                pass.set_bind_group(0, &self.chain[i - 1], &[]);
            }
            pass.draw(0..3, 0..1);
        }
    }
}

fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
        },
    }
}

fn down_bind_group(
    device: &wgpu::Device,
    pipeline: &wgpu::RenderPipeline,
    sampler: &wgpu::Sampler,
    src: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grana-glass downsample"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(src),
            },
        ],
    })
}

fn blit_bind_group(
    device: &wgpu::Device,
    pipelines: &Pipelines,
    src: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grana-glass blit"),
        layout: &pipelines.blit.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(src),
        }],
    })
}

fn blit(
    pipelines: &Pipelines,
    encoder: &mut wgpu::CommandEncoder,
    group: &wgpu::BindGroup,
    dst: &wgpu::TextureView,
    region: Option<Bounds>,
    size: (u32, u32),
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("grana-glass blit"),
        color_attachments: &[Some(attachment(dst))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(region) = region
        && !scissor(&mut pass, region, size)
    {
        return;
    }
    pass.set_pipeline(&pipelines.blit);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..3, 0..1);
}

struct BindCache<K> {
    entries: Vec<(K, wgpu::BindGroup)>,
}

impl<K: PartialEq> BindCache<K> {
    const CAPACITY: usize = 8;

    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get(&mut self, key: K, make: impl FnOnce(&K) -> wgpu::BindGroup) -> wgpu::BindGroup {
        if let Some((_, group)) = self.entries.iter().find(|(k, _)| *k == key) {
            return group.clone();
        }
        if self.entries.len() >= Self::CAPACITY {
            self.entries.remove(0);
        }
        let group = make(&key);
        self.entries.push((key, group.clone()));
        group
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

struct Binds {
    blit: BindCache<wgpu::TextureView>,
    down_first: BindCache<wgpu::TextureView>,
    glass: BindCache<(wgpu::TextureView, bool)>,
}

struct Backdrop {
    source: wgpu::TextureView,
    pyramid: Pyramid,
    bounds: Bounds,
}

const BACKDROPS: usize = 4;

pub struct Renderer {
    format: wgpu::TextureFormat,
    size: (u32, u32),
    pipelines: Pipelines,
    globals: wgpu::Buffer,
    shapes: wgpu::Buffer,
    materials: wgpu::Buffer,
    groups: wgpu::Buffer,
    backdrops: Vec<Backdrop>,
    layered: Option<Pyramid>,
    layer_textures: Vec<wgpu::TextureView>,
    packed: Packed,
    uploaded: Uploaded,
    binds: Binds,
    rebuild: Vec<bool>,
}

#[derive(Default)]
struct Uploaded {
    globals: Option<Globals>,
    shapes: Vec<ShapeGpu>,
    materials: Vec<MaterialGpu>,
    groups: Vec<GroupGpu>,
}

fn storage(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    vs: &str,
    fs: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    topology: wgpu::PrimitiveTopology,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: None,
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let glass = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grana-glass glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/glass.wgsl").into()),
        });
        let pyramid = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grana-glass pyramid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/pyramid.wgsl").into()),
        });
        let strip = wgpu::PrimitiveTopology::TriangleStrip;
        let list = wgpu::PrimitiveTopology::TriangleList;
        let pipelines = Pipelines {
            glass: pipeline(
                device,
                &glass,
                "vs_glass",
                "fs_glass",
                format,
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                strip,
            ),
            blit: pipeline(device, &pyramid, "vs_full", "fs_blit", format, None, list),
            down_first: pipeline(
                device,
                &pyramid,
                "vs_full",
                "fs_down_first",
                PYRAMID_FORMAT,
                None,
                list,
            ),
            down: pipeline(
                device,
                &pyramid,
                "vs_full",
                "fs_down",
                PYRAMID_FORMAT,
                None,
                list,
            ),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("grana-glass linear clamp"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        };
        Self {
            format,
            size: (0, 0),
            globals: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("grana-glass globals"),
                size: size_of::<Globals>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            shapes: storage(
                device,
                "grana-glass shapes",
                size_of::<ShapeGpu>() as u64 * 16,
            ),
            materials: storage(
                device,
                "grana-glass materials",
                size_of::<MaterialGpu>() as u64 * 16,
            ),
            groups: storage(
                device,
                "grana-glass groups",
                size_of::<GroupGpu>() as u64 * 16,
            ),
            pipelines,
            backdrops: Vec::new(),
            layered: None,
            layer_textures: Vec::new(),
            packed: Packed::default(),
            uploaded: Uploaded::default(),
            binds: Binds {
                blit: BindCache::new(),
                down_first: BindCache::new(),
                glass: BindCache::new(),
            },
            rebuild: Vec::new(),
        }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame<'_>,
        scene: &Scene,
    ) {
        self.render_layers(device, queue, encoder, frame, scene, ..);
    }

    pub fn render_layers(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame<'_>,
        scene: &Scene,
        layers: impl RangeBounds<i32>,
    ) {
        let size = frame.size;
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        let layers = (layers.start_bound().cloned(), layers.end_bound().cloned());
        pack(
            scene,
            layers,
            frame.scale,
            Vec2::new(size.0 as f32, size.1 as f32),
            &mut self.packed,
        );
        if self.size != size {
            self.size = size;
            self.backdrops.clear();
            self.layered = None;
            self.layer_textures.clear();
            self.binds.blit.clear();
            self.binds.down_first.clear();
            self.binds.glass.clear();
        }
        self.upload(device, queue, frame, scene);
        {
            let pipelines = &self.pipelines;
            let copy = self.binds.blit.get(frame.content.clone(), |src| {
                blit_bind_group(device, pipelines, src)
            });
            if frame.copy {
                blit(pipelines, encoder, &copy, frame.target, None, size);
            }
        }
        if self.packed.layers.is_empty() {
            return;
        }

        let screen = [0.0, 0.0, size.0 as f32, size.1 as f32];
        let reach = self.packed.reach;
        let index = self.backdrop(device, frame.content);
        let mut backdrop = self.backdrops.remove(index);
        let Renderer {
            pipelines,
            binds,
            layered,
            layer_textures,
            rebuild,
            globals,
            shapes,
            materials,
            groups,
            format,
            packed,
            ..
        } = self;
        let layers = &packed.layers;
        let tail = |i: usize| {
            clamp_bounds(
                expand(
                    layers[i..]
                        .iter()
                        .fold(layers[i].bounds, |a, l| union(a, l.bounds)),
                    reach,
                ),
                screen,
            )
        };
        rebuild.clear();
        rebuild.resize(layers.len(), false);
        let mut below = layers[0].bounds;
        for (i, layer) in layers.iter().enumerate().skip(1) {
            rebuild[i] = intersects(expand(layer.bounds, reach), below);
            below = union(below, layer.bounds);
        }
        if rebuild.contains(&true) && layer_textures.is_empty() {
            *layer_textures = (0..2)
                .map(|_| layer_texture(device, *format, size))
                .collect();
            *layered = Some(Pyramid::new(device, pipelines, size));
        }

        let pyramid = &backdrop.pyramid;
        let region = tail(0);
        if frame.content_changed || !contains(backdrop.bounds, region) {
            let region = match frame.content_changed {
                true => region,
                false => screen,
            };
            let first = binds.down_first.get(frame.content.clone(), |src| {
                down_bind_group(device, &pipelines.down_first, &pipelines.sampler, src)
            });
            pyramid.build(pipelines, encoder, &first, region);
            backdrop.bounds = region;
        }

        let mut glass_group = |src: &wgpu::TextureView, blur: &Pyramid, layered: bool| {
            binds.glass.get((src.clone(), layered), |(src, _)| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("grana-glass glass"),
                    layout: &pipelines.glass.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: globals.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: shapes.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: groups.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&pipelines.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::TextureView(src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&blur.full),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: materials.as_entire_binding(),
                        },
                    ],
                })
            })
        };

        let mut group = glass_group(frame.content, pyramid, false);
        let (mut src, mut baked, mut slot) = (frame.content, 0, 0);
        for (i, layer) in layers.iter().enumerate() {
            if let (true, Some(blurred)) = (rebuild[i], layered.as_ref()) {
                let region = tail(i);

                let dst = &layer_textures[slot];
                slot ^= 1;
                let copy = binds
                    .blit
                    .get(src.clone(), |src| blit_bind_group(device, pipelines, src));
                blit(pipelines, encoder, &copy, dst, Some(region), size);

                for lower in &layers[baked..i] {
                    draw_layer(pipelines, encoder, &group, lower, dst, Some(region), size);
                }

                let first = binds.down_first.get(dst.clone(), |src| {
                    down_bind_group(device, &pipelines.down_first, &pipelines.sampler, src)
                });
                blurred.build(pipelines, encoder, &first, region);
                group = glass_group(dst, blurred, true);
                (src, baked) = (dst, i);
            }
            draw_layer(pipelines, encoder, &group, layer, frame.target, None, size);
        }
        self.backdrops
            .insert(index.min(self.backdrops.len()), backdrop);
    }

    fn backdrop(&mut self, device: &wgpu::Device, source: &wgpu::TextureView) -> usize {
        if let Some(index) = self.backdrops.iter().position(|b| b.source == *source) {
            return index;
        }
        if self.backdrops.len() >= BACKDROPS {
            self.backdrops.remove(0);
        }
        self.backdrops.push(Backdrop {
            source: source.clone(),
            pyramid: Pyramid::new(device, &self.pipelines, self.size),
            bounds: [0.0; 4],
        });
        self.backdrops.len() - 1
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame<'_>,
        scene: &Scene,
    ) {
        let (w, h) = (frame.size.0 as f32, frame.size.1 as f32);
        let touch = &scene.touch;
        let globals = Globals {
            viewport: [w, h, 1.0 / w, 1.0 / h],
            misc: [
                frame.scale,
                Pyramid::levels(frame.size) as f32,
                if scene.appearance.dark { 0.0 } else { 1.0 },
                0.0,
            ],
            light: [0.0, -1.0, 0.0, 0.0],
            touch: [
                touch.pos.x * frame.scale,
                touch.pos.y * frame.scale,
                touch.radius.value.max(0.0) * frame.scale,
                touch.intensity.value.max(0.0),
            ],
        };

        if self.uploaded.globals != Some(globals) {
            queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
            self.uploaded.globals = Some(globals);
        }
        let grown = write_storage(
            device,
            queue,
            &mut self.shapes,
            "grana-glass shapes",
            &self.packed.shapes,
            &mut self.uploaded.shapes,
        ) | write_storage(
            device,
            queue,
            &mut self.materials,
            "grana-glass materials",
            &self.packed.materials,
            &mut self.uploaded.materials,
        ) | write_storage(
            device,
            queue,
            &mut self.groups,
            "grana-glass groups",
            &self.packed.groups,
            &mut self.uploaded.groups,
        );
        if grown {
            self.binds.glass.clear();
        }
    }
}

fn draw_layer(
    pipelines: &Pipelines,
    encoder: &mut wgpu::CommandEncoder,
    group: &wgpu::BindGroup,
    layer: &Layer,
    dst: &wgpu::TextureView,
    region: Option<Bounds>,
    size: (u32, u32),
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("grana-glass glass"),
        color_attachments: &[Some(attachment(dst))],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(region) = region
        && !scissor(&mut pass, region, size)
    {
        return;
    }
    pass.set_pipeline(&pipelines.glass);
    pass.set_bind_group(0, group, &[]);
    pass.draw(0..4, layer.groups.clone());
}

fn layer_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("grana-glass layer backdrop"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn write_storage<T: Pod + PartialEq>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &mut wgpu::Buffer,
    label: &str,
    data: &[T],
    last: &mut Vec<T>,
) -> bool {
    let bytes: &[u8] = bytemuck::cast_slice(data);
    let grown = bytes.len() as u64 > buffer.size();
    if grown {
        *buffer = storage(device, label, (bytes.len() as u64).next_power_of_two());
    }
    if grown || data != last.as_slice() {
        if !bytes.is_empty() {
            queue.write_buffer(buffer, 0, bytes);
        }
        last.clear();
        last.extend_from_slice(data);
    }
    grown
}
