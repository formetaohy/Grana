const LOWER: f64 = 2.0;
const UPPER: f64 = 98.0;
const EPSILON: f32 = 1.0e-7;

pub(crate) fn plane(values: &[f32]) -> Vec<f32> {
    let (low, high) = percentiles(values);
    let span = high - low + EPSILON;
    values
        .iter()
        .map(|value| ((value - low) / span).clamp(0.0, 1.0))
        .collect()
}

fn percentiles(values: &[f32]) -> (f32, f32) {
    assert!(
        !values.is_empty(),
        "a normalized plane holds at least one value"
    );
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f32::total_cmp);
    (quantile(&sorted, LOWER), quantile(&sorted, UPPER))
}

fn quantile(sorted: &[f32], percent: f64) -> f32 {
    let position = (sorted.len() - 1) as f64 * percent / 100.0;
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(sorted.len() - 1);
    let weight = (position - lower as f64) as f32;
    sorted[lower] + (sorted[upper] - sorted[lower]) * weight
}
