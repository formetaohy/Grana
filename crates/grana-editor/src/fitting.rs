use glam::Vec3;
use grana_display::{Camera, Slice};
use grana_scene::Scene;

pub fn fit_camera(scene: &Scene, camera: &mut Camera, aspect: f32) {
    let Some((axis, index, _, _)) = slice_position(scene, camera) else {
        camera.fit(scene.dataset.world_size(), aspect);
        return;
    };
    let level = scene.dataset.pyramid().level(0);
    let frame = level.slice_frame(axis);
    camera.fit_slice(
        Vec3::from(level.slice_center(axis, index)),
        frame.half_width(),
        frame.half_height(),
        aspect,
    );
}

pub fn enter_slice(scene: &Scene, camera: &mut Camera, axis: usize, aspect: f32) {
    let level = scene.dataset.pyramid().level(0);
    let frame = level.slice_frame(axis);
    let kept = camera.slice().filter(|slice| slice.axis() == axis);
    let index = if kept.is_some() {
        level.slice_index(axis, camera.pivot().to_array())
    } else {
        frame.count() / 2
    };
    let center = Vec3::from(level.slice_center(axis, index));
    let previous = camera.pivot();
    let offset = kept.map_or(Vec3::ZERO, |slice| in_plane(previous, center, slice));
    camera.set_slice(Slice::new(axis, frame.right(), frame.up(), frame.step()));
    camera.fit_slice(
        center + offset,
        frame.half_width(),
        frame.half_height(),
        aspect,
    );
}

pub fn set_slice_depth(scene: &Scene, camera: &mut Camera, at: f32) {
    let Some(slice) = camera.slice() else {
        return;
    };
    let level = scene.dataset.pyramid().level(0);
    let frame = level.slice_frame(slice.axis());
    let last = frame.count().saturating_sub(1);
    if last == 0 {
        return;
    }
    let index = (at.clamp(0.0, 1.0) * last as f32).round() as u32;
    let center = Vec3::from(level.slice_center(slice.axis(), index));
    camera.place_slice(center + in_plane(camera.pivot(), center, slice));
}

pub fn slice_step(scene: &Scene, camera: &mut Camera, steps: i32) {
    let Some(slice) = camera.slice() else {
        return;
    };
    let level = scene.dataset.pyramid().level(0);
    let last = level.slice_frame(slice.axis()).count().saturating_sub(1);
    if last == 0 {
        return;
    }
    let index =
        level.slice_index(slice.axis(), camera.pivot().to_array()) as i64 + i64::from(steps);
    let index = index.clamp(0, i64::from(last)) as u32;
    let center = Vec3::from(level.slice_center(slice.axis(), index));
    camera.place_slice(center + in_plane(camera.pivot(), center, slice));
}

pub fn slice_position(scene: &Scene, camera: &Camera) -> Option<(usize, u32, u32, f32)> {
    let slice = camera.slice()?;
    let level = scene.dataset.pyramid().level(0);
    let frame = level.slice_frame(slice.axis());
    let index = level.slice_index(slice.axis(), camera.pivot().to_array());
    let last = frame.count().saturating_sub(1);
    let at = if last == 0 {
        0.0
    } else {
        index as f32 / last as f32
    };
    Some((slice.axis(), index, frame.count(), at))
}

fn in_plane(pivot: Vec3, center: Vec3, slice: Slice) -> Vec3 {
    let offset = pivot - center;
    slice.right() * offset.dot(slice.right()) + slice.up() * offset.dot(slice.up())
}
