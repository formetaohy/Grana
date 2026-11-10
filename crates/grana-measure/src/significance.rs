use crate::math::{normal_two_sided, student_quantile, student_two_sided};
use crate::sample::Sample;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Object,
    Image,
}

impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Self::Object => "object",
            Self::Image => "image",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WelchTest {
    n_a: usize,
    n_b: usize,
    mean_a: f64,
    mean_b: f64,
    deviation_a: f64,
    deviation_b: f64,
    difference: f64,
    t: f64,
    df: f64,
    p: f64,
}

impl WelchTest {
    pub fn of(a: &[f64], b: &[f64]) -> Option<Self> {
        if a.len() < 2 || b.len() < 2 {
            return None;
        }
        assert!(
            a.iter().chain(b).all(|value| value.is_finite()),
            "a compared sample holds finite values"
        );
        let mean_a = mean(a);
        let mean_b = mean(b);
        let deviation_a = deviation(a, mean_a);
        let deviation_b = deviation(b, mean_b);
        let variance_a = deviation_a * deviation_a / a.len() as f64;
        let variance_b = deviation_b * deviation_b / b.len() as f64;
        let difference = mean_a - mean_b;
        let error = variance_a + variance_b;
        let (t, df) = if error > 0.0 {
            let term_a = variance_a * variance_a / (a.len() - 1) as f64;
            let term_b = variance_b * variance_b / (b.len() - 1) as f64;
            let denominator = term_a + term_b;
            let df = if denominator > 0.0 {
                error * error / denominator
            } else {
                degrees_of_freedom(a.len(), b.len())
            };
            (difference / error.sqrt(), df)
        } else if difference == 0.0 {
            (0.0, degrees_of_freedom(a.len(), b.len()))
        } else {
            (
                f64::INFINITY.copysign(difference),
                degrees_of_freedom(a.len(), b.len()),
            )
        };
        Some(Self {
            n_a: a.len(),
            n_b: b.len(),
            mean_a,
            mean_b,
            deviation_a,
            deviation_b,
            difference,
            t,
            df,
            p: student_two_sided(t, df),
        })
    }

    pub fn n_a(&self) -> usize {
        self.n_a
    }

    pub fn n_b(&self) -> usize {
        self.n_b
    }

    pub fn mean_a(&self) -> f64 {
        self.mean_a
    }

    pub fn mean_b(&self) -> f64 {
        self.mean_b
    }

    pub fn deviation_a(&self) -> f64 {
        self.deviation_a
    }

    pub fn deviation_b(&self) -> f64 {
        self.deviation_b
    }

    pub fn difference(&self) -> f64 {
        self.difference
    }

    pub fn t(&self) -> f64 {
        self.t
    }

    pub fn df(&self) -> f64 {
        self.df
    }

    pub fn p(&self) -> f64 {
        self.p
    }

    pub fn interval(&self, level: f64) -> Option<(f64, f64)> {
        assert!(
            level.is_finite() && level > 0.0 && level < 1.0,
            "a confidence level lies inside the unit interval"
        );
        let error = (self.deviation_a * self.deviation_a / self.n_a as f64
            + self.deviation_b * self.deviation_b / self.n_b as f64)
            .sqrt();
        if !error.is_finite() {
            return None;
        }
        let t = student_quantile(0.5 + 0.5 * level, self.df);
        Some((self.difference - t * error, self.difference + t * error))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MannWhitney {
    n_a: usize,
    n_b: usize,
    u: f64,
    z: f64,
    p: f64,
    delta: f64,
    median_a: f64,
    median_b: f64,
}

impl MannWhitney {
    pub fn of(a: &[f64], b: &[f64]) -> Option<Self> {
        if a.is_empty() || b.is_empty() {
            return None;
        }
        assert!(
            a.iter().chain(b).all(|value| value.is_finite()),
            "a compared sample holds finite values"
        );
        let mut ranked = Vec::with_capacity(a.len() + b.len());
        ranked.extend(a.iter().map(|value| (*value, true)));
        ranked.extend(b.iter().map(|value| (*value, false)));
        ranked.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut rank_sum_a = 0.0;
        let mut ties = 0.0;
        let mut index = 0;
        while index < ranked.len() {
            let mut end = index + 1;
            while end < ranked.len() && ranked[end].0 == ranked[index].0 {
                end += 1;
            }
            let width = end - index;
            let rank = 0.5 * (index + 1 + end) as f64;
            for (_, first) in &ranked[index..end] {
                if *first {
                    rank_sum_a += rank;
                }
            }
            if width > 1 {
                let width = width as f64;
                ties += width * width * width - width;
            }
            index = end;
        }
        let n_a = a.len();
        let n_b = b.len();
        let total = (n_a + n_b) as f64;
        let u = rank_sum_a - (n_a * (n_a + 1)) as f64 / 2.0;
        let expected = 0.5 * (n_a * n_b) as f64;
        let variance = if total > 1.0 {
            (n_a * n_b) as f64 / 12.0 * ((total + 1.0) - ties / (total * (total - 1.0)))
        } else {
            0.0
        };
        let (z, p) = if variance > 0.0 {
            let shift = if u > expected {
                0.5
            } else if u < expected {
                -0.5
            } else {
                0.0
            };
            let z = (u - expected - shift) / variance.sqrt();
            (z, normal_two_sided(z))
        } else {
            (0.0, 1.0)
        };
        let sample_a = Sample::of(a).expect("a compared sample holds a value");
        let sample_b = Sample::of(b).expect("a compared sample holds a value");
        Some(Self {
            n_a,
            n_b,
            u,
            z,
            p,
            delta: 2.0 * u / (n_a * n_b) as f64 - 1.0,
            median_a: sample_a.median(),
            median_b: sample_b.median(),
        })
    }

    pub fn n_a(&self) -> usize {
        self.n_a
    }

    pub fn n_b(&self) -> usize {
        self.n_b
    }

    pub fn u(&self) -> f64 {
        self.u
    }

    pub fn z(&self) -> f64 {
        self.z
    }

    pub fn p(&self) -> f64 {
        self.p.clamp(0.0, 1.0)
    }

    pub fn delta(&self) -> f64 {
        self.delta
    }

    pub fn median_a(&self) -> f64 {
        self.median_a
    }

    pub fn median_b(&self) -> f64 {
        self.median_b
    }
}

pub fn hedges_g(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() < 2 || b.len() < 2 {
        return None;
    }
    assert!(
        a.iter().chain(b).all(|value| value.is_finite()),
        "a compared sample holds finite values"
    );
    hedges_g_pooled(
        a.len(),
        mean(a),
        Some(deviation(a, mean(a))),
        b.len(),
        mean(b),
        Some(deviation(b, mean(b))),
    )
}

pub fn hedges_g_pooled(
    objects_a: usize,
    mean_a: f64,
    deviation_a: Option<f64>,
    objects_b: usize,
    mean_b: f64,
    deviation_b: Option<f64>,
) -> Option<f64> {
    if objects_a < 2 || objects_b < 2 {
        return None;
    }
    let deviation_a = deviation_a?;
    let deviation_b = deviation_b?;
    if ![mean_a, mean_b, deviation_a, deviation_b]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let pooled = ((objects_a - 1) as f64 * deviation_a * deviation_a
        + (objects_b - 1) as f64 * deviation_b * deviation_b)
        / (objects_a + objects_b - 2) as f64;
    if pooled <= 0.0 {
        return None;
    }
    let correction = 1.0 - 3.0 / (4.0 * (objects_a + objects_b) as f64 - 9.0);
    Some(correction * (mean_a - mean_b) / pooled.sqrt())
}

pub fn benjamini_hochberg(p: &[f64]) -> Vec<f64> {
    assert!(
        p.iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)),
        "a corrected p value is a probability"
    );
    let count = p.len();
    let mut order = (0..count).collect::<Vec<_>>();
    order.sort_by(|left, right| p[*left].total_cmp(&p[*right]));
    let mut corrected = vec![0.0; count];
    let mut running = 1.0;
    for rank in (1..=count).rev() {
        let index = order[rank - 1];
        let value = (p[index] * count as f64 / rank as f64).min(running);
        corrected[index] = value;
        running = value;
    }
    corrected
}

pub fn stars(q: f64) -> &'static str {
    if q < 0.001 {
        "***"
    } else if q < 0.01 {
        "**"
    } else if q < 0.05 {
        "*"
    } else {
        "ns"
    }
}

fn degrees_of_freedom(n_a: usize, n_b: usize) -> f64 {
    (n_a + n_b - 2) as f64
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn deviation(values: &[f64], mean: f64) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let variance = values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    variance.sqrt()
}
