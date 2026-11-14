use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use grana_volume::{Clip, Region};

use crate::brick::BrickTable;
use crate::planner::{BAND, Ladder};
use crate::shadow::ShadowSpace;
use grana_display::{Camera, Shading};

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
    pub slice: [f32; 4],
    pub slice_view: [f32; 4],
    pub occupancy: [f32; 4],
    pub bounds: [u32; 4],
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
    assert!(size_of::<FrameData>() == 1376);
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
    assert!(std::mem::offset_of!(FrameData, slice) == 1312);
    assert!(std::mem::offset_of!(FrameData, slice_view) == 1328);
    assert!(std::mem::offset_of!(FrameData, occupancy) == 1344);
    assert!(size_of::<ScheduleData>() == 48);
};

pub(crate) struct VolumeView<'a> {
    pub camera: &'a Camera,
    pub region: Region,
    pub clip: Clip,
    pub aspect: f32,
    pub height: u32,
    pub time: u32,
}

pub(crate) struct Ray {
    pub(crate) step: f32,
    pub(crate) bounds: [u32; 4],
    pub(crate) channels: usize,
}

const UNREACHED: f32 = 1.0e19;

impl FrameData {
    pub fn new(
        view: VolumeView<'_>,
        ladder: Ladder,
        tables: &[&BrickTable],
        shading: Shading,
        ray: Ray,
        space: ShadowSpace,
    ) -> Self {
        assert_eq!(
            tables.len(),
            ladder.count,
            "the ladder carries one table per band"
        );
        assert!(
            ray.step.is_finite() && ray.step > 0.0,
            "a frame marches at a positive step"
        );
        assert!(
            ray.bounds[0] > 0 && ray.bounds[1] >= ray.bounds[0],
            "a frame carries a usable ray bound"
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
        let (slice, slice_view) = match camera.slice() {
            Some(frame) => {
                let plane = frame.plane(camera.pivot());
                let normal = plane.normal();
                (
                    [normal.x, normal.y, normal.z, plane.offset()],
                    [0.5 * plane.thickness(), 1.0, 0.0, 0.0],
                )
            }
            None => ([0.0; 4], [0.0; 4]),
        };
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
            march: [ray.step, shading.shadows, ray.channels as f32, pixel_scale],
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
            slice,
            slice_view,
            occupancy: [1.0, 0.0, 0.0, 0.0],
            bounds: ray.bounds,
        }
    }
}

pub(crate) fn lens(camera: &Camera, index: u32, samples: u32, aspect: f32) -> [f32; 4] {
    assert!(aspect > 0.0, "a view has a positive aspect");
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

pub(super) fn step_length(camera: &Camera, table: &BrickTable, step_scale: f32) -> f32 {
    let direction = camera.forward();
    let mut voxel_world = f32::INFINITY;
    for axis in 0..3 {
        let reach = direction[axis].abs();
        if reach > 1.0e-6 {
            voxel_world = voxel_world.min(table.voxel[axis] / reach);
        }
    }
    let voxel_world = if voxel_world.is_finite() {
        voxel_world
    } else {
        table.voxel.min_element()
    };
    (voxel_world * step_scale.max(1.0e-3)).max(1.0e-7)
}

pub(super) const SAMPLE_CEILING: u32 = 4096;

pub(super) fn sample_bounds(
    camera: &Camera,
    table: &BrickTable,
    step: f32,
    step_scale: f32,
    height: u32,
) -> Result<[u32; 4], String> {
    let diagonal = table.world.length();
    let focal = camera.pixels_per_world(height).max(1.0e-6);
    let nearest = if camera.projection().orthographic() {
        0.0
    } else {
        let centre = 0.5 * (table.bounds.0 + table.bounds.1);
        let half = 0.5 * (table.bounds.1 - table.bounds.0);
        ((camera.eye() - centre).abs() - half)
            .max(Vec3::ZERO)
            .length()
    };
    let pixel = 0.5 * step_scale.max(1.0e-3) * nearest / focal;
    let minimum = step.max(pixel);
    let samples = (diagonal / minimum).ceil() as u32 + 2;
    if samples > SAMPLE_CEILING {
        return Err(format!(
            "a ray crosses {samples} samples at this step, above the {SAMPLE_CEILING} sample ceiling; raise the step scale or zoom out"
        ));
    }
    Ok([samples, samples.saturating_mul(4).saturating_add(8), 0, 0])
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
