use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use grana_display::Camera;
use grana_volume::Region;

use super::signature::hash_camera;

#[derive(Clone, Copy)]
pub(super) struct View {
    pub(super) region: [u32; 4],
    pub(super) height: u32,
    pub(super) aspect: f32,
    pub(super) motion: u64,
}

impl View {
    pub(super) fn new(
        pixel: [u32; 4],
        surface: [u32; 2],
        camera: &Camera,
        clip: Region,
        time: u32,
    ) -> Self {
        assert!(inside(pixel, surface), "the view lies on the surface");
        let width = pixel[2].max(1);
        let height = pixel[3].max(1);
        Self {
            region: pixel,
            height,
            aspect: width as f32 / height as f32,
            motion: motion(camera, pixel, clip, time),
        }
    }
}

fn inside(region: [u32; 4], size: [u32; 2]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0] + region[2] <= size[0]
        && region[1] + region[3] <= size[1]
}

fn motion(camera: &Camera, region: [u32; 4], clip: Region, time: u32) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_camera(camera.state(), &mut hasher);
    let aspect = region[2].max(1) as f32 / region[3].max(1) as f32;
    aspect.to_bits().hash(&mut hasher);
    region.hash(&mut hasher);
    time.hash(&mut hasher);
    clip.enabled().hash(&mut hasher);
    clip.center().to_array().map(f32::to_bits).hash(&mut hasher);
    clip.half().to_array().map(f32::to_bits).hash(&mut hasher);
    for axis in 0..3 {
        clip.axis(axis)
            .to_array()
            .map(f32::to_bits)
            .hash(&mut hasher);
    }
    hasher.finish()
}
