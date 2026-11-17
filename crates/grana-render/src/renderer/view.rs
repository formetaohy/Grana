use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use grana_display::Camera;
use grana_volume::Region;

use super::signature::hash_camera;

#[derive(Clone, Copy)]
pub(super) struct View {
    pub(super) surface: [u32; 4],
    pub(super) region: [u32; 4],
    pub(super) height: u32,
    pub(super) display_height: u32,
    pub(super) aspect: f32,
    pub(super) motion: u64,
}

impl View {
    pub(super) fn new(
        pixel: [u32; 4],
        surface: [u32; 2],
        resolution: [u32; 2],
        camera: &Camera,
        clip: Region,
        time: u32,
        exposure: f32,
    ) -> Self {
        assert!(inside(pixel, surface), "the view lies on the surface");
        let region = scaled(pixel, surface, resolution);
        assert!(
            inside(region, resolution),
            "the view lies on the render surface"
        );
        let width = region[2].max(1);
        let height = region[3].max(1);
        Self {
            surface: pixel,
            region,
            height,
            display_height: pixel[3].max(1),
            aspect: width as f32 / height as f32,
            motion: motion(camera, region, clip, time, exposure),
        }
    }
}

fn inside(region: [u32; 4], size: [u32; 2]) -> bool {
    region[2] > 0
        && region[3] > 0
        && region[0] + region[2] <= size[0]
        && region[1] + region[3] <= size[1]
}

fn scaled(region: [u32; 4], surface: [u32; 2], resolution: [u32; 2]) -> [u32; 4] {
    if surface == resolution {
        return region;
    }
    let scale = |value: u32, source: u32, target: u32| -> u32 {
        (u64::from(value) * u64::from(target) / u64::from(source)) as u32
    };
    let x = scale(region[0], surface[0], resolution[0]);
    let y = scale(region[1], surface[1], resolution[1]);
    let width = scale(region[2], surface[0], resolution[0])
        .max(1)
        .min(resolution[0].saturating_sub(x));
    let height = scale(region[3], surface[1], resolution[1])
        .max(1)
        .min(resolution[1].saturating_sub(y));
    [x, y, width, height]
}

fn motion(camera: &Camera, region: [u32; 4], clip: Region, time: u32, exposure: f32) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_camera(camera.state(), &mut hasher);
    let aspect = region[2].max(1) as f32 / region[3].max(1) as f32;
    aspect.to_bits().hash(&mut hasher);
    region.hash(&mut hasher);
    time.hash(&mut hasher);
    exposure.to_bits().hash(&mut hasher);
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
