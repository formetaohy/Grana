use bytemuck::{bytes_of, cast_slice};
use glam::Vec3;
use grana_gpu::Pipelines;
use grana_volume::{BrickGrid, Pyramid, Region};
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

pub(crate) fn plan_bytes(capacity: usize) -> u64 {
    (plan_words(capacity) * size_of::<u32>()) as u64
}

pub(crate) fn bytes(capacity: usize) -> u64 {
    BAND as u64 * (plan_bytes(capacity) + size_of::<ScheduleData>() as u64)
}

pub(crate) const READBACK_SLOTS: usize = 4;

pub(crate) fn readback_bytes(capacity: usize) -> u64 {
    READBACK_SLOTS as u64 * BAND as u64 * plan_bytes(capacity)
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
    pub missing: Vec<(u32, u32)>,
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
            size: plan_bytes(capacity),
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

    pub fn bytes(&self) -> u64 {
        bytes(self.view.bands[0].capacity())
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
    bytes: &[u8],
    capacity: usize,
    time: u32,
    level: usize,
    pyramid: &Pyramid,
) -> Option<Demand> {
    if bytes.len() < DEMAND_BASE * size_of::<u32>() {
        return None;
    }
    let word = |index: usize| {
        let start = index * size_of::<u32>();
        u32::from_ne_bytes(
            bytes[start..start + size_of::<u32>()]
                .try_into()
                .expect("a plan word holds four bytes"),
        )
    };
    let visible = word(0) as usize;
    let demand = (word(1) as usize)
        .min(capacity)
        .min(bytes.len() / size_of::<u32>() - DEMAND_BASE);
    let mut bucket_of = vec![0u32; demand];
    for bucket in 0..BUCKET_COUNT {
        let base = word(BASE_BASE + bucket);
        let fill = word(FILL_BASE + bucket) as usize;
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
    let mut missing = Vec::new();
    let mut indices = Vec::with_capacity(demand);
    for (position, bucket) in bucket_of.iter().enumerate() {
        let entry = word(DEMAND_BASE + position);
        let index = entry & !MISSING;
        indices.push(index);
        if entry & MISSING != 0 {
            missing.push((index, *bucket));
            requests.push(BrickRequest::new(time, level, grid.coord(index as usize)));
        }
    }
    Some(Demand {
        info: ViewInfo {
            level,
            visible,
            demand,
            resident: demand - requests.len(),
            unavailable: 0,
        },
        requests,
        missing,
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

pub(crate) fn volume_dispatch(table: &crate::brick::BrickTable) -> u32 {
    table.grid.count() as u32
}

#[derive(Clone, Copy)]
pub(crate) struct TargetView<'a> {
    pub(crate) pyramid: &'a Pyramid,
    pub(crate) camera: &'a Camera,
    pub(crate) height: f32,
    pub(crate) first: usize,
    pub(crate) current: Option<usize>,
}

pub(crate) fn volume_target(view: TargetView<'_>) -> usize {
    let TargetView {
        pyramid,
        camera,
        height,
        first,
        current,
    } = view;
    let levels = pyramid.level_count();
    let pixel_world = match camera.projection() {
        CameraProjection::Perspective => {
            2.0 * nearest_distance(pyramid.level(0), camera.eye()) * (camera.fov_y() * 0.5).tan()
                / height.max(1.0)
        }
        CameraProjection::Orthographic => 2.0 * camera.half_height() / height.max(1.0),
    };
    let footprint = pixel_world / crate::material::samples_per_voxel(pyramid);
    let mut target = first;
    for index in first..levels {
        if minimum_spacing(pyramid, index) <= footprint {
            target = index;
        } else {
            break;
        }
    }
    hold_level(levels, target, current, footprint, |index| {
        minimum_spacing(pyramid, index)
    })
    .max(first)
}

pub(crate) struct Counts<'a> {
    frustum: crate::culling::Frustum,
    pyramid: &'a Pyramid,
    scale: f32,
    first: usize,
    hollow: Option<crate::hollow::Cursor<'a>>,
    bands: Vec<crate::culling::Counts>,
}

impl<'a> Counts<'a> {
    pub(crate) fn new(
        pyramid: &'a Pyramid,
        camera: &Camera,
        region: Region,
        aspect: f32,
        height: u32,
        first: usize,
        hollow: Option<crate::hollow::Cursor<'a>>,
    ) -> Self {
        assert!(aspect > 0.0, "a view has a positive aspect");
        let scale =
            camera.pixels_per_world_at(height as f32) * crate::material::samples_per_voxel(pyramid);
        let radius = 0.5 * Vec3::from(pyramid.level(first).world_extent()).length();
        let planes = crate::frame::planes(camera.view_projection(aspect, radius));
        Self {
            frustum: crate::culling::Frustum::new(planes, camera.eye(), region),
            pyramid,
            scale,
            first,
            hollow,
            bands: Vec::new(),
        }
    }

    pub(crate) fn volume(&mut self, base: usize, count: usize) -> [usize; BAND] {
        for level in base..base + count {
            self.ensure(level);
        }
        let mut wanted = [0usize; BAND];
        for (band, slot) in wanted.iter_mut().enumerate().take(count) {
            let level = self.bands.get(base + band - self.first);
            *slot = level.map_or(0, |counts| counts.band(band, count));
        }
        wanted
    }

    pub(crate) fn bricks(&self, level: usize) -> Vec<u32> {
        self.frustum.gather(self.pyramid.level(level))
    }

    fn ensure(&mut self, level: usize) -> crate::culling::Counts {
        while self.first + self.bands.len() <= level {
            let held = self.first + self.bands.len();
            self.bands.push(self.band(held));
        }
        self.bands[level - self.first]
    }

    fn band(&self, level: usize) -> crate::culling::Counts {
        let source = self.pyramid.level(level);
        let lower = BAND_FLOOR * Vec3::from(source.voxel()).min_element() * self.scale;
        let upper = match level + 1 {
            next if next < self.pyramid.level_count() => {
                BAND_CEIL * Vec3::from(self.pyramid.level(next).voxel()).min_element() * self.scale
            }
            _ => f32::INFINITY,
        };
        let grid = BrickGrid::new(source.extent()).counts();
        let mask = self.hollow.and_then(|hollow| hollow.mask(level, grid));
        self.frustum.counts(source, lower, upper, mask.as_ref())
    }
}

pub(crate) fn ladder_count(projection: CameraProjection, levels: usize, base: usize) -> usize {
    match projection {
        CameraProjection::Perspective => BAND.min(levels - base),
        CameraProjection::Orthographic => 1,
    }
}

pub(crate) fn volume_demand(
    pyramid: &Pyramid,
    camera: &Camera,
    region: Region,
    aspect: f32,
    height: u32,
    base: usize,
    count: usize,
) -> [usize; BAND] {
    assert!(
        count > 0 && base + count <= pyramid.level_count(),
        "a ladder spans levels the pyramid holds"
    );
    Counts::new(pyramid, camera, region, aspect, height, base, None).volume(base, count)
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
    if target < current && footprint >= spacing(current) / LEVEL_HYSTERESIS {
        return current;
    }
    if target > current && footprint <= spacing(target) * LEVEL_HYSTERESIS {
        return current;
    }
    target
}

fn minimum_spacing(pyramid: &Pyramid, level: usize) -> f32 {
    let spacing = pyramid.level(level).spacing();
    spacing.x().min(spacing.y()).min(spacing.z())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grana_display::Camera;
    use grana_volume::{
        Calibration, ChannelInfo, ColorTable, Extent, Intensity, Level, Spacing, Statistics,
        VoxelFormat, VoxelSemantics,
    };

    fn pyramid(palette: bool) -> Pyramid {
        let fine = Level::new(Extent::new(1024, 1024, 1024), Spacing::new(1.0, 1.0, 1.0));
        let mut levels = vec![fine];
        for _ in 0..2 {
            let coarser = levels.last().expect("a level").coarser();
            levels.push(coarser);
        }
        let statistics = Statistics::declared(Intensity::new(0.0, 255.0), VoxelSemantics::Scalar);
        let channel = if palette {
            ChannelInfo::new("labels", [0.2, 0.6, 0.2], statistics, VoxelFormat::U8)
                .with_color_table(ColorTable::new(vec![[0.0, 0.0, 0.0], [0.2, 0.6, 0.2]]))
        } else {
            ChannelInfo::new("signal", [0.2, 0.6, 0.2], statistics, VoxelFormat::U8)
        };
        Pyramid::new(levels, 3, vec![channel], 1, 1.0, Calibration::Recorded)
    }

    fn target(pyramid: &Pyramid, height: f32) -> usize {
        let mut camera = Camera::new(35.0);
        camera.fit(Vec3::from(pyramid.level(0).world_extent()), 1.0);
        volume_target(TargetView {
            pyramid,
            camera: &camera,
            height,
            first: 0,
            current: None,
        })
    }

    #[test]
    fn keeps_a_categorical_volume_finer_than_the_same_scalar_volume() {
        let scalar = pyramid(false);
        let categorical = pyramid(true);
        let held = target(&scalar, 300.0);
        let kept = target(&categorical, 300.0);
        assert!(held > 0, "the scalar volume coarsens at this pixel size");
        assert!(
            kept < held,
            "a categorical volume keeps two samples per voxel: level {kept} against {held}"
        );
    }
}
