const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

const LANCZOS_SHIFT: f64 = 7.0;

const MAXIMUM_ITERATION: usize = 300;

const EPSILON: f64 = 3.0e-16;

const TINY: f64 = 1.0e-300;

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
}

pub fn hedges_g(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() < 2 || b.len() < 2 {
        return None;
    }
    assert!(
        a.iter().chain(b).all(|value| value.is_finite()),
        "a compared sample holds finite values"
    );
    let mean_a = mean(a);
    let mean_b = mean(b);
    let variance_a = deviation(a, mean_a).powi(2);
    let variance_b = deviation(b, mean_b).powi(2);
    let pooled = ((a.len() - 1) as f64 * variance_a + (b.len() - 1) as f64 * variance_b)
        / (a.len() + b.len() - 2) as f64;
    if pooled <= 0.0 {
        return None;
    }
    let correction = 1.0 - 3.0 / (4.0 * (a.len() + b.len()) as f64 - 9.0);
    Some(correction * (mean_a - mean_b) / pooled.sqrt())
}

pub fn student_two_sided(t: f64, df: f64) -> f64 {
    assert!(!t.is_nan(), "a Student statistic is a number");
    assert!(
        df > 0.0,
        "a Student test counts positive degrees of freedom"
    );
    if t == 0.0 {
        return 1.0;
    }
    let x = df / (df + t * t);
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    regularized_beta(0.5 * df, 0.5, x)
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

fn regularized_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_continued_fraction(a, b, x) / a
    } else {
        1.0 - front * beta_continued_fraction(b, a, 1.0 - x) / b
    }
}

fn beta_continued_fraction(a: f64, b: f64, x: f64) -> f64 {
    let sum = a + b;
    let forward = a + 1.0;
    let backward = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - sum * x / forward;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for iteration in 1..=MAXIMUM_ITERATION {
        let m = iteration as f64;
        let double = 2.0 * m;
        let numerator = m * (b - m) * x / ((backward + double) * (a + double));
        d = 1.0 + numerator * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + numerator / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let numerator = -(a + m) * (sum + m) * x / ((a + double) * (forward + double));
        d = 1.0 + numerator * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + numerator / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < EPSILON {
            break;
        }
    }
    h
}

fn ln_gamma(value: f64) -> f64 {
    assert!(value > 0.0, "a gamma function counts a positive argument");
    if value < 0.5 {
        let reflected = std::f64::consts::PI / (std::f64::consts::PI * value).sin();
        return reflected.ln() - ln_gamma(1.0 - value);
    }
    let shifted = value - 1.0;
    let mut series = LANCZOS[0];
    for (index, coefficient) in LANCZOS.iter().enumerate().skip(1) {
        series += coefficient / (shifted + index as f64);
    }
    let t = shifted + LANCZOS_SHIFT + 0.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (shifted + 0.5) * t.ln() - t + series.ln()
}
