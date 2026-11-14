use crate::sample::Sample;
use crate::table::Table;

const SAMPLE_LIMIT: usize = 16_384;
const HISTOGRAM_LIMIT: usize = 256;
const SCATTER_LIMIT: usize = 128;

pub(crate) fn histogram(table: &Table, column: usize, span: f32) -> usize {
    count(
        sample(table, column).as_ref(),
        f64::from(span),
        HISTOGRAM_LIMIT,
    )
}

pub(crate) fn scatter(
    table: &Table,
    x: usize,
    y: usize,
    x_span: f32,
    y_span: f32,
) -> (usize, usize) {
    (
        count(sample(table, x).as_ref(), f64::from(x_span), SCATTER_LIMIT),
        count(sample(table, y).as_ref(), f64::from(y_span), SCATTER_LIMIT),
    )
}

pub(crate) fn index(value: f32, low: f32, high: f32, bins: usize) -> Option<usize> {
    if !value.is_finite() {
        return None;
    }
    let span = high - low;
    if span <= 0.0 {
        return Some(0);
    }
    let position = ((value - low) / span * bins as f32).floor();
    Some((position as isize).clamp(0, bins as isize - 1) as usize)
}

fn sample(table: &Table, column: usize) -> Option<Sample> {
    let rows = table.rows();
    if rows == 0 {
        return None;
    }
    let values = table.materialize(column);
    let take = rows.min(SAMPLE_LIMIT);
    let span = rows - 1;
    Sample::new(
        (0..take)
            .map(|index| index * span / (take - 1).max(1))
            .map(|row| f64::from(values[row]))
            .collect(),
    )
}

fn count(sample: Option<&Sample>, span: f64, limit: usize) -> usize {
    let Some(sample) = sample else {
        return 1;
    };
    if sample.len() < 2 || !span.is_finite() || span <= 0.0 {
        return 1;
    }
    let scale = (sample.len() as f64).powf(-1.0 / 3.0);
    let interquartile = sample.quantile(75.0) - sample.quantile(25.0);
    let width = if interquartile > 0.0 {
        2.0 * interquartile * scale
    } else {
        let deviation = sample.deviation().unwrap_or(0.0);
        if deviation <= 0.0 {
            return 1;
        }
        3.49 * deviation * scale
    };
    if !(width.is_finite() && width > 0.0) {
        return 1;
    }
    ((span / width).ceil() as usize).clamp(1, limit)
}
