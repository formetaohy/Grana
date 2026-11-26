use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use grana_volume::{Clip, Region};

use crate::brick::BrickTable;
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
    pub dims: [f32; 4],
    pub grids: [u32; 4],
    pub origin: [f32; 4],
    pub axes: [[f32; 4]; 3],
    pub inverses: [[f32; 4]; 3],
    pub base: [u32; 4],
    pub projection: [f32; 4],
    pub lens: [f32; 4],
    pub sampling: [f32; 4],
    pub slice: [f32; 4],
    pub slice_view: [f32; 4],
    pub occupancy: [f32; 4],
    pub bounds: [u32; 4],
    pub packs: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ScheduleData {
    pub time: u32,
    pub capacity: u32,
    pub bucket_scale: f32,
    pub base: u32,
}

const _: () = {
    assert!(size_of::<FrameData>() == 928);
    assert!(std::mem::offset_of!(FrameData, region_axis) == 224);
    assert!(std::mem::offset_of!(FrameData, planes) == 352);
    assert!(std::mem::offset_of!(FrameData, light_view) == 448);
    assert!(std::mem::offset_of!(FrameData, dims) == 640);
    assert!(std::mem::offset_of!(FrameData, grids) == 656);
    assert!(std::mem::offset_of!(FrameData, origin) == 672);
    assert!(std::mem::offset_of!(FrameData, axes) == 688);
    assert!(std::mem::offset_of!(FrameData, inverses) == 736);
    assert!(std::mem::offset_of!(FrameData, base) == 784);
    assert!(std::mem::offset_of!(FrameData, projection) == 800);
    assert!(std::mem::offset_of!(FrameData, lens) == 816);
    assert!(std::mem::offset_of!(FrameData, sampling) == 832);
    assert!(std::mem::offset_of!(FrameData, slice) == 848);
    assert!(std::mem::offset_of!(FrameData, slice_view) == 864);
    assert!(std::mem::offset_of!(FrameData, occupancy) == 880);
    assert!(std::mem::offset_of!(FrameData, bounds) == 896);
    assert!(std::mem::offset_of!(FrameData, packs) == 912);
    assert!(size_of::<ScheduleData>() == 16);
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

impl FrameData {
    pub fn new(
        view: VolumeView<'_>,
        table: &BrickTable,
        shading: Shading,
        ray: Ray,
        space: ShadowSpace,
    ) -> Self {
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
        let first = table;
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
        let orthographic = camera.projection().orthographic();
        let half = camera.half_height();
        let (half_width, half_height) = if orthographic {
            (half * aspect, half)
        } else {
            let tan = (0.5 * camera.fov_y()).tan();
            (tan * aspect, tan)
        };
        let eye = camera.eye();
        let light = shading.light();
        let counts = first.grid.counts();
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
            march: [ray.step, shading.shadows, ray.channels as f32, 0.0],
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
            dims: [first.dims.x, first.dims.y, first.dims.z, 0.0],
            grids: [counts[0], counts[1], counts[2], 0],
            origin: [first.origin.x, first.origin.y, first.origin.z, 0.0],
            axes: std::array::from_fn(|axis| first.axes[axis].extend(0.0).to_array()),
            inverses: std::array::from_fn(|axis| first.inverses[axis].extend(0.0).to_array()),
            base: [first.record_base(), 0, 0, 0],
            projection: [
                f32::from(u8::from(orthographic)),
                0.0,
                half_width,
                half_height,
            ],
            lens: [0.0, 0.0, camera.focus(), camera.aperture()],
            sampling: [pixel_scale, pixel_scale, 0.0, 0.0],
            slice,
            slice_view,
            occupancy: [1.0, 0.0, 0.0, 0.0],
            bounds: ray.bounds,
            packs: [0; 4],
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

pub(super) fn reach(camera: &Camera, table: &BrickTable) -> f32 {
    let eye = camera.eye();
    let mut far = (camera.pivot() - eye).length();
    let (low, high) = table.bounds;
    for corner in 0..8 {
        let point = Vec3::new(
            if corner & 1 == 0 { low.x } else { high.x },
            if corner & 2 == 0 { low.y } else { high.y },
            if corner & 4 == 0 { low.z } else { high.z },
        );
        far = far.max((point - eye).length());
    }
    let far = far.max(1.0e-3);
    assert!(
        far > 0.0 && far.is_finite(),
        "a frame names a positive depth reach"
    );
    far * REACH_MARGIN
}

pub(super) const REACH_MARGIN: f32 = 1.25;

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

pub(super) const SAMPLE_CEILING: u32 = 8192;

pub(super) fn sample_bounds(
    camera: &Camera,
    table: &BrickTable,
    step: f32,
    step_scale: f32,
    height: u32,
) -> Result<[u32; 4], String> {
    let reach = camera.forward().abs();
    let chord = table.world.x * reach.x + table.world.y * reach.y + table.world.z * reach.z;
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
    let samples = (chord / minimum).ceil() as u32 + 2;
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
