use grana_display::srgb_to_linear;
use grana_engine::Overlay;
use grana_session::Session;

pub(super) fn overlays(session: &Session) -> Vec<Overlay<'_>> {
    let Some(scene) = session.scene() else {
        return Vec::new();
    };
    let localizations = session.localizations();
    let mut overlays = Vec::new();
    for set in scene.objects.iter() {
        if !scene.shown(set) {
            continue;
        }
        let Some(report) = localizations.report(set.id()) else {
            continue;
        };
        let Some(bitmap) = localizations.bitmap(set.id()) else {
            continue;
        };
        let depth = report.depth();
        let layer = localizations.layer(set.id()) as u32;
        if layer >= depth.layers() {
            continue;
        }
        let geometry = report.stack().geometry();
        assert_eq!(
            bitmap.size(),
            geometry.size(),
            "a rendered localization image matches the raster of its report"
        );
        let [width, height] = geometry.size();
        let pixel = geometry.pixel();
        let origin = geometry.origin();
        let z = 0.5 * (depth.lower(layer) + depth.upper(layer));
        let corners = [
            [origin[0], origin[1], z],
            [origin[0] + width as f32 * pixel, origin[1], z],
            [
                origin[0] + width as f32 * pixel,
                origin[1] + height as f32 * pixel,
                z,
            ],
            [origin[0], origin[1] + height as f32 * pixel, z],
        ];
        overlays.push(Overlay::new(
            bitmap.identity(),
            bitmap.size(),
            bitmap.pixels(),
            corners,
            srgb_to_linear(set.color()),
            set.opacity(),
        ));
    }
    overlays
}
