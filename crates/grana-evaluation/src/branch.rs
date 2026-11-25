use std::collections::BTreeSet;

use crate::graph::{Division, Graph};
use crate::matcher::Matching;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Branch {
    true_positive: usize,
    false_positive: usize,
    false_negative: usize,
}

impl Branch {
    pub fn of(truth: &Graph, computed: &Graph, matching: &Matching) -> Self {
        let held = truth
            .divisions()
            .into_iter()
            .map(event)
            .collect::<BTreeSet<_>>();
        let mut claimed = BTreeSet::new();
        let mut true_positive = 0;
        let mut false_positive = 0;
        for division in computed.divisions() {
            let (Some(parent), Some(first), Some(second)) = (
                matching.truth_of_computed(division.parent()),
                matching.truth_of_computed(division.children()[0]),
                matching.truth_of_computed(division.children()[1]),
            ) else {
                false_positive += 1;
                continue;
            };
            let mut found = [parent, first, second];
            found.sort_unstable();
            let found = (found[0], found[1], found[2]);
            if held.contains(&found) {
                true_positive += 1;
                claimed.insert(found);
            } else {
                false_positive += 1;
            }
        }
        Self {
            true_positive,
            false_positive,
            false_negative: held.len() - claimed.len(),
        }
    }

    pub fn true_positive(&self) -> usize {
        self.true_positive
    }

    pub fn false_positive(&self) -> usize {
        self.false_positive
    }

    pub fn false_negative(&self) -> usize {
        self.false_negative
    }

    pub fn f1(&self) -> f64 {
        let doubled = 2 * self.true_positive;
        if doubled == 0 {
            return 0.0;
        }
        doubled as f64 / (doubled + self.false_positive + self.false_negative) as f64
    }
}

fn event(division: Division) -> (usize, usize, usize) {
    let mut found = [
        division.parent(),
        division.children()[0],
        division.children()[1],
    ];
    found.sort_unstable();
    (found[0], found[1], found[2])
}
