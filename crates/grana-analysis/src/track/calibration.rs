const SCORE_GAIN: f64 = 6.0;
const SCORE_PRIOR: f64 = 0.5;
const DIVISION_PRIOR: f64 = 3.0;
const MISSING_COST: f64 = 1.5;
const SPURIOUS_COST: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Family {
    Link,
    Divide,
    Merge,
}

impl Family {
    fn offset(self) -> f64 {
        match self {
            Self::Link | Self::Merge => 0.0,
            Self::Divide => DIVISION_PRIOR,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Hypothesis {
    pub(super) nodes: [u32; 3],
    pub(super) count: u8,
    pub(super) edges: u8,
    pub(super) family: Family,
    pub(super) score: f64,
    pub(super) gap: f64,
}

pub(super) struct Calibration {
    odds: Vec<f64>,
}

impl Calibration {
    pub(super) fn new(node_count: usize) -> Self {
        Self {
            odds: vec![0.0; node_count],
        }
    }

    pub(super) fn weigh(&mut self, hypotheses: &[Hypothesis]) -> Vec<f64> {
        for hypothesis in hypotheses {
            for node in &hypothesis.nodes[..hypothesis.count as usize] {
                self.odds[*node as usize] = 0.0;
            }
        }
        for hypothesis in hypotheses {
            let odds = log_odds(hypothesis).exp();
            for node in &hypothesis.nodes[..hypothesis.count as usize] {
                self.odds[*node as usize] += odds;
            }
        }
        hypotheses
            .iter()
            .map(|hypothesis| {
                let log = log_odds(hypothesis);
                let odds = log.exp();
                hypothesis.nodes[..hypothesis.count as usize]
                    .iter()
                    .map(|node| log - (1.0 + self.odds[*node as usize] - odds).ln())
                    .sum()
            })
            .collect()
    }
}

fn log_odds(hypothesis: &Hypothesis) -> f64 {
    SCORE_GAIN * hypothesis.score - SCORE_PRIOR + hypothesis.family.offset() + hypothesis.gap
}

pub(super) fn posterior(weight: f64) -> f64 {
    1.0 / (1.0 + (-weight).exp())
}

pub(super) fn selection(hypothesis: &Hypothesis, weight: f64) -> f64 {
    f64::from(hypothesis.edges) * (weight + (MISSING_COST / SPURIOUS_COST).ln())
}

pub(super) fn abstains(hypothesis: &Hypothesis, weight: f64) -> bool {
    selection(hypothesis, weight) <= 0.0
}
