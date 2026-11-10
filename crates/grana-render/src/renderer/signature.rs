use super::*;
use grana_display::{CameraState, SectionState};

#[derive(Clone, Copy)]
pub(super) enum PaneSignature<'a> {
    Volume {
        data: &'a FrameData,
        camera: CameraState,
        region: [u32; 4],
    },
    Section {
        data: &'a SectionData,
        view: SectionState,
        region: [u32; 4],
    },
}

#[derive(Clone, Copy)]
pub(super) struct Signature<'a, 'b> {
    pub(super) scene: &'a SceneFrame<'b>,
    pub(super) panes: &'a [Option<PaneSignature<'a>>; PANE_COUNT],
    pub(super) levels: Levels,
    pub(super) time: u32,
    pub(super) exposure: f32,
    pub(super) epoch: u64,
    pub(super) content: u64,
    pub(super) size: [u32; 2],
}

impl Signature<'_, '_> {
    pub(super) fn digest(self) -> u64 {
        let Self {
            scene,
            panes,
            levels,
            time,
            exposure,
            epoch,
            content,
            size,
        } = self;
        let mut hasher = DefaultHasher::new();
        for (index, pane) in panes.iter().enumerate() {
            match pane {
                Some(PaneSignature::Volume {
                    data,
                    camera,
                    region,
                }) => {
                    let mut data = **data;
                    data.jitter = [0.0; 4];
                    data.lens = [0.0, 0.0, data.lens[2], data.lens[3]];
                    bytemuck::bytes_of(&data).hash(&mut hasher);
                    camera.projection.index().hash(&mut hasher);
                    region.hash(&mut hasher);
                }
                Some(PaneSignature::Section { data, region, .. }) => {
                    let mut data = **data;
                    data.jitter = [0.0; 4];
                    bytemuck::bytes_of(&data).hash(&mut hasher);
                    region.hash(&mut hasher);
                }
                None => {}
            }
            match levels.panes[index] {
                Some(PaneLevels::Volume(volume)) => {
                    volume.ladder.base.hash(&mut hasher);
                    volume.ladder.count.hash(&mut hasher);
                    volume.capacity.hash(&mut hasher);
                }
                Some(PaneLevels::Section(section)) => {
                    section.level.hash(&mut hasher);
                    section.fallback.hash(&mut hasher);
                    section.capacity.hash(&mut hasher);
                }
                None => {}
            }
        }
        exposure.to_bits().hash(&mut hasher);
        time.hash(&mut hasher);
        epoch.hash(&mut hasher);
        content.hash(&mut hasher);
        size.hash(&mut hasher);
        content_digest(scene, &mut hasher);
        hasher.finish()
    }

    pub(super) fn transition(self) -> u64 {
        let Self {
            scene,
            panes,
            time,
            exposure,
            epoch,
            size,
            ..
        } = self;
        let mut hasher = DefaultHasher::new();
        for pane in &panes.iter().flatten().collect::<Vec<_>>() {
            match pane {
                PaneSignature::Volume {
                    data,
                    camera,
                    region,
                } => {
                    hash_camera(*camera, &mut hasher);
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
                }
                PaneSignature::Section { view, data, region } => {
                    hash_section(*view, &mut hasher);
                    data.projection.map(f32::to_bits).hash(&mut hasher);
                    region.hash(&mut hasher);
                }
            }
        }
        scene.step_scale.to_bits().hash(&mut hasher);
        exposure.to_bits().hash(&mut hasher);
        time.hash(&mut hasher);
        epoch.hash(&mut hasher);
        size.hash(&mut hasher);
        content_digest(scene, &mut hasher);
        hasher.finish()
    }
}

fn hash_camera(camera: CameraState, hasher: &mut DefaultHasher) {
    camera.pivot.map(f32::to_bits).hash(hasher);
    camera.yaw.to_bits().hash(hasher);
    camera.pitch.to_bits().hash(hasher);
    camera.fov_y.to_bits().hash(hasher);
    camera.projection.index().hash(hasher);
    camera.aperture.to_bits().hash(hasher);
    camera.focus.to_bits().hash(hasher);
}

fn hash_section(view: SectionState, hasher: &mut DefaultHasher) {
    view.center.map(f32::to_bits).hash(hasher);
    view.azimuth.to_bits().hash(hasher);
    view.elevation.to_bits().hash(hasher);
    view.span.to_bits().hash(hasher);
    view.projection.index().hash(hasher);
    view.thickness.to_bits().hash(hasher);
}

fn content_digest(scene: &SceneFrame<'_>, hasher: &mut DefaultHasher) {
    scene.channels.len().hash(hasher);
    for channel in scene.channels {
        channel.visible.hash(hasher);
        channel.opacity.to_bits().hash(hasher);
        channel.window.min().to_bits().hash(hasher);
        channel.window.max().to_bits().hash(hasher);
        channel.volume.mode().index().hash(hasher);
        channel.volume.isovalue().to_bits().hash(hasher);
        channel.transfer.stops().len().hash(hasher);
        for stop in channel.transfer.stops() {
            stop.position().to_bits().hash(hasher);
            stop.opacity().to_bits().hash(hasher);
            stop.color().map(f32::to_bits).hash(hasher);
        }
    }
    scene.layers.len().hash(hasher);
    for layer in scene.layers {
        layer.key.hash(hasher);
        layer.style_start.hash(hasher);
        layer.style_count.hash(hasher);
    }
    scene.styles.len().hash(hasher);
    for style in scene.styles {
        style.color().map(f32::to_bits).hash(hasher);
        style.visible().hash(hasher);
        style.selected().hash(hasher);
        style.opacity().to_bits().hash(hasher);
    }
    scene.markers.len().hash(hasher);
    for marker in scene.markers {
        marker.hash(hasher);
    }
}

pub(super) fn digest(words: &[u64]) -> u64 {
    let mut hasher = DefaultHasher::new();
    words.hash(&mut hasher);
    hasher.finish()
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
    for marker in scene.markers {
        let center = Vec3::from(marker.center());
        let radius = Vec3::splat(marker.radius());
        low = low.min(center - radius);
        high = high.max(center + radius);
    }
    (low, high)
}

pub(super) fn shadow_geometry(scene: &SceneFrame<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    scene.layers.len().hash(&mut hasher);
    for layer in scene.layers {
        layer.key.hash(&mut hasher);
        layer.style_count.hash(&mut hasher);
        for style in &scene.styles[layer.style_start..layer.style_start + layer.style_count] {
            style.visible().hash(&mut hasher);
        }
    }
    scene.markers.len().hash(&mut hasher);
    for marker in scene.markers {
        marker.center().map(f32::to_bits).hash(&mut hasher);
        marker.radius().to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

pub(super) fn scene_styles(scene: &SceneFrame<'_>) -> u64 {
    let mut hasher = DefaultHasher::new();
    channel_style(scene.channels).hash(&mut hasher);
    for layer in scene.layers {
        layer.key.hash(&mut hasher);
        layer.style_start.hash(&mut hasher);
        layer.style_count.hash(&mut hasher);
    }
    for style in scene.styles {
        style.color().map(f32::to_bits).hash(&mut hasher);
        style.visible().hash(&mut hasher);
    }
    hasher.finish()
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
