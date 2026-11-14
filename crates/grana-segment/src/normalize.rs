const LOWER: f64 = 1.0;
const UPPER: f64 = 99.0;
const FLOOR: f32 = 1.0e-3;

pub fn plane(values: &[f32]) -> Vec<f32> {
    let (low, high) = percentiles(values);
    let span = high - low;
    if span > FLOOR {
        values.iter().map(|value| (value - low) / span).collect()
    } else {
        vec![0.0; values.len()]
    }
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
