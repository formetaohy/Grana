use bytemuck::bytes_of;
use grana_volume::{BRICK_SIDE, BrickGrid, Pyramid};
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, Buffer, BufferDescriptor, BufferUsages,
    CommandEncoder, ComputePipeline, ComputePipelineDescriptor, Device, Queue,
    ShaderModuleDescriptor, ShaderSource,
};

use crate::ViewInfo;
use crate::brick::BrickTable;
use crate::camera::{Camera, SectionCamera};
use crate::frame::ScheduleData;
use crate::layout::Layouts;

pub(crate) const REQUEST_CAPACITY: usize = 4096;
pub(crate) const REQUEST_HEADER: usize = 4;
pub(crate) const VISIBLE_OFFSET: usize = REQUEST_HEADER + REQUEST_CAPACITY;
pub(crate) const VIEW_WORDS: usize = REQUEST_HEADER + 2 * REQUEST_CAPACITY;
const WORKGROUP: u32 = 64;

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

pub(crate) struct Cull {
    pub time: u32,
    pub level: usize,
    pub capacity: u32,
    pub layer: u32,
    pub range: SectionRange,
    pub dispatch: u32,
}

pub(crate) struct CullPass<'a> {
    pub pipeline: &'a ComputePipeline,
    pub scene: &'a BindGroup,
    pub level: &'a BindGroup,
    pub requests: &'a BindGroup,
    pub count: u32,
}

impl CullPass<'_> {
    pub fn encode(&self, encoder: &mut CommandEncoder) {
        if self.count == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("grana.cull.pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(self.pipeline);
        pass.set_bind_group(0, self.scene, &[]);
        pass.set_bind_group(1, self.level, &[]);
        pass.set_bind_group(2, self.requests, &[]);
        pass.dispatch_workgroups(self.count.div_ceil(WORKGROUP), 1, 1);
    }
}

pub(crate) struct Scheduler {
    pub volume_cull: ComputePipeline,
    pub section_cull: ComputePipeline,
    pub volume: View,
    pub section: View,
}

impl Scheduler {
    pub fn new(device: &Device, layouts: &Layouts) -> Self {
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("grana.cull"),
            source: ShaderSource::Wgsl(
                concat!(
                    include_str!("shader/scene.wgsl"),
                    "\n",
                    include_str!("shader/brick.wgsl"),
                    "\n",
                    include_str!("shader/frustum.wgsl"),
                    "\n",
                    include_str!("shader/cull.wgsl")
                )
                .into(),
            ),
        });
        let volume_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.volume.cull"),
            layout: Some(&layouts.volume_cull),
            module: &module,
            entry_point: Some("volume_cull"),
            compilation_options: Default::default(),
            cache: None,
        });
        let section_cull = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("grana.section.cull"),
            layout: Some(&layouts.section_cull),
            module: &module,
            entry_point: Some("section_cull"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            volume_cull,
            section_cull,
            volume: View::new(device, layouts, "grana.volume"),
            section: View::new(device, layouts, "grana.section"),
        }
    }
}

pub(crate) struct View {
    requests: Buffer,
    uniform: Buffer,
    request_group: BindGroup,
    level_group: Option<(usize, BindGroup)>,
    dispatch: u32,
    info: ViewInfo,
}

impl View {
    fn new(device: &Device, layouts: &Layouts, label: &'static str) -> Self {
        let requests = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: (VIEW_WORDS * size_of::<u32>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: size_of::<ScheduleData>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let request_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some(label),
            layout: &layouts.requests,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: requests.as_entire_binding(),
            }],
        });
        Self {
            requests,
            uniform,
            request_group,
            level_group: None,
            dispatch: 0,
            info: ViewInfo::default(),
        }
    }

    pub fn info(&self) -> ViewInfo {
        self.info
    }

    pub fn dispatch(&self) -> u32 {
        self.dispatch
    }

    pub fn buffer(&self) -> &Buffer {
        &self.requests
    }

    pub fn requests_group(&self) -> &BindGroup {
        &self.request_group
    }

    pub fn level_group(&self) -> Option<&BindGroup> {
        self.level_group.as_ref().map(|(_, group)| group)
    }

    pub fn prepare(&mut self, queue: &Queue, cull: Cull) {
        self.dispatch = cull.dispatch;
        self.info.level = cull.level;
        let data = ScheduleData {
            time: cull.time,
            level: cull.level as u32,
            capacity: cull.capacity.min(REQUEST_CAPACITY as u32),
            layer: cull.layer,
            u0: cull.range.u0,
            v0: cull.range.v0,
            u_count: cull.range.u_count,
            v_count: cull.range.v_count,
        };
        queue.write_buffer(&self.uniform, 0, bytes_of(&data));
        queue.write_buffer(&self.requests, 0, bytes_of(&[0u32; REQUEST_HEADER]));
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

    pub fn record(&mut self, visible: usize, resident: usize) {
        self.info.visible = visible;
        self.info.resident = resident;
    }
}

pub(crate) fn section_range(
    pyramid: &Pyramid,
    level_index: usize,
    camera: &Camera,
    section: &SectionCamera,
    aspect: f32,
) -> SectionRange {
    let level = pyramid.level(level_index);
    let extent = level.extent();
    let world = level.world_extent();
    let dims = [extent.x() as f32, extent.y() as f32, extent.z() as f32];
    let axis = section.axis().index();
    let (u_axis, v_axis) = section.axis().in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let focus = camera.focus();
    let layer = ((focus[axis] / world[axis] + 0.5) * dims[axis])
        .floor()
        .clamp(0.0, dims[axis] - 1.0);
    let center_u = focus[u] / world[u] + 0.5;
    let center_v = focus[v] / world[v] + 0.5;
    let half_u = section.span() * aspect / (2.0 * world[u]);
    let half_v = section.span() / (2.0 * world[v]);
    let umin = (center_u - half_u).clamp(0.0, 1.0);
    let umax = (center_u + half_u).clamp(0.0, 1.0);
    let vmin = (center_v - half_v).clamp(0.0, 1.0);
    let vmax = (center_v + half_v).clamp(0.0, 1.0);
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

pub(crate) fn select_volume_level(pyramid: &Pyramid, camera: &Camera, height: f32) -> usize {
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
    level
}

pub(crate) fn select_section_level(
    pyramid: &Pyramid,
    section: &SectionCamera,
    height: f32,
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
    level
}

pub(crate) fn resolve_section_level(
    pyramid: &Pyramid,
    camera: &Camera,
    section: &SectionCamera,
    aspect: f32,
    target: usize,
    capacity: usize,
) -> (usize, usize) {
    let levels = pyramid.level_count();
    let mut level = target.min(levels - 1);
    loop {
        let count = section_range(pyramid, level, camera, section, aspect).dispatch() as usize;
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

pub(crate) fn view_requests(
    words: &[u32],
    time: u32,
    level: usize,
    capacity: usize,
    pyramid: &Pyramid,
) -> Vec<crate::BrickRequest> {
    let count = (words[3] as usize)
        .min(capacity)
        .min(REQUEST_CAPACITY)
        .min(words.len().saturating_sub(REQUEST_HEADER));
    let grid = BrickGrid::new(pyramid.level(level).extent());
    words[REQUEST_HEADER..REQUEST_HEADER + count]
        .iter()
        .map(|index| crate::BrickRequest {
            time,
            level,
            coord: grid.coord(*index as usize),
        })
        .collect()
}

pub(crate) fn view_visible<'a>(
    words: &'a [u32],
    capacity: usize,
) -> impl Iterator<Item = u32> + 'a {
    let count = (words[1] as usize)
        .min(capacity)
        .min(REQUEST_CAPACITY)
        .min(words.len().saturating_sub(VISIBLE_OFFSET));
    words[VISIBLE_OFFSET..VISIBLE_OFFSET + count]
        .iter()
        .copied()
}
