pub(super) const PARAMETERS: usize = 5;

const ITERATIONS: usize = 200;

const ATTEMPTS: usize = 8;

const FLOOR: f64 = 1.0e-9;

pub(super) fn fit(
    x: &[f32],
    y: &[f64],
    starts: &[[f64; PARAMETERS]],
    model: impl Fn(f64, &[f64; PARAMETERS]) -> f64,
) -> Option<([f64; PARAMETERS], f64)> {
    assert_eq!(x.len(), y.len(), "a fit pairs every sample with a value");
    if x.is_empty() || y.is_empty() {
        return None;
    }
    let mut best: Option<([f64; PARAMETERS], f64)> = None;
    for start in starts {
        let Some(found) = descend(x, y, start, &model) else {
            continue;
        };
        if best.is_none_or(|(_, cost)| found.1 < cost) {
            best = Some(found);
        }
    }
    best
}

fn descend(
    x: &[f32],
    y: &[f64],
    start: &[f64; PARAMETERS],
    model: &impl Fn(f64, &[f64; PARAMETERS]) -> f64,
) -> Option<([f64; PARAMETERS], f64)> {
    let mut parameters = clamp(*start);
    let mut cost = residual(x, y, &parameters, model);
    if !cost.is_finite() {
        return None;
    }
    let mut damping = 1.0e-3;
    for _ in 0..ITERATIONS {
        let mut jacobian = vec![[0.0f64; PARAMETERS]; x.len()];
        for sample in 0..x.len() {
            for parameter in 0..PARAMETERS {
                let step = (parameters[parameter].abs() * 1.0e-4).max(1.0e-7);
                let mut shifted = parameters;
                shifted[parameter] += step;
                let moved = model(f64::from(x[sample]), &shifted);
                let held = model(f64::from(x[sample]), &parameters);
                jacobian[sample][parameter] = (moved - held) / step;
            }
        }
        let mut normal = [[0.0f64; PARAMETERS]; PARAMETERS];
        let mut gradient = [0.0f64; PARAMETERS];
        for sample in 0..x.len() {
            let error = model(f64::from(x[sample]), &parameters) - y[sample];
            for left in 0..PARAMETERS {
                gradient[left] += jacobian[sample][left] * error;
                for right in 0..PARAMETERS {
                    normal[left][right] += jacobian[sample][left] * jacobian[sample][right];
                }
            }
        }
        let mut stepped = None;
        for _ in 0..ATTEMPTS {
            let mut system = normal;
            for parameter in 0..PARAMETERS {
                system[parameter][parameter] += damping * normal[parameter][parameter].max(FLOOR);
            }
            let right = gradient.map(|value| -value);
            let Some(delta) = solve(system, right) else {
                damping *= 10.0;
                continue;
            };
            let mut candidate = parameters;
            for parameter in 0..PARAMETERS {
                candidate[parameter] = (parameters[parameter] + delta[parameter]).max(FLOOR);
            }
            let next = residual(x, y, &candidate, model);
            if next.is_finite() && next < cost {
                stepped = Some((candidate, next, delta));
                break;
            }
            damping *= 10.0;
        }
        let Some((candidate, next, delta)) = stepped else {
            break;
        };
        let converged = (0..PARAMETERS).all(|parameter| {
            delta[parameter].abs() <= 1.0e-9 * parameters[parameter].abs().max(1.0)
        });
        parameters = candidate;
        cost = next;
        damping = (damping * 0.2).max(1.0e-9);
        if converged {
            break;
        }
    }
    Some((parameters, cost))
}

fn residual(
    x: &[f32],
    y: &[f64],
    parameters: &[f64; PARAMETERS],
    model: &impl Fn(f64, &[f64; PARAMETERS]) -> f64,
) -> f64 {
    x.iter()
        .zip(y)
        .map(|(sample, value)| {
            let error = model(f64::from(*sample), parameters) - value;
            error * error
        })
        .sum()
}

fn clamp(parameters: [f64; PARAMETERS]) -> [f64; PARAMETERS] {
    parameters.map(|value| {
        if value.is_finite() {
            value.max(FLOOR)
        } else {
            FLOOR
        }
    })
}

fn solve(
    mut matrix: [[f64; PARAMETERS]; PARAMETERS],
    mut right: [f64; PARAMETERS],
) -> Option<[f64; PARAMETERS]> {
    for column in 0..PARAMETERS {
        let pivot = (column..PARAMETERS).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        if matrix[pivot][column].abs() <= f64::EPSILON {
            return None;
        }
        matrix.swap(column, pivot);
        right.swap(column, pivot);
        let held = matrix[column];
        for row in column + 1..PARAMETERS {
            let factor = matrix[row][column] / matrix[column][column];
            if factor == 0.0 {
                continue;
            }
            for (slot, value) in matrix[row].iter_mut().enumerate().skip(column) {
                *value -= factor * held[slot];
            }
            right[row] -= factor * right[column];
        }
    }
    let mut solution = [0.0f64; PARAMETERS];
    for row in (0..PARAMETERS).rev() {
        let mut sum = right[row];
        for column in row + 1..PARAMETERS {
            sum -= matrix[row][column] * solution[column];
        }
        solution[row] = sum / matrix[row][row];
    }
    solution
        .iter()
        .all(|value| value.is_finite())
        .then_some(solution)
}
