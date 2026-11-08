use bytemuck::{bytes_of, cast_slice};
use glam::Vec3;
use grana_volume::{BRICK_SIDE, BrickGrid, Pyramid, Region};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoder, ComputePass, ComputePipeline, ComputePipelineDescriptor, Device, Queue,
    ShaderModuleDescriptor, ShaderSource,
};

use crate::ViewInfo;
use crate::brick::BrickArena;
use crate::frame::ScheduleData;
use crate::layout::Layouts;
use grana_display::{Camera, SectionCamera};
use grana_volume::BrickRequest;

pub(crate) const BAND: usize = 4;

pub(crate) const BUCKET_COUNT: usize = 128;
const LEVEL_HYSTERESIS: f32 = 1.2;
const HEADER_WORDS: usize = 4;
const VISIBLE_BASE: usize = HEADER_WORDS;
const BASE_BASE: usize = VISIBLE_BASE + BUCKET_COUNT;
const FILL_BASE: usize = BASE_BASE + BUCKET_COUNT;
const DEMAND_BASE: usize = FILL_BASE + BUCKET_COUNT;
const MISSING: u32 = 0x8000_0000;
const EXCLUDED: u32 = u32::MAX;
const WORKGROUP: u32 = 64;
const CULL_SHADER: &str = concat!(
    include_str!("shader/scene.wgsl"),
    "\n",
    include_str!("shader/brick.wgsl"),
    "\n",
    include_str!("shader/band.wgsl"),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Ladder {
    pub base: usize,
    pub count: usize,
}

impl Ladder {
    pub fn level(&self, band: usize) -> usize {
        assert!(band < self.count, "a ladder names one of its bands");
        self.base + band
    }

    pub fn levels(&self) -> impl Iterator<Item = usize> + '_ {
        self.base..self.base + self.count
    }
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
    pub band: u32,
    pub capacity: u32,
    pub layer: u32,
    pub range: SectionRange,
    pub dispatch: u32,
    pub bucket_scale: f32,
    pub base: u32,
}

#[derive(Default)]
pub(crate) struct Demand {
    pub info: ViewInfo,
    pub requests: Vec<BrickRequest>,
    pub buckets: Vec<u32>,
    pub indices: Vec<u32>,
}

impl Demand {
    pub fn merge(&mut self, other: Self) {
        self.info.visible += other.info.visible;
        self.info.demand += other.info.demand;
        self.info.resident += other.info.resident;
        let mut requests = std::mem::take(&mut self.requests)
            .into_iter()
            .zip(std::mem::take(&mut self.buckets))
            .chain(other.requests.into_iter().zip(other.buckets))
            .collect::<Vec<_>>();
        requests.sort_by_key(|(_, bucket)| *bucket);
        for (request, bucket) in requests {
            self.requests.push(request);
            self.buckets.push(bucket);
        }
        self.indices.extend(other.indices);
    }
}

pub(crate) struct Plan {
    capacity: usize,
    buffer: Buffer,
    uniform: Buffer,
    storage_group: BindGroup,
    data_group: BindGroup,
    write_group: Option<BindGroup>,
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
            write_group: None,
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

    fn write_group(&self) -> &BindGroup {
        self.write_group
            .as_ref()
            .expect("the plan binds the brick arena before it encodes")
    }

    pub fn prepare(&mut self, queue: &Queue, schedule: Schedule) {
        self.dispatch = schedule.dispatch;
        let data = ScheduleData {
            time: schedule.time,
            band: schedule.band,
            capacity: schedule.capacity.min(self.capacity as u32),
            layer: schedule.layer,
            u0: schedule.range.u0,
            v0: schedule.range.v0,
            u_count: schedule.range.u_count,
            v_count: schedule.range.v_count,
            bucket_scale: schedule.bucket_scale,
            base: schedule.base,
            reserved: [0; 2],
        };
        queue.write_buffer(&self.uniform, 0, bytes_of(&data));
        let header = [0u32; HEADER_WORDS + BUCKET_COUNT];
        queue.write_buffer(&self.buffer, 0, cast_slice(&header));
    }

    pub fn bind_arena(&mut self, device: &Device, layouts: &Layouts, arena: &BrickArena) {
        if self.write_group.is_some() {
            return;
        }
        self.write_group = Some(device.create_bind_group(&BindGroupDescriptor {
            label: Some("grana.arena.write.group"),
            layout: &layouts.level_write,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: arena.buffer().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        }));
    }
}

pub(crate) struct Planner {
    plan: ComputePipeline,
    volume_count: ComputePipeline,
    volume_scatter: ComputePipeline,
    section_count: ComputePipeline,
    section_scatter: ComputePipeline,
    pub bands: [Plan; BAND],
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
            bands: std::array::from_fn(|_| Plan::new(device, layouts, "grana.band.plan", capacity)),
            section: Plan::new(device, layouts, "grana.section.plan", capacity),
        }
    }

    pub fn bind_arena(&mut self, device: &Device, layouts: &Layouts, arena: &BrickArena) {
        for plan in self
            .bands
            .iter_mut()
            .chain(std::iter::once(&mut self.section))
        {
            plan.bind_arena(device, layouts, arena);
        }
    }

    pub fn encode(&self, encoder: &mut CommandEncoder, scene: &BindGroup) {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.plan.count"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, scene, &[]);
            for plan in &self.bands {
                dispatch(&mut pass, &self.volume_count, plan);
            }
            dispatch(&mut pass, &self.section_count, &self.section);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.plan.reduce"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.plan);
            for plan in self.bands.iter().chain(std::iter::once(&self.section)) {
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
            for plan in &self.bands {
                dispatch(&mut pass, &self.volume_scatter, plan);
            }
            dispatch(&mut pass, &self.section_scatter, &self.section);
        }
    }
}

fn dispatch(pass: &mut ComputePass<'_>, pipeline: &ComputePipeline, plan: &Plan) {
    if plan.dispatch() == 0 {
        return;
    }
    pass.set_pipeline(pipeline);
    pass.set_bind_group(1, plan.write_group(), &[]);
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
    let mut bucket_of = vec![0u32; demand];
    for bucket in 0..BUCKET_COUNT {
        let base = words[BASE_BASE + bucket];
        let fill = words[FILL_BASE + bucket] as usize;
        if base == EXCLUDED || fill == 0 {
            continue;
        }
        let start = (base as usize).min(demand);
        let end = (start + fill).min(demand);
        for entry in &mut bucket_of[start..end] {
            *entry = bucket as u32;
        }
    }
    let grid = BrickGrid::new(pyramid.level(level).extent());
    let mut requests = Vec::new();
    let mut buckets = Vec::new();
    let mut indices = Vec::with_capacity(demand);
    for (position, entry) in words[DEMAND_BASE..DEMAND_BASE + demand].iter().enumerate() {
        let index = entry & !MISSING;
        indices.push(index);
        if entry & MISSING != 0 {
            buckets.push(bucket_of[position]);
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
        buckets,
        indices,
    })
}

pub(crate) fn capacities(budget: usize, wanted: &[usize; BAND], count: usize) -> [usize; BAND] {
    assert!(
        budget > 0 && count > 0 && count <= BAND,
        "a ladder divides a budget"
    );
    let mut weights = [1usize; BAND];
    for band in 0..count {
        weights[band] = wanted[band].max(1);
    }
    let total: usize = weights[..count].iter().sum();
    let mut caps = [0usize; BAND];
    for band in 0..count {
        let share = (budget as u128 * weights[band] as u128 / total as u128) as usize;
        caps[band] = share.max(1);
    }
    let mut sum: usize = caps[..count].iter().sum();
    while sum > budget {
        let band = (0..count)
            .max_by_key(|band| caps[*band])
            .expect("a ladder holds a band");
        if caps[band] == 0 {
            break;
        }
        caps[band] -= 1;
        sum -= 1;
    }
    assert!(
        caps[..count].iter().any(|cap| *cap > 0),
        "a ladder reserves at least one brick"
    );
    caps
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
    BUCKET_COUNT as f32 / far.max(1.0e-6)
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

pub(crate) fn volume_dispatch(table: &crate::brick::BrickTable) -> u32 {
    table.grid.count() as u32
}

pub(crate) fn volume_ladder(
    pyramid: &Pyramid,
    camera: &Camera,
    height: f32,
    current: Option<usize>,
    first: usize,
    previous_visible: usize,
    capacity: usize,
) -> Ladder {
    let levels = pyramid.level_count();
    let nearest = nearest_distance(pyramid.level(0), camera.eye());
    let mut pixel_world = 2.0 * nearest * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    if previous_visible > capacity {
        pixel_world *= 2.0;
    }
    let mut target = first;
    for index in first..levels {
        if minimum_spacing(pyramid, index) <= pixel_world {
            target = index;
        } else {
            break;
        }
    }
    let base = hold_level(levels, target, current, pixel_world, |index| {
        minimum_spacing(pyramid, index)
    })
    .max(first);
    let count = BAND.min(levels - base);
    Ladder { base, count }
}

fn nearest_distance(level: grana_volume::Level, eye: Vec3) -> f32 {
    let origin = Vec3::from(level.origin());
    let corner = origin + Vec3::from(level.world_extent());
    let low = origin.min(corner);
    let high = origin.max(corner);
    (eye.clamp(low, high) - eye).length()
}

pub(crate) fn select_section_level(
    pyramid: &Pyramid,
    section: &SectionCamera,
    height: f32,
    current: Option<usize>,
    first: usize,
) -> usize {
    let pixel_world = section.span() / height.max(1.0);
    let mut level = first;
    for index in first..pyramid.level_count() {
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
    .max(first)
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

fn minimum_spacing(pyramid: &Pyramid, level: usize) -> f32 {
    let spacing = pyramid.level(level).spacing();
    spacing.x().min(spacing.y()).min(spacing.z())
}

fn section_spacing(pyramid: &Pyramid, level: usize, axis: grana_volume::Axis) -> f32 {
    let spacing = pyramid.level(level).spacing();
    let (u, v) = axis.in_plane();
    u.spacing(spacing).max(v.spacing(spacing))
}
