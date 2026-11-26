use grana_render::Demand;
use grana_scene::{ObjectKind, ObjectSet, Scene};

pub fn demand(scene: &Scene) -> Demand {
    let mut demand = Demand::none();
    demand.volume = scene
        .channels
        .iter()
        .any(|channel| channel.enabled && channel.opacity > 0.0);
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
