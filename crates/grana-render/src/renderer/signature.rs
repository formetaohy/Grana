use super::*;

pub(super) struct Signature<'a, 'b> {
    pub(super) scene: &'a SceneFrame<'b>,
    pub(super) data: &'a FrameData,
    pub(super) section: &'a SectionData,
    pub(super) levels: Levels,
    pub(super) time: u32,
    pub(super) exposure: f32,
    pub(super) epoch: u64,
    pub(super) content: u64,
    pub(super) size: [u32; 2],
    pub(super) volume_region: [u32; 4],
    pub(super) section_region: [u32; 4],
}

impl Signature<'_, '_> {
    pub(super) fn digest(self) -> u64 {
        let Self {
            scene,
            data,
            section,
            levels,
            time,
            exposure,
            epoch,
            content,
            size,
            volume_region,
            section_region,
        } = self;
        let mut hasher = DefaultHasher::new();
        bytemuck::bytes_of(data).hash(&mut hasher);
        let mut section = *section;
        section.jitter = [0.0; 4];
        bytemuck::bytes_of(&section).hash(&mut hasher);
        exposure.to_bits().hash(&mut hasher);
        time.hash(&mut hasher);
        epoch.hash(&mut hasher);
        content.hash(&mut hasher);
        size.hash(&mut hasher);
        volume_region.hash(&mut hasher);
        section_region.hash(&mut hasher);
        levels.volume.hash(&mut hasher);
        levels.section.hash(&mut hasher);
        levels.volume_fallback.hash(&mut hasher);
        levels.section_fallback.hash(&mut hasher);
        levels.volume_capacity.hash(&mut hasher);
        levels.section_capacity.hash(&mut hasher);
        scene.channels.len().hash(&mut hasher);
        for channel in scene.channels {
            channel.visible.hash(&mut hasher);
            channel.opacity.to_bits().hash(&mut hasher);
            channel.window.min().to_bits().hash(&mut hasher);
            channel.window.max().to_bits().hash(&mut hasher);
            channel.transfer.stops().len().hash(&mut hasher);
            for stop in channel.transfer.stops() {
                stop.position().to_bits().hash(&mut hasher);
                stop.opacity().to_bits().hash(&mut hasher);
                stop.color().map(f32::to_bits).hash(&mut hasher);
            }
        }
        scene.layers.len().hash(&mut hasher);
        for layer in scene.layers {
            layer.key.hash(&mut hasher);
            layer.style_start.hash(&mut hasher);
            layer.style_count.hash(&mut hasher);
        }
        scene.styles.len().hash(&mut hasher);
        for style in scene.styles {
            style.color().map(f32::to_bits).hash(&mut hasher);
            style.visible().hash(&mut hasher);
            style.selected().hash(&mut hasher);
            style.opacity().to_bits().hash(&mut hasher);
        }
        scene.markers.len().hash(&mut hasher);
        for marker in scene.markers {
            marker.hash(&mut hasher);
        }
        hasher.finish()
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

pub(super) fn channel_style(channels: &[crate::ChannelRow<'_>]) -> u64 {
    let mut key = channels.len() as u64;
    for channel in channels {
        let mut row = (channel.visible as u64) ^ ((channel.opacity.to_bits() as u64) << 1);
        row = row.rotate_left(17) ^ (channel.window.min().to_bits() as u64);
        row = row.rotate_left(17) ^ (channel.window.max().to_bits() as u64);
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
