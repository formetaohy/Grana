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
