use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use grana_volume::{Clip, Region};

use crate::brick::BrickTable;
use crate::shadow::ShadowSpace;
use grana_display::Shading;
use grana_display::{Camera, SectionCamera};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct FrameData {
    pub view_projection: [[f32; 4]; 4],
    pub inv_view_projection: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub light: [f32; 4],
    pub right: [f32; 4],
    pub up: [f32; 4],
    pub forward: [f32; 4],
    pub box_min: [f32; 4],
    pub box_size: [f32; 4],
    pub region_min: [f32; 4],
    pub region_max: [f32; 4],
    pub dims: [f32; 4],
    pub grid: [u32; 4],
    pub atlas: [u32; 4],
    pub shading: [f32; 4],
    pub march: [f32; 4],
    pub viewport: [f32; 4],
    pub jitter: [f32; 4],
    pub planes: [[f32; 4]; 6],
    pub light_view: [[f32; 4]; 4],
    pub light_inverse: [[f32; 4]; 4],
    pub light_reach: [f32; 4],
    pub oit: [u32; 4],
    pub clip: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct SectionData {
    pub uv_center: [f32; 4],
    pub uv_scale: [f32; 4],
    pub plane: [f32; 4],
    pub jitter: [f32; 4],
    pub mapping: [u32; 4],
    pub box_min: [f32; 4],
    pub world: [f32; 4],
    pub dims: [f32; 4],
    pub grid: [u32; 4],
    pub atlas: [u32; 4],
    pub axes: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ScheduleData {
    pub time: u32,
    pub level: u32,
    pub capacity: u32,
    pub layer: u32,
    pub u0: u32,
    pub v0: u32,
    pub u_count: u32,
    pub v_count: u32,
    pub bucket_scale: f32,
    pub reserved: [u32; 3],
}

const _: () = {
    assert!(size_of::<FrameData>() == 656);
    assert!(std::mem::offset_of!(FrameData, planes) == 384);
    assert!(std::mem::offset_of!(FrameData, light_view) == 480);
    assert!(size_of::<SectionData>() == 176);
    assert!(size_of::<ScheduleData>() == 48);
};
pub(crate) struct VolumeView<'a> {
    pub camera: &'a Camera,
    pub region: Region,
    pub clip: Clip,
    pub aspect: f32,
    pub height: u32,
}

impl FrameData {
    pub fn new(
        view: VolumeView<'_>,
        table: &BrickTable,
        shading: Shading,
        step_scale: f32,
        channels: usize,
        space: ShadowSpace,
    ) -> Self {
        let VolumeView {
            camera,
            region,
            clip,
            aspect,
            height,
        } = view;
        let view_projection = camera.view_projection(aspect, 0.5 * table.world.length());
        let world = table.world;
        let grid = table.grid.counts();
        let step = step_length(camera, table, step_scale, height as f32);
        let pixel_scale = 0.5 * height as f32 / (0.5 * camera.fov_y()).tan();
        let eye = camera.eye();
        let light = camera.light_direction();
        Self {
            view_projection: view_projection.to_cols_array_2d(),
            inv_view_projection: view_projection.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 0.0],
            light: [light.x, light.y, light.z, 0.0],
            right: camera.right().extend(0.0).to_array(),
            up: camera.up().extend(0.0).to_array(),
            forward: camera.forward().extend(0.0).to_array(),
            box_min: [-0.5 * world.x, -0.5 * world.y, -0.5 * world.z, 0.0],
            box_size: [world.x, world.y, world.z, 0.0],
            region_min: [
                region.min().x,
                region.min().y,
                region.min().z,
                if region.enabled() { 1.0 } else { 0.0 },
            ],
            region_max: [region.max().x, region.max().y, region.max().z, 0.0],
            dims: [table.dims.x, table.dims.y, table.dims.z, 0.0],
            grid: [grid[0], grid[1], grid[2], 0],
            atlas: [table.atlas[0], table.atlas[1], table.atlas[2], 0],
            shading: [
                shading.ambient,
                shading.diffuse,
                shading.specular,
                shading.shininess,
            ],
            march: [step, shading.shadows, channels as f32, pixel_scale],
            viewport: [0.0; 4],
            jitter: [0.0; 4],
            planes: planes(view_projection),
            light_view: space.view().to_cols_array_2d(),
            light_inverse: space.inverse().to_cols_array_2d(),
            light_reach: space.reach(),
            oit: [0; 4],
            clip: {
                let (normal, offset) = clip.parts();
                [normal[0], normal[1], normal[2], offset]
            },
        }
    }
}

impl SectionData {
    pub fn new(
        camera: &Camera,
        section: &SectionCamera,
        aspect: f32,
        table: &BrickTable,
        channels: usize,
    ) -> Self {
        let axis = section.axis();
        let (u_axis, v_axis) = axis.in_plane();
        let world = table.world;
        let dims = table.dims;
        let focus = camera.focus();
        let plane_index = axis.index();
        let layer = ((focus[plane_index] / world[plane_index] + 0.5) * dims[plane_index])
            .floor()
            .clamp(0.0, dims[plane_index] - 1.0);
        let plane = (layer + 0.5) / dims[plane_index];
        let uv_center = [
            focus[u_axis.index()] / world[u_axis.index()] + 0.5,
            focus[v_axis.index()] / world[v_axis.index()] + 0.5,
            0.0,
            0.0,
        ];
        let uv_scale = [
            section.span() * aspect / (2.0 * world[u_axis.index()]),
            section.span() / (2.0 * world[v_axis.index()]),
            0.0,
            0.0,
        ];
        let grid = table.grid.counts();
        Self {
            uv_center,
            uv_scale,
            plane: [plane, 0.0, 0.0, 0.0],
            jitter: [0.0; 4],
            mapping: [
                u_axis.index() as u32,
                v_axis.index() as u32,
                plane_index as u32,
                0,
            ],
            box_min: [-0.5 * world.x, -0.5 * world.y, -0.5 * world.z, 0.0],
            world: [world.x, world.y, world.z, 0.0],
            dims: [dims.x, dims.y, dims.z, 0.0],
            grid: [grid[0], grid[1], grid[2], 0],
            atlas: [table.atlas[0], table.atlas[1], table.atlas[2], 0],
            axes: [channels as u32, 0, 0, 0],
        }
    }
}

fn step_length(camera: &Camera, table: &BrickTable, step_scale: f32, height: f32) -> f32 {
    let pixel_world = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    let direction = camera.forward() / table.world;
    let mut voxel_world = f32::INFINITY;
    for axis in 0..3 {
        let rate = direction[axis].abs();
        if rate > 1.0e-6 {
            voxel_world = voxel_world.min((1.0 / table.dims[axis]) / rate);
        }
    }
    let voxel_world = if voxel_world.is_finite() {
        voxel_world
    } else {
        table.voxel.min_element()
    };
    (pixel_world * 0.5)
        .max(voxel_world * step_scale.max(1.0e-3))
        .max(1.0e-7)
}

fn planes(view_projection: Mat4) -> [[f32; 4]; 6] {
    let row0 = view_projection.row(0);
    let row1 = view_projection.row(1);
    let row2 = view_projection.row(2);
    let row3 = view_projection.row(3);
    [
        row3 + row0,
        row3 - row0,
        row3 + row1,
        row3 - row1,
        row2,
        row3 - row2,
    ]
    .map(|plane| {
        let normal = plane.truncate().normalize();
        let bias = plane.w / plane.truncate().length();
        [normal.x, normal.y, normal.z, bias]
    })
}
