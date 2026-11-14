use bytemuck::{bytes_of, cast_slice};
use glam::Vec3;
use grana_gpu::Pipelines;
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
use grana_display::{Camera, CameraProjection};
use grana_volume::BrickRequest;

pub(crate) const BAND: usize = 4;

pub(crate) const BUCKET_COUNT: usize = 128;
const LEVEL_HYSTERESIS: f32 = 1.2;
const BAND_FLOOR: f32 = 0.75;
const BAND_CEIL: f32 = 1.25;
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

#[derive(Clone, Copy)]
pub(crate) struct Schedule {
    pub time: u32,
    pub band: u32,
    pub capacity: u32,
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
            x0: 0,
            y0: 0,
            z0: 0,
            x_count: 0,
            y_count: 0,
            bucket_scale: schedule.bucket_scale,
            base: schedule.base,
            z_count: 0,
            reserved: 0,
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

pub(crate) struct Plans {
    pub bands: [Plan; BAND],
}

pub(crate) struct Planner {
    plan: ComputePipeline,
    volume_count: ComputePipeline,
    volume_scatter: ComputePipeline,
    pub view: Plans,
}

impl Planner {
    pub fn new(device: &Device, layouts: &Layouts, capacity: usize, pipelines: &Pipelines) -> Self {
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
                cache: pipelines.cache(),
            })
        };
        let plan_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.plan"),
            layout: Some(&layouts.plan_pipeline),
            module: &plan,
            entry_point: Some("plan_main"),
            compilation_options: Default::default(),
            cache: pipelines.cache(),
        });
        Self {
            plan: plan_pipeline,
            volume_count: pipeline("grana.volume.count", &cull, "volume_count"),
            volume_scatter: pipeline("grana.volume.scatter", &cull, "volume_scatter"),
            view: Plans {
                bands: std::array::from_fn(|_| {
                    Plan::new(device, layouts, "grana.band.plan", capacity)
                }),
            },
        }
    }

    pub fn bind_arena(&mut self, device: &Device, layouts: &Layouts, arena: &BrickArena) {
        for plan in &mut self.view.bands {
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
            for plan in &self.view.bands {
                dispatch(&mut pass, &self.volume_count, plan);
            }
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("grana.plan.reduce"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.plan);
            for plan in &self.view.bands {
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
            for plan in &self.view.bands {
                dispatch(&mut pass, &self.volume_scatter, plan);
            }
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

fn brick_bounds(level: grana_volume::Level, low: Vec3, size: Vec3) -> (Vec3, Vec3) {
    let basis = level.basis();
    let mut low_world = Vec3::from(level.origin());
    let mut high_world = low_world;
    for axis in 0..3 {
        let column = Vec3::from(basis.column(axis));
        let first = column * low[axis];
        let second = column * (low[axis] + size[axis]);
        low_world += first.min(second);
        high_world += first.max(second);
    }
    (low_world, high_world)
}

pub(crate) fn volume_dispatch(table: &crate::brick::BrickTable) -> u32 {
    table.grid.count() as u32
}

#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub(crate) first: usize,
    pub(crate) ceiling: usize,
}

impl Limits {
    pub(crate) fn count(self, pyramid: &Pyramid) -> usize {
        pyramid
            .level_count()
            .min(self.ceiling + 1)
            .max(self.first + 1)
    }
}

pub(crate) struct LadderView<'a> {
    pub(crate) pyramid: &'a Pyramid,
    pub(crate) camera: &'a Camera,
    pub(crate) height: f32,
    pub(crate) current: Option<usize>,
    pub(crate) limits: Limits,
}

pub(crate) fn volume_ladder(view: LadderView<'_>) -> Ladder {
    let LadderView {
        pyramid,
        camera,
        height,
        current,
        limits,
    } = view;
    let first = limits.first;
    let levels = limits.count(pyramid);
    let pixel_world = match camera.projection() {
        CameraProjection::Perspective => {
            2.0 * nearest_distance(pyramid.level(0), camera.eye()) * (camera.fov_y() * 0.5).tan()
                / height.max(1.0)
        }
        CameraProjection::Orthographic => 2.0 * camera.half_height() / height.max(1.0),
    };
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
    let count = match camera.projection() {
        CameraProjection::Perspective => BAND.min(levels - base),
        CameraProjection::Orthographic => 1,
    };
    Ladder { base, count }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VolumeFit {
    pub(crate) ladder: Ladder,
    pub(crate) capacity: [usize; BAND],
}

pub(crate) struct FitView<'a> {
    pub(crate) pyramid: &'a Pyramid,
    pub(crate) camera: &'a Camera,
    pub(crate) region: Region,
    pub(crate) aspect: f32,
    pub(crate) height: u32,
    pub(crate) limits: Limits,
    pub(crate) capacity: usize,
    pub(crate) current: Option<usize>,
}

pub(crate) fn fit_volume(view: FitView<'_>) -> VolumeFit {
    let FitView {
        pyramid,
        camera,
        region,
        aspect,
        height,
        limits,
        capacity,
        current,
    } = view;
    assert!(capacity > 0, "a volume fit reserves at least one brick");
    assert!(aspect > 0.0, "a view has a positive aspect");
    let levels = limits.count(pyramid);
    let desired = volume_ladder(LadderView {
        pyramid,
        camera,
        height: height as f32,
        current,
        limits,
    });
    let mut fallback: Option<VolumeFit> = None;
    for base in desired.base..levels {
        let count = ladder_count(camera.projection(), levels, base);
        let counts = volume_counts(pyramid, base, count, camera, aspect, height as f32, region);
        let demand = counts[..count].iter().sum::<usize>();
        if demand <= capacity {
            return VolumeFit {
                ladder: Ladder { base, count },
                capacity: counts,
            };
        }
        let closer = fallback
            .as_ref()
            .is_none_or(|held| demand < held.capacity[..held.ladder.count].iter().sum::<usize>());
        if closer {
            fallback = Some(VolumeFit {
                ladder: Ladder { base, count },
                capacity: capacities(capacity, &counts, count),
            });
        }
    }
    fallback.expect("the coarsest level holds a ladder")
}

fn ladder_count(projection: CameraProjection, levels: usize, base: usize) -> usize {
    match projection {
        CameraProjection::Perspective => BAND.min(levels - base),
        CameraProjection::Orthographic => 1,
    }
}

pub(crate) fn fit_measured(
    view: FitView<'_>,
    measure: &[Option<usize>],
    rejected: Option<usize>,
) -> (VolumeFit, Option<usize>) {
    let FitView {
        pyramid,
        camera,
        region: _,
        aspect: _,
        height,
        limits,
        capacity,
        current,
    } = view;
    assert!(capacity > 0, "a volume fit reserves at least one brick");
    let levels = limits.count(pyramid);
    let desired = volume_ladder(LadderView {
        pyramid,
        camera,
        height: height as f32,
        current,
        limits,
    });
    let mut base = current
        .unwrap_or(desired.base)
        .clamp(desired.base, levels - 1);
    let mut overflowed = None;
    while base + 1 < levels {
        let count = ladder_count(camera.projection(), levels, base);
        let Some(total) = measured_demand(measure, base, count) else {
            break;
        };
        if total <= capacity {
            break;
        }
        overflowed = Some(base);
        base += 1;
    }
    if base > desired.base {
        let target = base - 1;
        let headroom = match measure.get(target).copied().flatten() {
            Some(visible) => visible.saturating_mul(2) <= capacity,
            None => {
                rejected != Some(target)
                    && measure
                        .get(base)
                        .copied()
                        .flatten()
                        .is_some_and(|visible| visible.saturating_mul(2) <= capacity)
            }
        };
        if headroom {
            base = target;
        }
    }
    let count = ladder_count(camera.projection(), levels, base);
    let wanted: [usize; BAND] =
        std::array::from_fn(|band| measure.get(base + band).copied().flatten().unwrap_or(1));
    (
        VolumeFit {
            ladder: Ladder { base, count },
            capacity: capacities(capacity, &wanted, count),
        },
        overflowed,
    )
}

fn measured_demand(measure: &[Option<usize>], base: usize, count: usize) -> Option<usize> {
    let mut total = 0usize;
    for band in 0..count {
        total = total.checked_add(measure.get(base + band).copied().flatten()?)?;
    }
    Some(total)
}

pub(crate) fn volume_counts(
    pyramid: &Pyramid,
    base: usize,
    count: usize,
    camera: &Camera,
    aspect: f32,
    height: f32,
    region: Region,
) -> [usize; BAND] {
    let scale = camera.pixels_per_world_at(height);
    let radius = 0.5 * Vec3::from(pyramid.level(base).world_extent()).length();
    let planes = crate::frame::planes(camera.view_projection(aspect, radius));
    let eye = camera.eye();
    let mut counts = [0usize; BAND];
    for (band, slot) in counts.iter_mut().enumerate().take(count) {
        let level = pyramid.level(base + band);
        let lower = if band == 0 || band + 1 == count {
            0.0
        } else {
            BAND_FLOOR * Vec3::from(level.voxel()).min_element() * scale
        };
        let upper = if band + 1 < count {
            BAND_CEIL * Vec3::from(pyramid.level(base + band + 1).voxel()).min_element() * scale
        } else {
            f32::INFINITY
        };
        *slot = band_visible(level, lower, upper, eye, &planes, region);
    }
    counts
}

fn band_visible(
    level: grana_volume::Level,
    lower: f32,
    upper: f32,
    eye: Vec3,
    planes: &[[f32; 4]; 6],
    region: Region,
) -> usize {
    let extent = level.extent();
    let counts = BrickGrid::new(extent).counts();
    let dims = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
    let mut visible = 0;
    for z in 0..counts[2] {
        for y in 0..counts[1] {
            for x in 0..counts[0] {
                let cell = Vec3::new(x as f32, y as f32, z as f32) * BRICK_SIDE as f32;
                let size = (dims - cell).min(Vec3::splat(BRICK_SIDE as f32));
                let (low, high) = brick_bounds(level, cell, size);
                if outside_frustum(low, high, planes) || outside_region(low, high, region) {
                    continue;
                }
                let (near, far) = reach(low, high, eye);
                if far >= lower && near < upper {
                    visible += 1;
                }
            }
        }
    }
    visible
}

fn outside_frustum(low: Vec3, high: Vec3, planes: &[[f32; 4]; 6]) -> bool {
    planes.iter().any(|plane| {
        let direction = Vec3::from_array([plane[0], plane[1], plane[2]]);
        let positive = Vec3::new(
            if plane[0] >= 0.0 { high.x } else { low.x },
            if plane[1] >= 0.0 { high.y } else { low.y },
            if plane[2] >= 0.0 { high.z } else { low.z },
        );
        direction.dot(positive) + plane[3] < 0.0
    })
}

fn outside_region(low: Vec3, high: Vec3, region: Region) -> bool {
    if !region.enabled() {
        return false;
    }
    let half = region.half();
    let middle = 0.5 * (low + high);
    let extent = 0.5 * (high - low);
    let offset = middle - region.center();
    (0..3).any(|axis| {
        let direction = region.axis(axis);
        let reach = extent.dot(direction.abs());
        offset.dot(direction).abs() > half[axis] + reach
    })
}

fn reach(low: Vec3, high: Vec3, eye: Vec3) -> (f32, f32) {
    let near = (eye.clamp(low, high) - eye).length();
    let far = (eye - low).abs().max((eye - high).abs()).length();
    (near, far)
}

fn nearest_distance(level: grana_volume::Level, eye: Vec3) -> f32 {
    let (low, high) = level.bounds();
    let low = Vec3::from(low);
    let high = Vec3::from(high);
    (eye.clamp(low, high) - eye).length()
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

fn minimum_spacing(pyramid: &Pyramid, level: usize) -> f32 {
    let spacing = pyramid.level(level).spacing();
    spacing.x().min(spacing.y()).min(spacing.z())
}
