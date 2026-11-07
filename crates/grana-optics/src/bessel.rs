const ACCURACY: f64 = 8.0;

pub fn j0(x: f64) -> f64 {
    let ax = x.abs();
    if ax < ACCURACY {
        let y = x * x;
        let numerator = 57568490574.0
            + y * (-13362590354.0
                + y * (651619640.7 + y * (-11214424.18 + y * (77392.33017 + y * -184.9052456))));
        let denominator = 57568490411.0
            + y * (1029532985.0 + y * (9494680.718 + y * (59272.64853 + y * (267.8532712 + y))));
        numerator / denominator
    } else {
        let z = ACCURACY / ax;
        let y = z * z;
        let phase = ax - 0.785398164;
        let first = 1.0
            + y * (-0.1098628627e-2
                + y * (0.2734510407e-4 + y * (-0.2073370639e-5 + y * 0.2093887211e-6)));
        let second = -0.1562499995e-1
            + y * (0.1430488765e-3
                + y * (-0.6911147651e-5 + y * (0.7621095161e-6 + y * -0.934935152e-7)));
        (std::f64::consts::FRAC_2_PI / ax).sqrt() * (phase.cos() * first - z * phase.sin() * second)
    }
}

#[cfg(test)]
pub fn j1(x: f64) -> f64 {
    let ax = x.abs();
    if ax < ACCURACY {
        let y = x * x;
        let numerator = x
            * (72362614232.0
                + y * (-7895059235.0
                    + y * (242396853.1
                        + y * (-2972611.439 + y * (15704.48260 + y * -30.16036606)))));
        let denominator = 144725228442.0
            + y * (2300535178.0 + y * (18583304.74 + y * (99447.43394 + y * (376.9991397 + y))));
        numerator / denominator
    } else {
        let z = ACCURACY / ax;
        let y = z * z;
        let phase = ax - 2.356194491;
        let first = 1.0
            + y * (0.183105e-2
                + y * (-0.3516396496e-4 + y * (0.2457520174e-5 + y * -0.240337019e-6)));
        let second = 0.04687499995
            + y * (-0.2002690873e-3
                + y * (0.8449199096e-5 + y * (-0.88228987e-6 + y * 0.105787412e-6)));
        let magnitude = (std::f64::consts::FRAC_2_PI / ax).sqrt()
            * (phase.cos() * first - z * phase.sin() * second);
        if x < 0.0 { -magnitude } else { magnitude }
    }
}
