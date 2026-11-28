use grana_render::Demand;
use grana_scene::{ObjectKind, ObjectSet, Scene};

pub fn surface_channel(scene: &Scene, index: usize) -> bool {
    scene
        .dataset
        .channels()
        .get(index)
        .is_some_and(|info| info.color_table().is_some())
}

pub fn demand(scene: &Scene) -> Demand {
    let mut demand = Demand::none();
    demand.volume = scene.channels.iter().enumerate().any(|(index, channel)| {
        channel.enabled && channel.opacity > 0.0 && !surface_channel(scene, index)
    });
    for (index, channel) in scene.channels.iter().enumerate() {
        demand.surfaces |= channel.enabled && surface_channel(scene, index);
    }
    for set in scene.objects.iter() {
        if !scene.shown(set) {
            continue;
        }
        demand.surfaces |= layers(set);
        demand.splats |= splats(set);
    }
    demand.transparent = demand.splats || scene.pickable().any(|set| set.opacity() < 1.0);
    demand
}

fn layers(set: &ObjectSet) -> bool {
    (set.detected() > 0
        && matches!(
            set.kind(),
            ObjectKind::Surfaces | ObjectKind::Cells | ObjectKind::Filaments
        ))
        || set.tracks().is_some_and(|tracks| !tracks.set().is_empty())
}

fn splats(set: &ObjectSet) -> bool {
    (set.kind() == ObjectKind::Spots && set.detected() > 0)
        || set
            .surface_seeds()
            .manual()
            .is_some_and(|frames| frames.iter().any(|seed| !seed.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grana_scene::Dataset;
    use grana_volume::{
        Calibration, ChannelInfo, ColorTable, Extent, Intensity, Level, Pyramid, Spacing,
        Statistics, VoxelFormat, VoxelSemantics,
    };

    fn scene(palette: bool) -> Scene {
        let level = Level::new(Extent::new(8, 8, 4), Spacing::new(1.0, 1.0, 1.0));
        let statistics = Statistics::declared(Intensity::new(0.0, 255.0), VoxelSemantics::Scalar);
        let channel = ChannelInfo::new("signal", [0.2, 0.6, 0.2], statistics, VoxelFormat::U8);
        let channel = match palette {
            true => {
                channel.with_color_table(ColorTable::new(vec![[0.0, 0.0, 0.0], [0.2, 0.6, 0.2]]))
            }
            false => channel,
        };
        let pyramid = Pyramid::new(vec![level], 1, vec![channel], 1, 1.0, Calibration::Recorded);
        Scene::new(Dataset::synthetic("demand", pyramid))
    }

    #[test]
    fn an_indexed_channel_demands_no_volume() {
        assert!(
            !demand(&scene(true)).volume,
            "an indexed channel traces its boundary geometry instead of marching a volume"
        );
        assert!(
            demand(&scene(false)).volume,
            "a scalar channel still marches its volume"
        );
    }
}
