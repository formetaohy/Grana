use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use grana_volume::{Clip, Region};

use crate::brick::BrickTable;
use crate::planner::{BAND, Ladder};
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
    pub region_center: [f32; 4],
    pub region_axis: [[f32; 4]; 3],
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
    pub tag: [u32; 4],
    pub dims: [[f32; 4]; BAND],
    pub grids: [[u32; 4]; BAND],
    pub origins: [[f32; 4]; BAND],
    pub axes: [[f32; 4]; BAND * 3],
    pub inverses: [[f32; 4]; BAND * 3],
    pub bases: [u32; 4],
    pub bands: [u32; 4],
    pub edge: [f32; 4],
    pub projection: [f32; 4],
    pub lens: [f32; 4],
    pub sampling: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct SectionData {
    pub center: [f32; 4],
    pub plane: [f32; 4],
    pub right: [f32; 4],
    pub up: [f32; 4],
    pub jitter: [f32; 4],
    pub origin: [f32; 4],
    pub axes: [[f32; 4]; 3],
    pub inverses: [[f32; 4]; 3],
    pub dims: [f32; 4],
    pub grid: [u32; 4],
    pub atlas: [u32; 4],
    pub channels: [u32; 4],
    pub bases: [u32; 4],
    pub fallback_origin: [f32; 4],
    pub fallback_axes: [[f32; 4]; 3],
    pub fallback_inverses: [[f32; 4]; 3],
    pub fallback_dims: [f32; 4],
    pub fallback_grid: [u32; 4],
    pub projection: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ScheduleData {
    pub time: u32,
    pub band: u32,
    pub capacity: u32,
    pub x0: u32,
    pub y0: u32,
    pub z0: u32,
    pub x_count: u32,
    pub y_count: u32,
    pub bucket_scale: f32,
    pub base: u32,
    pub z_count: u32,
    pub reserved: u32,
}

const _: () = {
    assert!(size_of::<FrameData>() == 1312);
    assert!(std::mem::offset_of!(FrameData, region_axis) == 224);
    assert!(std::mem::offset_of!(FrameData, planes) == 352);
    assert!(std::mem::offset_of!(FrameData, light_view) == 448);
    assert!(std::mem::offset_of!(FrameData, dims) == 640);
    assert!(std::mem::offset_of!(FrameData, origins) == 768);
    assert!(std::mem::offset_of!(FrameData, axes) == 832);
    assert!(std::mem::offset_of!(FrameData, inverses) == 1024);
    assert!(std::mem::offset_of!(FrameData, bases) == 1216);
    assert!(std::mem::offset_of!(FrameData, bands) == 1232);
    assert!(std::mem::offset_of!(FrameData, edge) == 1248);
    assert!(std::mem::offset_of!(FrameData, projection) == 1264);
    assert!(std::mem::offset_of!(FrameData, lens) == 1280);
    assert!(std::mem::offset_of!(FrameData, sampling) == 1296);
    assert!(size_of::<SectionData>() == 432);
    assert!(size_of::<ScheduleData>() == 48);
};

pub(crate) struct SharedView {
    pub shading: Shading,
    pub channels: usize,
    pub region: Region,
    pub clip: Clip,
    pub time: u32,
    pub space: ShadowSpace,
}

pub(crate) struct VolumeView<'a> {
    pub camera: &'a Camera,
    pub region: Region,
    pub clip: Clip,
    pub aspect: f32,
    pub height: u32,
    pub time: u32,
}

const UNREACHED: f32 = 1.0e19;

impl FrameData {
    pub fn shared(view: SharedView) -> Self {
        let SharedView {
            shading,
            channels,
            region,
            clip,
            time,
            space,
        } = view;
        let light = shading.light();
        let (normal, offset) = clip.parts();
        Self {
            view_projection: [[0.0; 4]; 4],
            inv_view_projection: [[0.0; 4]; 4],
            eye: [0.0; 4],
            light: [light.x, light.y, light.z, 0.0],
            right: [0.0; 4],
            up: [0.0; 4],
            forward: [0.0; 4],
            region_center: [
                region.center().x,
                region.center().y,
                region.center().z,
                if region.enabled() { 1.0 } else { 0.0 },
            ],
            region_axis: std::array::from_fn(|index| {
                let axis = region.axis(index);
                [axis.x, axis.y, axis.z, region.half()[index]]
            }),
            atlas: [0; 4],
            shading: [shading.ambient, shading.diffuse, 0.0, 0.0],
            march: [0.0, shading.shadows, channels as f32, 0.0],
            viewport: [0.0; 4],
            jitter: [0.0; 4],
            planes: [[0.0; 4]; 6],
            light_view: space.view().to_cols_array_2d(),
            light_inverse: space.inverse().to_cols_array_2d(),
            light_reach: space.reach(),
            oit: [0; 4],
            clip: [normal[0], normal[1], normal[2], offset],
            tag: [time, 0, 0, 0],
            dims: [[0.0; 4]; BAND],
            grids: [[0; 4]; BAND],
            origins: [[0.0; 4]; BAND],
            axes: [[0.0; 4]; BAND * 3],
            inverses: [[0.0; 4]; BAND * 3],
            bases: [0; 4],
            bands: [0; 4],
            edge: [UNREACHED; 4],
            projection: [1.0, 0.0, 0.0, 0.0],
            lens: [0.0, 0.0, 1.0, 0.0],
            sampling: [1.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn new(
        view: VolumeView<'_>,
        ladder: Ladder,
        tables: &[&BrickTable],
        shading: Shading,
        step_scale: f32,
        channels: usize,
        space: ShadowSpace,
    ) -> Self {
        assert_eq!(
            tables.len(),
            ladder.count,
            "the ladder carries one table per band"
        );
        let VolumeView {
            camera,
            region,
            clip,
            aspect,
            height,
            time,
        } = view;
        let first = tables[0];
        let view_projection = camera.view_projection(aspect, 0.5 * first.world.length());
        let step = step_length(camera, first, step_scale);
        let pixel_scale = camera.pixels_per_world(height);
        let sampling = pixel_scale;
        let orthographic = camera.projection().orthographic();
        let half = camera.half_height();
        let (half_width, half_height) = if orthographic {
            (half * aspect, half)
        } else {
            (0.0, 0.0)
        };
        let eye = camera.eye();
        let light = shading.light();
        let mut dims = [[0.0; 4]; BAND];
        let mut grids = [[0; 4]; BAND];
        let mut origins = [[0.0; 4]; BAND];
        let mut axes = [[0.0; 4]; BAND * 3];
        let mut inverses = [[0.0; 4]; BAND * 3];
        let mut bases = [0; 4];
        let mut edge = [UNREACHED; 4];
        for (band, table) in tables.iter().enumerate() {
            let counts = table.grid.counts();
            dims[band] = [table.dims.x, table.dims.y, table.dims.z, 0.0];
            grids[band] = [counts[0], counts[1], counts[2], 0];
            origins[band] = [table.origin.x, table.origin.y, table.origin.z, 0.0];
            for axis in 0..3 {
                axes[band * 3 + axis] = table.axes[axis].extend(0.0).to_array();
                inverses[band * 3 + axis] = table.inverses[axis].extend(0.0).to_array();
            }
            bases[band] = table.record_base();
            if band > 0 && !orthographic {
                edge[band - 1] = table.voxel.min_element() * sampling;
            }
        }
        Self {
            view_projection: view_projection.to_cols_array_2d(),
            inv_view_projection: view_projection.inverse().to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 0.0],
            light: [light.x, light.y, light.z, 0.0],
            right: camera.right().extend(0.0).to_array(),
            up: camera.up().extend(0.0).to_array(),
            forward: camera.forward().extend(0.0).to_array(),
            region_center: [
                region.center().x,
                region.center().y,
                region.center().z,
                if region.enabled() { 1.0 } else { 0.0 },
            ],
            region_axis: std::array::from_fn(|index| {
                let axis = region.axis(index);
                [axis.x, axis.y, axis.z, region.half()[index]]
            }),
            atlas: [first.atlas[0], first.atlas[1], first.atlas[2], 0],
            shading: [shading.ambient, shading.diffuse, 0.0, 0.0],
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
            tag: [time, 0, 0, 0],
            dims,
            grids,
            origins,
            axes,
            inverses,
            bases,
            bands: [ladder.count as u32, 0, 0, 0],
            edge,
            projection: [
                f32::from(u8::from(orthographic)),
                0.0,
                half_width,
                half_height,
            ],
            lens: [0.0, 0.0, camera.focus(), camera.aperture()],
            sampling: [sampling, 0.0, 0.0, 0.0],
        }
    }
}

pub(crate) fn lens(camera: &Camera, index: u32, samples: u32, aspect: f32) -> [f32; 4] {
    assert!(aspect > 0.0, "a volume pane has a positive aspect");
    let focus = camera.focus();
    if !camera.focused() {
        return [0.0, 0.0, focus, 0.0];
    }
    let offset = camera.lens(index, samples);
    let focal = 1.0 / (0.5 * camera.fov_y()).tan();
    [
        offset.x * focal / aspect,
        offset.y * focal,
        focus,
        camera.aperture(),
    ]
}

impl SectionData {
    pub fn new(
        section: &SectionCamera,
        aspect: f32,
        table: &BrickTable,
        fallback: Option<&BrickTable>,
        channels: usize,
    ) -> Self {
        assert!(aspect > 0.0, "a section pane has a positive aspect");
        let dims = table.dims;
        let center = section.center();
        let normal = section.normal();
        let right = section.right();
        let up = section.up();
        let grid = table.grid.counts();
        let fallback = fallback.filter(|fallback| fallback.level() != table.level());
        let support = fallback.unwrap_or(table);
        let support_grid = support.grid.counts();
        Self {
            center: [center.dot(right), center.dot(up), 0.0, 0.0],
            plane: [normal.x, normal.y, normal.z, center.dot(normal)],
            right: [right.x, right.y, right.z, 0.5 * section.span() * aspect],
            up: [up.x, up.y, up.z, 0.5 * section.span()],
            jitter: [0.0; 4],
            origin: [table.origin.x, table.origin.y, table.origin.z, 0.0],
            axes: std::array::from_fn(|axis| table.axes[axis].extend(0.0).to_array()),
            inverses: std::array::from_fn(|axis| table.inverses[axis].extend(0.0).to_array()),
            dims: [dims.x, dims.y, dims.z, 0.0],
            grid: [grid[0], grid[1], grid[2], 0],
            atlas: [table.atlas[0], table.atlas[1], table.atlas[2], 0],
            channels: [channels as u32, 0, 0, 0],
            bases: [table.record_base(), support.record_base(), 0, 0],
            fallback_origin: [support.origin.x, support.origin.y, support.origin.z, 0.0],
            fallback_axes: std::array::from_fn(|axis| support.axes[axis].extend(0.0).to_array()),
            fallback_inverses: std::array::from_fn(|axis| {
                support.inverses[axis].extend(0.0).to_array()
            }),
            fallback_dims: [support.dims.x, support.dims.y, support.dims.z, 0.0],
            fallback_grid: [
                support_grid[0],
                support_grid[1],
                support_grid[2],
                u32::from(fallback.is_some()),
            ],
            projection: projection(section, table),
        }
    }
}

const PROJECTION_SAMPLE_LIMIT: u32 = 96;

fn projection(section: &SectionCamera, table: &BrickTable) -> [f32; 4] {
    if !section.projected() {
        return [0.0, 0.0, 1.0, 0.0];
    }
    let half = 0.5 * section.thickness();
    let step = normal_step(table, section.normal());
    let samples = ((2.0 * half / step).ceil() as u32).clamp(1, PROJECTION_SAMPLE_LIMIT);
    [
        section.projection().index() as f32,
        half,
        samples as f32,
        0.0,
    ]
}

fn normal_step(table: &BrickTable, normal: Vec3) -> f32 {
    let rate = table.rate(normal).abs();
    let mut step = f32::INFINITY;
    for rate in rate.to_array() {
        if rate > 1.0e-6 {
            step = step.min(1.0 / rate);
        }
    }
    if step.is_finite() {
        step
    } else {
        table.voxel.min_element()
    }
}

fn step_length(camera: &Camera, table: &BrickTable, step_scale: f32) -> f32 {
    let rate = table.rate(camera.forward());
    let mut voxel_world = f32::INFINITY;
    for axis in 0..3 {
        let rate = rate[axis].abs();
        if rate > 1.0e-6 {
            voxel_world = voxel_world.min((1.0 / table.dims[axis]) / rate);
        }
    }
    let voxel_world = if voxel_world.is_finite() {
        voxel_world
    } else {
        table.voxel.min_element()
    };
    (voxel_world * step_scale.max(1.0e-3)).max(1.0e-7)
}

pub(crate) fn planes(view_projection: Mat4) -> [[f32; 4]; 6] {
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
