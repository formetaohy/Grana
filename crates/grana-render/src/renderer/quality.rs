const UTILIZATION: f32 = 0.9;
const FLOOR: f32 = 1.0;
const CEILING: f32 = 32.0;
const FORGETTING: f32 = 0.9;
const DAMPING: f32 = 0.6;
const DEADBAND: f32 = 0.05;
const MIN_UPDATES: u32 = 3;
const MIN_VARIATION: f32 = 1.1;
const OTHER_SMOOTHING: f32 = 0.2;
const MIN_VOLUME: f32 = 0.05;
const MIN_AVAILABLE: f32 = 1.0e-3;
const SLOPE_LIMIT: f32 = 1.0e4;
const INTERCEPT_LIMIT: f32 = 1.0e3;
const STIFFNESS: f32 = 1.0e4;

#[derive(Clone, Copy, Debug)]
struct Fit {
    slope: f32,
    intercept: f32,
    covariance: [[f32; 2]; 2],
    updates: u32,
    lowest: f32,
    highest: f32,
}

impl Fit {
    fn new() -> Self {
        Self {
            slope: 0.0,
            intercept: 0.0,
            covariance: [[STIFFNESS, 0.0], [0.0, STIFFNESS]],
            updates: 0,
            lowest: f32::INFINITY,
            highest: 0.0,
        }
    }

    fn observe(&mut self, sample: f32, elapsed: f32) {
        assert!(
            sample.is_finite() && sample > 0.0 && elapsed.is_finite() && elapsed >= 0.0,
            "a cost sample pairs a positive rate with a finite duration"
        );
        let phi = [sample, 1.0];
        let covariance = self.covariance;
        let across = [
            covariance[0][0] * phi[0] + covariance[0][1] * phi[1],
            covariance[1][0] * phi[0] + covariance[1][1] * phi[1],
        ];
        let denominator = FORGETTING + phi[0] * across[0] + phi[1] * across[1];
        let gain = [across[0] / denominator, across[1] / denominator];
        let residual = elapsed - (self.slope * phi[0] + self.intercept);
        self.slope = (self.slope + gain[0] * residual).clamp(0.0, SLOPE_LIMIT);
        self.intercept = (self.intercept + gain[1] * residual).clamp(0.0, INTERCEPT_LIMIT);
        for row in 0..2 {
            for column in 0..2 {
                self.covariance[row][column] =
                    (covariance[row][column] - gain[row] * across[column]) / FORGETTING;
            }
        }
        self.updates = self.updates.saturating_add(1);
        self.lowest = self.lowest.min(sample);
        self.highest = self.highest.max(sample);
    }

    fn identified(&self) -> bool {
        self.updates >= MIN_UPDATES
            && self.slope > 0.0
            && self.highest > self.lowest * MIN_VARIATION
    }

    fn predict(&self, sample: f32) -> f32 {
        self.slope * sample + self.intercept
    }

    fn solve(&self, available: f32) -> Option<f32> {
        if !self.identified() {
            return None;
        }
        let room = available - self.intercept;
        if room <= MIN_AVAILABLE {
            return None;
        }
        Some(self.slope / room)
    }
}

pub struct Governor {
    target: f32,
    penalty: f32,
    fit: Fit,
    other: f32,
    volume: f32,
    measured: bool,
}

impl Governor {
    pub fn new(target: f32) -> Self {
        Self {
            target: validated(target),
            penalty: FLOOR,
            fit: Fit::new(),
            other: 0.0,
            volume: 0.0,
            measured: false,
        }
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn penalty(&self) -> f32 {
        self.penalty
    }

    pub fn retarget(&mut self, target: f32) {
        self.target = validated(target);
    }

    pub fn forget(&mut self) {
        *self = Self::new(self.target);
    }

    pub fn observe(&mut self, volume: f32, total: f32, penalty: f32) {
        assert!(
            volume.is_finite() && volume >= 0.0,
            "a measured volume pass spans a finite duration"
        );
        assert!(
            total.is_finite() && total >= volume,
            "a measured frame spans at least its volume pass"
        );
        assert!(
            penalty.is_finite() && penalty >= FLOOR,
            "a measured frame samples at least the base rate"
        );
        if volume < MIN_VOLUME {
            return;
        }
        let other = (total - volume).max(0.0);
        self.other = if self.measured {
            self.other + OTHER_SMOOTHING * (other - self.other)
        } else {
            other
        };
        self.measured = true;
        self.volume = volume;
        self.fit.observe(1.0 / penalty, volume);
    }

    pub fn decide(&mut self, interactive: bool, scalable: bool) -> f32 {
        if !interactive || !scalable || !self.measured {
            return FLOOR;
        }
        let available = self.target * UTILIZATION - self.other;
        if available <= MIN_AVAILABLE {
            self.penalty = CEILING;
            return self.penalty;
        }
        if !self.fit.identified() {
            let ratio = self.volume / available;
            if ratio > 1.0 {
                self.penalty = (self.penalty * ratio.clamp(1.0, 2.0)).min(CEILING);
            } else if ratio < 0.5 {
                self.penalty = (self.penalty * 0.9).max(FLOOR);
            }
            return self.penalty;
        }
        let Some(desired) = self.fit.solve(available) else {
            self.penalty = CEILING;
            return self.penalty;
        };
        let desired = desired.clamp(FLOOR, CEILING);
        let predicted = self.fit.predict(1.0 / self.penalty);
        if (predicted - available).abs() <= available * DEADBAND {
            return self.penalty;
        }
        let next = self.penalty + (desired - self.penalty) * DAMPING;
        self.penalty = next
            .clamp(self.penalty * 0.75, self.penalty * 1.33)
            .clamp(FLOOR, CEILING);
        self.penalty
    }
}

fn validated(target: f32) -> f32 {
    assert!(
        target.is_finite() && target > 0.0,
        "a frame budget is a positive duration"
    );
    target
}
