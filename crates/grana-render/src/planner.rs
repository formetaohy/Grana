use bytemuck::{bytes_of, cast_slice};
use glam::Vec3;
use grana_volume::{BRICK_SIDE, BrickGrid, Pyramid, Region};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoder, ComputePass, ComputePipeline, ComputePipelineDescriptor, Device, Queue,
    ShaderModuleDescriptor, ShaderSource,
};

use crate::ViewInfo;
use crate::brick::BrickTable;
use crate::frame::ScheduleData;
use crate::layout::Layouts;
use grana_display::{Camera, SectionCamera};
use grana_volume::BrickRequest;

pub(crate) const BUCKETS: usize = 128;
const LEVEL_HYSTERESIS: f32 = 1.2;
const HEADER_WORDS: usize = 4;
const DEMAND_BASE: usize = HEADER_WORDS + 3 * BUCKETS;
const MISSING: u32 = 0x8000_0000;
const WORKGROUP: u32 = 64;
const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/frustum.wgsl"),
    "\n",
    include_str!("shader/schedule.wgsl"),
    "\n",
    include_str!("shader/cull.wgsl")
);
const PLAN_SHADER: &str = concat!(
    include_str!("shader/schedule.wgsl"),
    "\n",
    include_str!("shader/plan.wgsl")
);

pub(crate) fn plan_words(capacity: usize) -> usize {
    DEMAND_BASE + capacity
}

#[derive(Clone, Copy, Default)]
pub(crate) struct SectionRange {
    pub layer: u32,
    pub u0: u32,
    pub v0: u32,
    pub u_count: u32,
    pub v_count: u32,
}

impl SectionRange {
    pub fn dispatch(&self) -> u32 {
        self.u_count * self.v_count
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Schedule {
    pub time: u32,
    pub level: usize,
    pub capacity: u32,
    pub layer: u32,
    pub range: SectionRange,
    pub dispatch: u32,
    pub bucket_scale: f32,
}

pub(crate) struct Demand {
    pub info: ViewInfo,
    pub requests: Vec<BrickRequest>,
    pub indices: Vec<u32>,
}

pub(crate) struct Plan {
    capacity: usize,
    buffer: Buffer,
    uniform: Buffer,
    storage_group: BindGroup,
    data_group: BindGroup,
    level_group: Option<(usize, BindGroup)>,
    dispatch: u32,
}

impl Plan {
    fn new(device: &Device, layouts: &Layouts, label: &'static str, capacity: usize) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: (plan_words(capacity) * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: size_of::<ScheduleData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let storage_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout: &layouts.plan_storage,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        let data_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout: &layouts.plan_data,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });
        Self {
            capacity,
            buffer,
            uniform,
            storage_group,
            data_group,
            level_group: None,
            dispatch: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn dispatch(&self) -> u32 {
        self.dispatch
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    fn storage_group(&self) -> &BindGroup {
        &self.storage_group
    }

    fn data_group(&self) -> &BindGroup {
        &self.data_group
    }

    fn level_group(&self) -> &BindGroup {
        self.level_group
            .as_ref()
            .map(|(_, group)| group)
            .expect("the plan binds a brick table before it encodes")
    }

    pub fn prepare(&mut self, queue: &Queue, schedule: Schedule) {
        self.dispatch = schedule.dispatch;
        let data = ScheduleData {
            time: schedule.time,
            level: schedule.level as u32,
            capacity: schedule.capacity.min(self.capacity as u32),
            layer: schedule.layer,
            u0: schedule.range.u0,
            v0: schedule.range.v0,
            u_count: schedule.range.u_count,
            v_count: schedule.range.v_count,
            bucket_scale: schedule.bucket_scale,
            reserved: [0; 3],
        };
        queue.write_buffer(&self.uniform, 0, bytes_of(&data));
        let header = [0u32; HEADER_WORDS + BUCKETS];
        queue.write_buffer(&self.buffer, 0, cast_slice(&header));
    }

    pub fn bind_level(&mut self, device: &Device, layouts: &Layouts, table: &BrickTable) {
        let stale = self
            .level_group
            .as_ref()
            .is_none_or(|(level, _)| *level != table.level());
        if stale {
            let group = device.create_bind_group(&BindGroupDescriptor {
                label: Some("grana.level.write.group"),
                layout: &layouts.level_write,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: table.buffer().as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: self.uniform.as_entire_binding(),
                    },
                ],
            });
            self.level_group = Some((table.level(), group));
        }
    }
}

pub(crate) struct Planner {
    plan: ComputePipeline,
    volume_count: ComputePipeline,
    volume_scatter: ComputePipeline,
    section_count: ComputePipeline,
    section_scatter: ComputePipeline,
    pub volume: Plan,
    pub section: Plan,
}

impl Planner {
    pub fn new(device: &Device, layouts: &Layouts, capacity: usize) -> Self {
        let cull = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.cull"),
            source: ShaderSource::Wgsl(CULL_SHADER.into()),
        });
        let plan = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.plan"),
            source: ShaderSource::Wgsl(PLAN_SHADER.into()),
        });
        let pipeline = |label: &'static str, module: &wgpu::ShaderModule, entry: &str| {
            device.create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&layouts.cull),
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let plan_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.plan"),
            layout: Some(&layouts.plan_pipeline),
            module: &plan,
            entry_point: Some("plan_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            plan: plan_pipeline,
            volume_count: pipeline("grana.volume.count", &cull, "volume_count"),
            volume_scatter: pipeline("grana.volume.scatter", &cull, "volume_scatter"),
            section_count: pipeline("grana.section.count", &cull, "section_count"),
            section_scatter: pipeline("grana.section.scatter", &cull, "section_scatter"),
            volume: Plan::new(device, layouts, "grana.volume.plan", capacity),
            section: Plan::new(device, layouts, "grana.section.plan", capacity),
        }
    }

    pub fn encode(
        &self,
        encoder: &mut CommandEncoder,
        scene: &BindGroup,
        volume: &Plan,
        section: &Plan,
    ) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.plan.count"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, scene, &[]);
            dispatch(&mut pass, &self.volume_count, volume);
            dispatch(&mut pass, &self.section_count, section);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.plan.reduce"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.plan);
            for plan in [volume, section] {
                pass.set_bind_group(0, plan.data_group(), &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.plan.scatter"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, scene, &[]);
            dispatch(&mut pass, &self.volume_scatter, volume);
            dispatch(&mut pass, &self.section_scatter, section);
        }
    }
}

fn dispatch(pass: &mut ComputePass<'_>, pipeline: &ComputePipeline, plan: &Plan) {
    if plan.dispatch() == 0 {
        return;
    }
    pass.set_pipeline(pipeline);
    pass.set_bind_group(1, plan.level_group(), &[]);
    pass.set_bind_group(2, plan.storage_group(), &[]);
    pass.dispatch_workgroups(plan.dispatch().div_ceil(WORKGROUP), 1, 1);
}

pub(crate) fn decode_plan(
    words: &[u32],
    capacity: usize,
    time: u32,
    level: usize,
    pyramid: &Pyramid,
) -> Option<Demand> {
    if words.len() < DEMAND_BASE {
        return None;
    }
    let visible = words[0] as usize;
    let demand = (words[1] as usize)
        .min(capacity)
        .min(words.len().saturating_sub(DEMAND_BASE));
    let grid = BrickGrid::new(pyramid.level(level).extent());
    let mut requests = Vec::new();
    let mut indices = Vec::with_capacity(demand);
    for entry in &words[DEMAND_BASE..DEMAND_BASE + demand] {
        let index = entry & !MISSING;
        indices.push(index);
        if entry & MISSING != 0 {
            requests.push(BrickRequest {
                time,
                level,
                coord: grid.coord(index as usize),
            });
        }
    }
    Some(Demand {
        info: ViewInfo {
            level,
            visible,
            demand,
            resident: demand - requests.len(),
        },
        requests,
        indices,
    })
}

pub(crate) fn bucket_scale(camera: &Camera, origin: [f32; 3], world: [f32; 3]) -> f32 {
    let eye = camera.eye();
    let mut far = 0.0f32;
    for index in 0..8u32 {
        let corner = Vec3::new(
            origin[0] + if index & 1 == 0 { 0.0 } else { world[0] },
            origin[1] + if index & 2 == 0 { 0.0 } else { world[1] },
            origin[2] + if index & 4 == 0 { 0.0 } else { world[2] },
        );
        far = far.max((corner - eye).length());
    }
    BUCKETS as f32 / far.max(1.0e-6)
}

pub(crate) fn section_range(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &Camera,
    section: &SectionCamera,
    aspect: f32,
    region: Region,
) -> SectionRange {
    let level = pyramid.level(level_index);
    let extent = level.extent();
    let world = level.world_extent();
    let origin = level.origin();
    let dims = [extent.x() as f32, extent.y() as f32, extent.z() as f32];
    let axis = section.axis().index();
    let (u_axis, v_axis) = section.axis().in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let focus = camera.focus();
    let layer = (((focus[axis] - origin[axis]) / world[axis]) * dims[axis])
        .floor()
        .clamp(0.0, dims[axis] - 1.0);
    let center_u = (focus[u] - origin[u]) / world[u];
    let center_v = (focus[v] - origin[v]) / world[v];
    let half_u = section.span() * aspect / (2.0 * world[u]);
    let half_v = section.span() / (2.0 * world[v]);
    let mut umin = (center_u - half_u).clamp(0.0, 1.0);
    let mut umax = (center_u + half_u).clamp(0.0, 1.0);
    let mut vmin = (center_v - half_v).clamp(0.0, 1.0);
    let mut vmax = (center_v + half_v).clamp(0.0, 1.0);
    if region.enabled() {
        let region_min = region.min();
        let region_max = region.max();
        if focus[axis] < region_min[axis] || focus[axis] > region_max[axis] {
            return SectionRange::default();
        }
        let region_umin = ((region_min[u] - origin[u]) / world[u]).clamp(0.0, 1.0);
        let region_umax = ((region_max[u] - origin[u]) / world[u]).clamp(0.0, 1.0);
        let region_vmin = ((region_min[v] - origin[v]) / world[v]).clamp(0.0, 1.0);
        let region_vmax = ((region_max[v] - origin[v]) / world[v]).clamp(0.0, 1.0);
        umin = umin.max(region_umin);
        umax = umax.min(region_umax);
        vmin = vmin.max(region_vmin);
        vmax = vmax.min(region_vmax);
    }
    if umin >= umax || vmin >= vmax {
        return SectionRange::default();
    }
    let u_extent = u_axis.extent(extent);
    let v_extent = v_axis.extent(extent);
    let u0 = ((umin * dims[u]).floor() as u32).min(u_extent - 1);
    let u1 = ((umax * dims[u]).ceil() as u32)
        .saturating_sub(1)
        .min(u_extent - 1);
    let v0 = ((vmin * dims[v]).floor() as u32).min(v_extent - 1);
    let v1 = ((vmax * dims[v]).ceil() as u32)
        .saturating_sub(1)
        .min(v_extent - 1);
    SectionRange {
        layer: (layer as u32) / BRICK_SIDE,
        u0: u0 / BRICK_SIDE,
        v0: v0 / BRICK_SIDE,
        u_count: u1 / BRICK_SIDE - u0 / BRICK_SIDE + 1,
        v_count: v1 / BRICK_SIDE - v0 / BRICK_SIDE + 1,
    }
}

pub(crate) fn volume_dispatch(table: &BrickTable) -> u32 {
    table.grid.count() as u32
}

pub(crate) fn select_volume_level(
    pyramid: &Pyramid,
    camera: &Camera,
    height: f32,
    current: Option<usize>,
) -> usize {
    let pixel_world = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        let voxel = minimum_spacing(pyramid, index);
        if voxel <= pixel_world {
            level = index;
        } else {
            break;
        }
    }
    hold_level(
        pyramid.level_count(),
        level,
        current,
        pixel_world,
        |index| minimum_spacing(pyramid, index),
    )
}

pub(crate) fn select_section_level(
    pyramid: &Pyramid,
    section: &SectionCamera,
    height: f32,
    current: Option<usize>,
) -> usize {
    let pixel_world = section.span() / height.max(1.0);
    let mut level = 0;
    for index in 0..pyramid.level_count() {
        if section_spacing(pyramid, index, section.axis()) <= pixel_world {
            level = index;
        } else {
            break;
        }
    }
    hold_level(
        pyramid.level_count(),
        level,
        current,
        pixel_world,
        |index| section_spacing(pyramid, index, section.axis()),
    )
}

fn hold_level(
    levels: usize,
    target: usize,
    current: Option<usize>,
    footprint: f32,
    spacing: impl Fn(usize) -> f32,
) -> usize {
    let Some(current) = current.filter(|current| *current < levels) else {
        return target;
    };
    if target < current && footprint > spacing(current - 1) / LEVEL_HYSTERESIS {
        return current;
    }
    if target > current && footprint < spacing(current) * LEVEL_HYSTERESIS {
        return current;
    }
    target
}

pub(crate) fn resolve_section_level(
    pyramid: &Pyramid,
    camera: &Camera,
    section: &SectionCamera,
    aspect: f32,
    target: usize,
    capacity: usize,
    region: Region,
) -> (usize, usize) {
    let levels = pyramid.level_count();
    let mut level = target.min(levels - 1);
    loop {
        let count =
            section_range(pyramid, level, camera, section, aspect, region).dispatch() as usize;
        if count <= capacity || level + 1 == levels {
            return (level, count);
        }
        level += 1;
    }
}

pub(crate) fn resolve_volume_level(
    target: usize,
    levels: usize,
    capacity: usize,
    counts: &[u32],
) -> usize {
    let mut level = target.min(levels - 1);
    while level + 1 < levels && counts.get(level).copied().unwrap_or(0) as usize > capacity {
        level += 1;
    }
    level
}

fn minimum_spacing(pyramid: &Pyramid, level: usize) -> f32 {
    let spacing = pyramid.level(level).spacing();
    spacing.x().min(spacing.y()).min(spacing.z())
}

fn section_spacing(pyramid: &Pyramid, level: usize, axis: grana_volume::Axis) -> f32 {
    let spacing = pyramid.level(level).spacing();
    let (u, v) = axis.in_plane();
    u.spacing(spacing).max(v.spacing(spacing))
}
