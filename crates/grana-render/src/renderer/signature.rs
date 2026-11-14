use super::*;
use grana_display::CameraState;

#[derive(Clone, Copy)]
pub(super) struct Signature<'a> {
    pub(super) data: &'a FrameData,
    pub(super) camera: CameraState,
    pub(super) region: [u32; 4],
    pub(super) levels: Levels,
    pub(super) versions: Versions,
    pub(super) channels: u64,
    pub(super) generation: u64,
    pub(super) time: u32,
    pub(super) exposure: f32,
    pub(super) epoch: u64,
    pub(super) size: [u32; 2],
    pub(super) overlay: u64,
}

impl Signature<'_> {
    pub(super) fn digest(self) -> u64 {
        let Self {
            data,
            camera,
            region,
            levels,
            versions,
            channels,
            generation,
            time,
            exposure,
            epoch,
            size,
            overlay,
        } = self;
        let mut hasher = DefaultHasher::new();
        let mut data = *data;
        data.jitter = [0.0; 4];
        data.lens = [0.0, 0.0, data.lens[2], data.lens[3]];
        bytemuck::bytes_of(&data).hash(&mut hasher);
        camera.projection.index().hash(&mut hasher);
        region.hash(&mut hasher);
        levels.ladder.base.hash(&mut hasher);
        levels.ladder.count.hash(&mut hasher);
        levels.capacity.hash(&mut hasher);
        exposure.to_bits().hash(&mut hasher);
        time.hash(&mut hasher);
        epoch.hash(&mut hasher);
        size.hash(&mut hasher);
        versions.data().hash(&mut hasher);
        versions.objects().hash(&mut hasher);
        versions.style().hash(&mut hasher);
        channels.hash(&mut hasher);
        generation.hash(&mut hasher);
        overlay.hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn shot(self) -> u64 {
        let Self {
            data,
            region,
            versions,
            channels,
            exposure,
            epoch,
            size,
            overlay,
            ..
        } = self;
        let mut hasher = DefaultHasher::new();
        region.hash(&mut hasher);
        data.clip.map(f32::to_bits).hash(&mut hasher);
        data.slice.map(f32::to_bits).hash(&mut hasher);
        data.slice_view.map(f32::to_bits).hash(&mut hasher);
        data.region_center.map(f32::to_bits).hash(&mut hasher);
        for axis in data.region_axis {
            axis.map(f32::to_bits).hash(&mut hasher);
        }
        exposure.to_bits().hash(&mut hasher);
        epoch.hash(&mut hasher);
        size.hash(&mut hasher);
        versions.data().hash(&mut hasher);
        versions.objects().hash(&mut hasher);
        versions.style().hash(&mut hasher);
        channels.hash(&mut hasher);
        overlay.hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn transition(self) -> u64 {
        let Self {
            data,
            camera,
            region,
            versions,
            channels,
            time,
            exposure,
            epoch,
            size,
            overlay,
            ..
        } = self;
        let mut hasher = DefaultHasher::new();
        hash_camera(camera, &mut hasher);
        data.light.map(f32::to_bits).hash(&mut hasher);
        data.region_center.map(f32::to_bits).hash(&mut hasher);
        for axis in data.region_axis {
            axis.map(f32::to_bits).hash(&mut hasher);
        }
        data.shading.map(f32::to_bits).hash(&mut hasher);
        for value in &data.march[1..] {
            value.to_bits().hash(&mut hasher);
        }
        data.clip.map(f32::to_bits).hash(&mut hasher);
        data.tag.hash(&mut hasher);
        region.hash(&mut hasher);
        exposure.to_bits().hash(&mut hasher);
        time.hash(&mut hasher);
        epoch.hash(&mut hasher);
        size.hash(&mut hasher);
        versions.data().hash(&mut hasher);
        versions.objects().hash(&mut hasher);
        versions.style().hash(&mut hasher);
        channels.hash(&mut hasher);
        overlay.hash(&mut hasher);
        hasher.finish()
    }
}

pub(super) fn hash_camera(camera: CameraState, hasher: &mut DefaultHasher) {
    camera.pivot.map(f32::to_bits).hash(hasher);
    camera.yaw.to_bits().hash(hasher);
    camera.pitch.to_bits().hash(hasher);
    camera.fov_y.to_bits().hash(hasher);
    camera.projection.index().hash(hasher);
    camera.aperture.to_bits().hash(hasher);
    camera.focus.to_bits().hash(hasher);
}

pub(super) fn digest(words: &[u64]) -> u64 {
    let mut hasher = DefaultHasher::new();
    words.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShadowKeys {
    volume: u64,
    objects: u64,
}

impl ShadowKeys {
    pub(super) fn of(
        objects: u64,
        generation: u64,
        time: u32,
        style: u64,
        ladder: Ladder,
        space: &ShadowSpace,
    ) -> Self {
        let space = space.digest();
        Self {
            volume: digest(&[
                style,
                ladder.base as u64,
                ladder.count as u64,
                u64::from(time),
                generation,
                space,
            ]),
            objects: digest(&[objects, space]),
        }
    }

    pub(super) fn volume(self) -> u64 {
        self.volume
    }

    pub(super) fn objects(self) -> u64 {
        self.objects
    }
}

pub(super) fn shadow_bounds(scene: &SceneFrame<'_>, world: Vec3) -> (Vec3, Vec3) {
    let mut low = -0.5 * world;
    let mut high = 0.5 * world;
    for layer in scene.layers {
        for (object, group) in layer.mesh.groups().iter().enumerate() {
            if !scene.styles[layer.style_start + object].visible() {
                continue;
            }
            let object_low = Vec3::from(group.min());
            let object_high = object_low + UNIT * Vec3::from(group.scale());
            low = low.min(object_low);
            high = high.max(object_high);
        }
    }
    (low, high)
}

pub(super) fn channel_style(channels: &[crate::ChannelRow]) -> u64 {
    let mut key = channels.len() as u64;
    for channel in channels {
        let mut row = (channel.visible as u64) ^ ((channel.opacity.to_bits() as u64) << 1);
        row = row.rotate_left(17) ^ (channel.window.min().to_bits() as u64);
        row = row.rotate_left(17) ^ (channel.window.max().to_bits() as u64);
        row = row.rotate_left(17) ^ ((channel.volume.mode().index() as u64) << 3);
        row = row.rotate_left(17) ^ (channel.volume.isovalue().to_bits() as u64);
        for stop in channel.transfer.stops() {
            row = row.rotate_left(11) ^ (stop.position().to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.opacity().to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.color()[0].to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.color()[1].to_bits() as u64);
            row = row.rotate_left(11) ^ (stop.color()[2].to_bits() as u64);
        }
        key = key.rotate_left(13) ^ row;
    }
    key
}
