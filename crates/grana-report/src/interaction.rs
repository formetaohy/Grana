use grana_measure::{Level, MannWhitney, WelchTest, benjamini_hochberg, hedges_g, stars};

use crate::comparison::{Comparison, ComparisonSample, pairs};
use crate::format::{formatted, scientific};
use crate::quality::Metric;

#[derive(Clone, Debug)]
pub struct Interaction {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub metric: Metric,
    pub group_a: String,
    pub group_b: String,
    pub test: WelchTest,
    pub rank: MannWhitney,
    pub effect: Option<f64>,
    pub adjusted: f64,
    pub rank_adjusted: f64,
}

impl Interaction {
    pub fn p(&self) -> f64 {
        self.test.p()
    }

    pub fn rank_p(&self) -> f64 {
        self.rank.p()
    }

    pub fn q(&self) -> f64 {
        self.adjusted
    }

    pub fn rank_q(&self) -> f64 {
        self.rank_adjusted
    }

    pub fn level(&self) -> Level {
        Level::Image
    }

    pub fn samples(&self) -> (usize, usize) {
        (self.test.n_a(), self.test.n_b())
    }

    pub fn slope_a(&self) -> f64 {
        self.test.mean_a()
    }

    pub fn slope_b(&self) -> f64 {
        self.test.mean_b()
    }

    pub fn difference(&self) -> f64 {
        self.test.difference()
    }

    pub fn interval(&self) -> Option<(f64, f64)> {
        self.test.interval(0.95)
    }

    pub fn mark(&self) -> &'static str {
        stars(self.adjusted)
    }

    pub fn significant(&self) -> bool {
        self.adjusted < 0.05
    }

    pub fn p_text(&self) -> String {
        scientific(self.p())
    }

    pub fn q_text(&self) -> String {
        scientific(self.adjusted)
    }

    pub fn rank_p_text(&self) -> String {
        scientific(self.rank_p())
    }

    pub fn rank_q_text(&self) -> String {
        scientific(self.rank_adjusted)
    }

    pub fn interval_text(&self) -> Option<String> {
        let (low, high) = self.interval()?;
        let decimals = self.metric.decimals();
        Some(format!("{low:.*} – {high:.*}", decimals, decimals))
    }
}

pub fn all(groups: &[Comparison]) -> Vec<Interaction> {
    let mut tested = Vec::new();
    for (first, second) in pairs(groups) {
        for metric in Metric::ALL {
            let a = slopes(first, metric);
            let b = slopes(second, metric);
            let Some(test) = WelchTest::of(&a, &b) else {
                continue;
            };
            let Some(rank) = MannWhitney::of(&a, &b) else {
                continue;
            };
            tested.push(Interaction {
                key: first.key.clone(),
                name: first.name.clone(),
                kind: first.kind.clone(),
                metric,
                group_a: first.group.clone(),
                group_b: second.group.clone(),
                effect: hedges_g(&a, &b),
                test,
                rank,
                adjusted: 1.0,
                rank_adjusted: 1.0,
            });
        }
    }
    let corrected = benjamini_hochberg(&tested.iter().map(Interaction::p).collect::<Vec<_>>());
    let ranked = benjamini_hochberg(&tested.iter().map(Interaction::rank_p).collect::<Vec<_>>());
    for ((test, adjusted), ranked) in tested.iter_mut().zip(corrected).zip(ranked) {
        test.adjusted = adjusted;
        test.rank_adjusted = ranked;
    }
    tested.sort_by(order);
    tested
}

pub fn table(tests: &[Interaction]) -> String {
    let header = grana_measure::csv::line(&[
        "group_a",
        "group_b",
        "set_key",
        "set",
        "kind",
        "metric",
        "level",
        "images_a",
        "images_b",
        "slope_a",
        "slope_b",
        "difference",
        "difference_ci_low",
        "difference_ci_high",
        "t",
        "df",
        "p",
        "q",
        "hedges_g",
        "u",
        "z",
        "rank_p",
        "rank_q",
        "cliffs_delta",
        "mark",
    ]);
    let mut text = header;
    for test in tests {
        let metric = test.metric;
        let decimals = metric.decimals();
        let interval = test.interval();
        text.push_str(&grana_measure::csv::line(&[
            &test.group_a,
            &test.group_b,
            &test.key,
            &test.name,
            &test.kind,
            metric.name(),
            test.level().key(),
            &test.test.n_a().to_string(),
            &test.test.n_b().to_string(),
            &format!("{:.*}", decimals, test.slope_a()),
            &format!("{:.*}", decimals, test.slope_b()),
            &format!("{:.*}", decimals, test.difference()),
            &formatted(interval.map(|interval| interval.0), decimals),
            &formatted(interval.map(|interval| interval.1), decimals),
            &format!("{:.3}", test.test.t()),
            &format!("{:.2}", test.test.df()),
            &test.p_text(),
            &test.q_text(),
            &test
                .effect
                .map_or_else(String::new, |effect| format!("{effect:.3}")),
            &format!("{:.1}", test.rank.u()),
            &format!("{:.3}", test.rank.z()),
            &test.rank_p_text(),
            &test.rank_q_text(),
            &format!("{:.3}", test.rank.delta()),
            test.mark(),
        ]));
    }
    text
}

fn slopes(row: &Comparison, metric: Metric) -> Vec<f64> {
    row.samples
        .iter()
        .filter_map(|sample| slope(sample, metric))
        .collect()
}

fn slope(sample: &ComparisonSample, metric: Metric) -> Option<f64> {
    let points = sample
        .frames
        .iter()
        .filter_map(|(time, values)| {
            let value = values[metric.index()]?;
            Some((f64::from(*time), value.mean))
        })
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return None;
    }
    let count = points.len() as f64;
    let mean_x = points.iter().map(|point| point.0).sum::<f64>() / count;
    let mean_y = points.iter().map(|point| point.1).sum::<f64>() / count;
    let mut spread = 0.0;
    let mut covariance = 0.0;
    for (x, y) in points {
        spread += (x - mean_x) * (x - mean_x);
        covariance += (x - mean_x) * (y - mean_y);
    }
    (spread > 0.0).then(|| covariance / spread)
}

fn order(left: &Interaction, right: &Interaction) -> std::cmp::Ordering {
    left.key
        .cmp(&right.key)
        .then(left.kind.cmp(&right.kind))
        .then(left.metric.cmp(&right.metric))
        .then(left.group_a.cmp(&right.group_a))
        .then(left.group_b.cmp(&right.group_b))
}
