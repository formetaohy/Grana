use std::sync::Arc;

use grana_engine::Surface;

pub(crate) fn traced(surfaces: &[grana_pipeline::Surface]) -> Vec<Surface> {
    surfaces
        .iter()
        .map(|surface| {
            Surface::new(
                surface.channel(),
                Arc::clone(surface.mesh()),
                Arc::clone(surface.shared_colors()),
                surface.revision(),
            )
        })
        .collect()
}
