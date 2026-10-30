use bytemuck::{Pod, Zeroable};
use grana_volume::Pyramid;

use crate::atlas::Atlas;
use crate::level::LevelCache;
use crate::params::RenderParams;
use crate::{Camera, SectionCamera};

const MAX_STEPS: f32 = 4096.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct Uniforms {
    inv_view_projection: [[f32; 4]; 4],
    eye: [f32; 4],
    box_min: [f32; 4],
    box_size: [f32; 4],
    shading: [f32; 4],
    march: [f32; 4],
    light: [f32; 4],
    grid: [u32; 4],
    atlas: [u32; 4],
    dims: [f32; 4],
    channel_opacity: [[f32; 4]; 2],
    channel_window: [[f32; 4]; 2],
    channel_inverse: [[f32; 4]; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct SectionUniforms {
    uv_center: [f32; 4],
    uv_scale: [f32; 4],
    plane: [f32; 4],
    axes: [u32; 4],
    grid: [u32; 4],
    atlas: [u32; 4],
    dims: [f32; 4],
    box_min: [f32; 4],
    world: [f32; 4],
    mapping: [u32; 4],
    channel_opacity: [[f32; 4]; 2],
    channel_window: [[f32; 4]; 2],
    channel_inverse: [[f32; 4]; 2],
}

const _: () = {
    assert!(size_of::<Uniforms>() == 304);
    assert!(std::mem::offset_of!(Uniforms, channel_opacity) == 208);
    assert!(std::mem::offset_of!(Uniforms, channel_window) == 240);
    assert!(std::mem::offset_of!(Uniforms, channel_inverse) == 272);
    assert!(size_of::<SectionUniforms>() == 256);
    assert!(std::mem::offset_of!(SectionUniforms, mapping) == 144);
    assert!(std::mem::offset_of!(SectionUniforms, channel_opacity) == 160);
};

struct ChannelFields {
    opacity: [[f32; 4]; 2],
    window: [[f32; 4]; 2],
    inverse: [[f32; 4]; 2],
}

pub(crate) fn volume_uniforms(
    pyramid: &Pyramid,
    cache: &LevelCache,
    atlas: &Atlas,
    camera: &Camera,
    params: &RenderParams,
    aspect: f32,
    height: u32,
) -> Uniforms {
    let view_projection = camera.view_projection(aspect, 0.5 * cache.world.length());
    let eye = camera.eye();
    let light = camera.light_direction();
    let step = step_length(camera, cache, params.step_scale, height as f32);
    let channels = channel_fields(pyramid, params);
    let grid = cache.grid.counts();
    let atlas_grid = atlas.slot_grid();
    Uniforms {
        inv_view_projection: view_projection.inverse().to_cols_array_2d(),
        eye: [eye.x, eye.y, eye.z, 0.0],
        box_min: [
            -0.5 * cache.world.x,
            -0.5 * cache.world.y,
            -0.5 * cache.world.z,
            0.0,
        ],
        box_size: [cache.world.x, cache.world.y, cache.world.z, 0.0],
        shading: [
            params.shading.ambient,
            params.shading.diffuse,
            params.shading.specular,
            params.shading.shininess,
        ],
        march: [step, 0.0, params.channels.len() as f32, MAX_STEPS],
        light: [light.x, light.y, light.z, 0.0],
        grid: [grid[0], grid[1], grid[2], 0],
        atlas: [atlas_grid[0], atlas_grid[1], atlas_grid[2], 0],
        dims: [cache.dims.x, cache.dims.y, cache.dims.z, 0.0],
        channel_opacity: channels.opacity,
        channel_window: channels.window,
        channel_inverse: channels.inverse,
    }
}

pub(crate) fn section_uniforms(
    pyramid: &Pyramid,
    cache: &LevelCache,
    atlas: &Atlas,
    camera: &Camera,
    section: &SectionCamera,
    params: &RenderParams,
    aspect: f32,
) -> SectionUniforms {
    let axis = section.axis();
    let (u_axis, v_axis) = axis.in_plane();
    let world = cache.world;
    let dims = cache.dims;
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
    let channels = channel_fields(pyramid, params);
    let grid = cache.grid.counts();
    let atlas_grid = atlas.slot_grid();
    SectionUniforms {
        uv_center,
        uv_scale,
        plane: [plane, 0.0, 0.0, 0.0],
        axes: [plane_index as u32, params.channels.len() as u32, 0, 0],
        grid: [grid[0], grid[1], grid[2], 0],
        atlas: [atlas_grid[0], atlas_grid[1], atlas_grid[2], 0],
        dims: [dims.x, dims.y, dims.z, 0.0],
        box_min: [-0.5 * world.x, -0.5 * world.y, -0.5 * world.z, 0.0],
        world: [world.x, world.y, world.z, 0.0],
        mapping: [
            u_axis.index() as u32,
            v_axis.index() as u32,
            plane_index as u32,
            0,
        ],
        channel_opacity: channels.opacity,
        channel_window: channels.window,
        channel_inverse: channels.inverse,
    }
}

fn channel_fields(pyramid: &Pyramid, params: &RenderParams) -> ChannelFields {
    let mut fields = ChannelFields {
        opacity: [[0.0; 4]; 2],
        window: [[0.0; 4]; 2],
        inverse: [[0.0; 4]; 2],
    };
    for (index, channel) in params.channels.iter().enumerate() {
        let plane = index / 4;
        let lane = index % 4;
        fields.opacity[plane][lane] = if channel.visible {
            channel.opacity.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let data = pyramid.channels()[index].intensity();
        let span = data.span();
        if span <= 0.0 {
            continue;
        }
        let start = (channel.window.min() - data.min()) / span;
        let window_span = (channel.window.max() - channel.window.min()) / span;
        let inverse = if window_span > 0.0 {
            1.0 / window_span
        } else {
            0.0
        };
        fields.window[plane][lane] = start;
        fields.inverse[plane][lane] = inverse;
    }
    fields
}

fn step_length(camera: &Camera, cache: &LevelCache, step_scale: f32, height: f32) -> f32 {
    let pixel_world = 2.0 * camera.distance() * (camera.fov_y() * 0.5).tan() / height.max(1.0);
    let direction = camera.forward() / cache.world;
    let dims = cache.dims;
    let mut voxel_world = f32::INFINITY;
    for axis in 0..3 {
        let rate = direction[axis].abs();
        if rate > 1.0e-6 {
            voxel_world = voxel_world.min((1.0 / dims[axis]) / rate);
        }
    }
    let voxel_world = if voxel_world.is_finite() {
        voxel_world
    } else {
        cache.voxel.min_element()
    };
    (pixel_world * 0.5)
        .max(voxel_world * step_scale.max(1.0e-3))
        .max(1.0e-7)
}
