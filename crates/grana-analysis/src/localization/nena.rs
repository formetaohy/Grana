use super::LocalizationFrame;
use super::fit::{PARAMETERS, fit};
use super::report::NeNa;

const BINS: usize = 1000;

const MINIMUM_PAIRS: f64 = 100.0;

const FLOOR: f64 = 1.0e-9;

pub(super) fn measure(
    frames: &[LocalizationFrame<'_>],
    reach: f32,
    sigma: f32,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<NeNa>, String> {
    if frames.len() < 2 {
        return Err("the localizations span one frame to estimate a precision".to_string());
    }
    let Some(counts) = histogram(frames, reach, report)? else {
        return Ok(None);
    };
    let total: f64 = counts.iter().sum();
    if total < MINIMUM_PAIRS {
        return Err(format!(
            "the localizations hold {total} next-frame neighbours, too few to estimate a precision"
        ));
    }
    let width = f64::from(reach) * 1000.0 / BINS as f64;
    let bins = (0..BINS)
        .map(|bin| ((bin as f64 + 0.5) * width) as f32)
        .collect::<Vec<_>>();
    let area = (1..BINS)
        .map(|bin| 0.5 * (counts[bin] + counts[bin - 1]) * width)
        .sum::<f64>();
    let peak = counts
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map_or(0.0, |(bin, _)| (bin as f64 + 0.5) * width);
    let reach = f64::from(reach) * 1000.0;
    let sigma = f64::from(sigma).max(FLOOR);
    let model = |distance: f64, parameters: &[f64; PARAMETERS]| {
        let [amplitude, scale, short_amplitude, short_center, short_scale] = *parameters;
        let single = amplitude * distance / (2.0 * scale * scale)
            * (-(distance * distance) / (4.0 * scale * scale)).exp();
        let short = short_amplitude / (short_scale * (2.0 * std::f64::consts::PI).sqrt())
            * (-0.5 * ((distance - short_center) / short_scale).powi(2)).exp();
        single + short
    };
    let starts = [
        [0.8 * area, sigma, 0.1 * area, 2.0 * sigma, sigma],
        [
            0.8 * area,
            peak / 2.0f64.sqrt(),
            0.1 * area,
            0.7 * reach,
            0.3 * reach,
        ],
        [0.8 * area, sigma, 0.1 * area, 0.5 * reach, sigma],
    ];
    let Some((parameters, _)) = fit(&bins, &counts, &starts, model) else {
        return Err("the next-frame neighbour distances do not fit a NeNA model".to_string());
    };
    let precision = parameters[1];
    if !(precision.is_finite() && precision > FLOOR) || precision > reach {
        return Err(format!(
            "the next-frame neighbour distances fit the precision {precision:.3} nm"
        ));
    }
    let fitted = bins
        .iter()
        .map(|value| model(f64::from(*value), &parameters) as f32)
        .collect();
    let counted = counts.iter().map(|value| *value as f32).collect();
    Ok(Some(NeNa::new(bins, counted, fitted, precision as f32)))
}

fn histogram(
    frames: &[LocalizationFrame<'_>],
    reach: f32,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Vec<f64>>, String> {
    assert!(
        reach.is_finite() && reach > 0.0,
        "a next-frame neighbour distance reaches a positive length"
    );
    let pairs = frames.len() - 1;
    let mut counts = vec![0.0f64; BINS];
    let mut next = Lattice::new(frames[1].centers(), reach);
    for pair in 0..pairs {
        if !report(pair, pairs) {
            return Ok(None);
        }
        let source = frames[pair].centers();
        accumulate(&source, &next, reach, &mut counts);
        if pair + 2 < frames.len() {
            next = Lattice::new(frames[pair + 2].centers(), reach);
        }
        if counts.iter().any(|count| !count.is_finite()) {
            return Err("the next-frame neighbour histogram overflows".to_string());
        }
    }
    report(pairs, pairs);
    Ok(Some(counts))
}

fn accumulate(source: &[[f32; 2]], target: &Lattice, reach: f32, counts: &mut [f64]) {
    let width = f64::from(reach) / BINS as f64;
    for point in source {
        target.visit_near(*point, reach, |other| {
            let x = f64::from(point[0]) - f64::from(other[0]);
            let y = f64::from(point[1]) - f64::from(other[1]);
            let distance = (x * x + y * y).sqrt();
            if distance < f64::from(reach) {
                let bin = (distance / width) as usize;
                counts[bin.min(BINS - 1)] += 1.0;
            }
        });
    }
}

struct Lattice {
    low: [f32; 2],
    cell: f32,
    dims: [usize; 2],
    points: Vec<[f32; 2]>,
    cells: Vec<Vec<u32>>,
}

impl Lattice {
    fn new(points: Vec<[f32; 2]>, cell: f32) -> Self {
        assert!(
            cell.is_finite() && cell > 0.0,
            "a neighbour lattice spans a positive cell"
        );
        if points.is_empty() {
            return Self {
                low: [0.0; 2],
                cell,
                dims: [1, 1],
                points,
                cells: vec![Vec::new()],
            };
        }
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        for point in &points {
            for (axis, value) in point.iter().enumerate() {
                low[axis] = low[axis].min(*value);
                high[axis] = high[axis].max(*value);
            }
        }
        let dims = [0, 1].map(|axis| ((high[axis] - low[axis]) / cell).ceil() as usize + 1);
        let mut cells = vec![Vec::new(); dims[0] * dims[1]];
        for (index, point) in points.iter().enumerate() {
            let coord = [0, 1]
                .map(|axis| (((point[axis] - low[axis]) / cell) as usize).min(dims[axis] - 1));
            cells[coord[0] + dims[0] * coord[1]].push(index as u32);
        }
        Self {
            low,
            cell,
            dims,
            points,
            cells,
        }
    }

    fn visit_near(&self, point: [f32; 2], reach: f32, mut visit: impl FnMut([f32; 2])) {
        let center = [0, 1].map(|axis| {
            let probe = (point[axis] - self.low[axis]) / self.cell;
            if probe.is_finite() {
                (probe.floor() as i64).clamp(0, self.dims[axis] as i64 - 1)
            } else {
                0
            }
        });
        for row in (center[1] - 1)..=(center[1] + 1) {
            if !(0..self.dims[1] as i64).contains(&row) {
                continue;
            }
            for column in (center[0] - 1)..=(center[0] + 1) {
                if !(0..self.dims[0] as i64).contains(&column) {
                    continue;
                }
                let cell = &self.cells[column as usize + self.dims[0] * row as usize];
                for index in cell {
                    let other = self.points[*index as usize];
                    let x = other[0] - point[0];
                    let y = other[1] - point[1];
                    if x * x + y * y <= reach * reach {
                        visit(other);
                    }
                }
            }
        }
    }
}
