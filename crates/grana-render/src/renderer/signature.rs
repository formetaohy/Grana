use super::*;
use crate::schema::{Digest, Layer, Versions};
use grana_display::CameraState;

#[derive(Clone, Copy)]
pub(super) struct Signature<'a> {
    pub(super) data: &'a FrameData,
    pub(super) region: [u32; 4],
    pub(super) versions: Versions,
    pub(super) channels: u64,
    pub(super) content: u32,
    pub(super) stamp: u64,
    pub(super) time: u32,
    pub(super) epoch: u64,
    pub(super) size: [u32; 2],
    pub(super) step: f32,
}

impl Signature<'_> {
    pub(super) fn layers(self) -> u64 {
        let Self {
            data,
            region,
            versions,
            content,
            time,
            epoch,
            size,
            ..
        } = self;
        let mut hasher = DefaultHasher::new();
        region.hash(&mut hasher);
        size.hash(&mut hasher);
        time.hash(&mut hasher);
        epoch.hash(&mut hasher);
        content.hash(&mut hasher);
        versions.topology().hash(&mut hasher);
        data.clip.map(f32::to_bits).hash(&mut hasher);
        data.slice.map(f32::to_bits).hash(&mut hasher);
        data.slice_view.map(f32::to_bits).hash(&mut hasher);
        data.region_center.map(f32::to_bits).hash(&mut hasher);
        for axis in data.region_axis {
            axis.map(f32::to_bits).hash(&mut hasher);
        }
        hasher.finish()
    }

    pub(super) fn content(self) -> u64 {
        digest(&[self.layers(), self.stamp])
    }

    pub(super) fn classification(self) -> u64 {
        let Self { data, versions, .. } = self;
        let mut hasher = DefaultHasher::new();
        versions.classification().hash(&mut hasher);
        data.march[1].to_bits().hash(&mut hasher);
        hasher.finish()
    }

    pub(super) fn appearance(self) -> u64 {
        let Self {
            data,
            versions,
            channels,
            step,
            ..
        } = self;
        let mut hasher = DefaultHasher::new();
        versions.appearance().hash(&mut hasher);
        channels.hash(&mut hasher);
        step.to_bits().hash(&mut hasher);
        data.light.map(f32::to_bits).hash(&mut hasher);
        data.light_reach.map(f32::to_bits).hash(&mut hasher);
        data.shading.map(f32::to_bits).hash(&mut hasher);
        for value in &data.march[2..] {
            value.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    }
}

pub(super) fn view(data: &FrameData) -> u64 {
    let mut hasher = DefaultHasher::new();
    for column in data.view_projection {
        column.map(f32::to_bits).hash(&mut hasher);
    }
    data.lens[2].to_bits().hash(&mut hasher);
    data.lens[3].to_bits().hash(&mut hasher);
    hasher.finish()
}

pub(super) fn hash_camera(camera: CameraState, hasher: &mut DefaultHasher) {
    camera.pivot.map(f32::to_bits).hash(hasher);
    camera.yaw.to_bits().hash(hasher);
    camera.pitch.to_bits().hash(hasher);
    camera.distance.to_bits().hash(hasher);
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

#[derive(Clone, Copy)]
pub(super) struct ShadowVolume {
    pub(super) revision: u64,
    pub(super) writes: u64,
    pub(super) time: u32,
    pub(super) style: u64,
    pub(super) level: usize,
}

impl ShadowKeys {
    pub(super) fn of(layers: u64, volume: ShadowVolume, space: &ShadowSpace) -> Self {
        let ShadowVolume {
            revision,
            writes,
            time,
            style,
            level,
        } = volume;
        let space = space.digest();
        Self {
            volume: digest(&[
                style,
                level as u64,
                u64::from(time),
                revision,
                writes,
                space,
            ]),
            objects: digest(&[layers, space]),
        }
    }

    pub(super) fn volume(self) -> u64 {
        self.volume
    }

    pub(super) fn objects(self) -> u64 {
        self.objects
    }
}

pub(super) fn shadow_visibility(layers: &[Layer]) -> u64 {
    let mut digest = Digest::new();
    for layer in layers {
        for style in layer.styles.iter() {
            digest.flag(style.visible());
        }
    }
    digest.finish()
}

#[derive(Clone, Copy)]
pub(super) struct SceneSummary {
    topology: u64,
    classification: u64,
    bounds: (Vec3, Vec3),
    visibility: u64,
}

impl SceneSummary {
    pub(super) fn of(scene: &SceneFrame<'_>, world: Vec3) -> Self {
        Self {
            topology: scene.versions.topology(),
            classification: scene.versions.classification(),
            bounds: surface_bounds(scene, world),
            visibility: shadow_visibility(scene.layers),
        }
    }

    pub(super) fn holds(self, versions: Versions) -> bool {
        self.topology == versions.topology() && self.classification == versions.classification()
    }

    pub(super) fn bounds(self) -> (Vec3, Vec3) {
        self.bounds
    }

    pub(super) fn visibility(self) -> u64 {
        self.visibility
    }
}

fn surface_bounds(scene: &SceneFrame<'_>, world: Vec3) -> (Vec3, Vec3) {
    let mut low = -0.5 * world;
    let mut high = 0.5 * world;
    for layer in scene.layers {
        for (object, group) in layer.mesh.groups().iter().enumerate() {
            if !layer.styles[object].visible() {
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
