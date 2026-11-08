const BAND_FLOOR: f32 = 0.75;
const BAND_CEIL: f32 = 1.25;
const BAND_REACH: f32 = 1.0e19;

fn band_count() -> u32 {
    return frame.bands.x;
}

fn band_edge(band: u32) -> f32 {
    return frame.edge[band - 1u];
}

fn band_lower(band: u32) -> f32 {
    if (band + 1u == band_count()) {
        return 0.0;
    }
    return select(0.0, BAND_FLOOR * band_edge(band), band > 0u);
}

fn band_upper(band: u32) -> f32 {
    return select(BAND_REACH, BAND_CEIL * band_edge(band + 1u), band + 1u < band_count());
}

fn band_for(t: f32, noise: f32) -> u32 {
    let count = band_count();
    var band = 0u;
    for (var reached = 1u; reached < count; reached += 1u) {
        band = select(band, reached, t >= band_edge(reached));
    }
    if (band > 0u) {
        let edge = band_edge(band);
        let width = (BAND_CEIL - BAND_FLOOR) * edge;
        let weight = clamp((t - BAND_FLOOR * edge) / max(width, 1.0e-9), 0.0, 1.0);
        band = select(band - 1u, band, noise < weight);
    }
    return band;
}
