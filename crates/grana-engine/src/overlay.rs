use grana_render as render;
use grana_scene::Scene;

pub fn overlays(scene: &Scene) -> Vec<render::Overlay<'_>> {
    scene
        .overlays
        .iter()
        .filter(|(set, _)| {
            scene
                .objects
                .get(*set)
                .is_some_and(|object| scene.shown(object))
        })
        .map(|(_, overlay)| {
            let bitmap = overlay.bitmap();
            render::Overlay::new(
                bitmap.identity(),
                bitmap.size(),
                bitmap.pixels(),
                overlay.corners(),
                overlay.tint(),
                overlay.opacity(),
            )
        })
        .collect()
}
