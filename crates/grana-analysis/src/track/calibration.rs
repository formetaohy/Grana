const SCORE_GAIN: f64 = 6.0;
const SCORE_PRIOR: f64 = 0.5;
const DIVISION_PRIOR: f64 = 3.0;
const MISSING_COST: f64 = 1.5;
const SPURIOUS_COST: f64 = 1.0;
const ENUMERATION_LIMIT: usize = 1 << 15;

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

impl Hypothesis {
    fn members(&self) -> &[u32] {
        &self.nodes[..self.count as usize]
    }
}

fn log_odds(hypothesis: &Hypothesis) -> f64 {
    SCORE_GAIN * hypothesis.score - SCORE_PRIOR + hypothesis.family.offset() + hypothesis.gap
}

pub(super) fn selection(hypothesis: &Hypothesis, posterior: f64) -> f64 {
    let posterior = posterior.clamp(1.0e-9, 1.0 - 1.0e-9);
    let logit = (posterior / (1.0 - posterior)).ln();
    f64::from(hypothesis.edges) * (logit + (MISSING_COST / SPURIOUS_COST).ln())
}

pub(super) struct Marginals {
    probability: Vec<f64>,
}

impl Marginals {
    pub(super) fn probability(&self, index: usize) -> f64 {
        self.probability[index]
    }
}

pub(super) fn marginals(
    hypotheses: &[Hypothesis],
    members: &[usize],
    incidents: &[Vec<u32>],
    nodes: &[usize],
    node_count: usize,
) -> Marginals {
    let mut enumeration = Enumeration {
        hypotheses,
        members,
        incidents,
        nodes,
        used: vec![false; node_count],
        taken: Vec::new(),
        mass: vec![0.0; hypotheses.len()],
        total: 0.0,
        steps: 0,
        exhausted: false,
    };
    enumeration.branch(0, 1.0);
    let mut probability = vec![0.0; hypotheses.len()];
    if enumeration.exhausted || !enumeration.total.is_finite() || enumeration.total <= 0.0 {
        return Marginals {
            probability: mean_field(hypotheses),
        };
    }
    for (position, &index) in members.iter().enumerate() {
        probability[index] = enumeration.mass[position] / enumeration.total;
    }
    Marginals { probability }
}

fn mean_field(hypotheses: &[Hypothesis]) -> Vec<f64> {
    hypotheses
        .iter()
        .map(|hypothesis| 1.0 / (1.0 + (-log_odds(hypothesis)).exp()))
        .collect()
}

struct Enumeration<'a> {
    hypotheses: &'a [Hypothesis],
    members: &'a [usize],
    incidents: &'a [Vec<u32>],
    nodes: &'a [usize],
    used: Vec<bool>,
    taken: Vec<usize>,
    mass: Vec<f64>,
    total: f64,
    steps: usize,
    exhausted: bool,
}

impl Enumeration<'_> {
    fn branch(&mut self, cursor: usize, weight: f64) {
        if self.exhausted {
            return;
        }
        self.steps += 1;
        if self.steps > ENUMERATION_LIMIT {
            self.exhausted = true;
            return;
        }
        let mut position = cursor;
        while position < self.nodes.len() && self.used[self.nodes[position]] {
            position += 1;
        }
        if position == self.nodes.len() {
            self.total += weight;
            for &held in &self.taken {
                self.mass[held] += weight;
            }
            return;
        }
        let node = self.nodes[position];
        self.branch(position + 1, weight);
        if self.exhausted {
            return;
        }
        for &incident in &self.incidents[node] {
            self.take(incident as usize, position, weight);
            if self.exhausted {
                return;
            }
        }
    }

    fn take(&mut self, position: usize, cursor: usize, weight: f64) {
        let hypothesis = self.hypotheses[self.members[position]];
        if hypothesis
            .members()
            .iter()
            .any(|node| self.used[*node as usize])
        {
            return;
        }
        for &node in hypothesis.members() {
            self.used[node as usize] = true;
        }
        self.taken.push(position);
        let weight = weight * log_odds(&hypothesis).exp();
        self.branch(cursor + 1, weight);
        self.taken.pop();
        for &node in hypothesis.members() {
            self.used[node as usize] = false;
        }
    }
}
