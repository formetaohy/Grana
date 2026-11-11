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

pub fn student_quantile(probability: f64, df: f64) -> f64 {
    assert!(
        probability.is_finite() && probability > 0.0 && probability < 1.0,
        "a Student quantile samples an interior probability"
    );
    assert!(
        df.is_finite() && df > 0.0,
        "a Student quantile counts positive degrees of freedom"
    );
    if probability == 0.5 {
        return 0.0;
    }
    if probability < 0.5 {
        return -student_quantile(1.0 - probability, df);
    }
    let mut low = 0.0;
    let mut high = 1.0;
    while student_cdf(high, df) < probability && high < 1.0e12 {
        high *= 2.0;
    }
    for _ in 0..200 {
        let middle = 0.5 * (low + high);
        if student_cdf(middle, df) < probability {
            low = middle;
        } else {
            high = middle;
        }
    }
    0.5 * (low + high)
}

pub fn normal_two_sided(z: f64) -> f64 {
    assert!(!z.is_nan(), "a normal statistic is a number");
    if z == 0.0 {
        return 1.0;
    }
    regularized_gamma_q(0.5, 0.5 * z * z).clamp(0.0, 1.0)
}

pub(crate) fn binomial_cdf(k: usize, n: usize, probability: f64) -> f64 {
    assert!(
        n > 0,
        "a binomial count samples a positive number of trials"
    );
    assert!(
        probability.is_finite() && probability > 0.0 && probability < 1.0,
        "a binomial trial samples an interior probability"
    );
    if k >= n {
        return 1.0;
    }
    regularized_beta((n - k) as f64, (k + 1) as f64, 1.0 - probability)
}

fn student_cdf(t: f64, df: f64) -> f64 {
    if t <= 0.0 {
        return 1.0 - student_cdf(-t, df);
    }
    1.0 - 0.5 * regularized_beta(0.5 * df, 0.5, df / (df + t * t))
}

fn regularized_gamma_q(a: f64, x: f64) -> f64 {
    assert!(a > 0.0, "a gamma function counts a positive shape");
    assert!(x >= 0.0, "a gamma function samples a non-negative value");
    if x == 0.0 {
        return 1.0;
    }
    if x < a + 1.0 {
        return 1.0 - gamma_series(a, x);
    }
    gamma_continued_fraction(a, x)
}

fn gamma_series(a: f64, x: f64) -> f64 {
    let mut sum = 1.0 / a;
    let mut term = sum;
    let mut shifted = a;
    for _ in 0..MAXIMUM_ITERATION {
        shifted += 1.0;
        term *= x / shifted;
        sum += term;
        if term.abs() < sum.abs() * EPSILON {
            break;
        }
    }
    sum * (-x + a * x.ln() - ln_gamma(a)).exp()
}

fn gamma_continued_fraction(a: f64, x: f64) -> f64 {
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / TINY;
    let mut d = 1.0 / b;
    let mut h = d;
    for iteration in 1..=MAXIMUM_ITERATION {
        let m = iteration as f64;
        let numerator = -m * (m - a);
        b += 2.0;
        d = numerator * d + b;
        if d.abs() < TINY {
            d = TINY;
        }
        c = b + numerator / c;
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
    (h * (-x + a * x.ln() - ln_gamma(a)).exp()).max(0.0)
}

pub(crate) fn regularized_beta(a: f64, b: f64, x: f64) -> f64 {
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

pub(crate) fn ln_gamma(value: f64) -> f64 {
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
