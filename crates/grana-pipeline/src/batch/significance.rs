use std::collections::BTreeMap;

use grana_measure::{
    ClusterSample, ClusteredWelch, Level, MannWhitney, benjamini_hochberg, hedges_g_pooled, stars,
};

use super::quality::{Comparison, Metric};

#[derive(Clone, Debug)]
pub struct Significance {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub metric: Metric,
    pub group_a: String,
    pub group_b: String,
    pub test: ClusteredWelch,
    pub rank: MannWhitney,
    pub effect: Option<f64>,
    pub interval: Option<(f64, f64)>,
    pub adjusted: f64,
    pub rank_adjusted: f64,
}

impl Significance {
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
        self.metric.level()
    }

    pub fn samples(&self) -> (usize, usize) {
        (self.test.samples_a(), self.test.samples_b())
    }

    pub fn clusters(&self) -> (usize, usize) {
        (self.test.clusters_a(), self.test.clusters_b())
    }

    pub fn design_effect(&self) -> Option<f64> {
        self.test.design_effect()
    }

    pub fn involves(&self, group: &str) -> bool {
        self.group_a == group || self.group_b == group
    }

    pub fn other(&self, group: &str) -> &str {
        assert!(
            self.involves(group),
            "a group test names the groups it compares"
        );
        if self.group_a == group {
            &self.group_b
        } else {
            &self.group_a
        }
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
        let (low, high) = self.interval?;
        let decimals = self.metric.decimals();
        Some(format!("{low:.*} … {high:.*}", decimals, decimals))
    }
}

pub(super) fn tests(groups: &[Comparison]) -> Vec<Significance> {
    let mut buckets = BTreeMap::<(&str, &str), Vec<&Comparison>>::new();
    for row in groups {
        buckets
            .entry((row.key.as_str(), row.kind.as_str()))
            .or_default()
            .push(row);
    }
    let mut tested = Vec::new();
    for mut bucket in buckets.into_values() {
        bucket.sort_by(|left, right| left.group.cmp(&right.group));
        for (index, first) in bucket.iter().enumerate() {
            for second in &bucket[index + 1..] {
                for metric in Metric::ALL {
                    let a = clusters(first, metric);
                    let b = clusters(second, metric);
                    let Some(test) = ClusteredWelch::of(&a, &b) else {
                        continue;
                    };
                    let Some(rank) = MannWhitney::of(&means(first, metric), &means(second, metric))
                    else {
                        continue;
                    };
                    tested.push(Significance {
                        key: first.key.clone(),
                        name: first.name.clone(),
                        kind: first.kind.clone(),
                        metric,
                        group_a: first.group.clone(),
                        group_b: second.group.clone(),
                        effect: hedges_g_pooled(
                            test.samples_a(),
                            test.mean_a(),
                            test.deviation_a(),
                            test.samples_b(),
                            test.mean_b(),
                            test.deviation_b(),
                        ),
                        interval: test.interval(0.95),
                        test,
                        rank,
                        adjusted: 1.0,
                        rank_adjusted: 1.0,
                    });
                }
            }
        }
    }
    let corrected = benjamini_hochberg(&tested.iter().map(Significance::p).collect::<Vec<_>>());
    let ranked = benjamini_hochberg(&tested.iter().map(Significance::rank_p).collect::<Vec<_>>());
    for ((test, adjusted), ranked) in tested.iter_mut().zip(corrected).zip(ranked) {
        test.adjusted = adjusted;
        test.rank_adjusted = ranked;
    }
    tested.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then(left.kind.cmp(&right.kind))
            .then(left.metric.cmp(&right.metric))
            .then(left.group_a.cmp(&right.group_a))
            .then(left.group_b.cmp(&right.group_b))
    });
    tested
}

pub fn table(tests: &[Significance]) -> String {
    let header = grana_measure::csv::line(&[
        "group_a",
        "group_b",
        "set_key",
        "set",
        "kind",
        "metric",
        "level",
        "samples_a",
        "samples_b",
        "clusters_a",
        "clusters_b",
        "mean_a",
        "std_a",
        "mean_b",
        "std_b",
        "difference",
        "difference_ci_low",
        "difference_ci_high",
        "t",
        "df",
        "p",
        "q",
        "hedges_g",
        "image_median_a",
        "image_median_b",
        "u",
        "z",
        "rank_p",
        "rank_q",
        "cliffs_delta",
        "design_effect",
        "mark",
    ]);
    let mut text = header;
    for test in tests {
        let metric = test.metric;
        let decimals = metric.decimals();
        let interval = test.interval;
        let line = grana_measure::csv::line(&[
            &test.group_a,
            &test.group_b,
            &test.key,
            &test.name,
            &test.kind,
            metric.name(),
            test.level().key(),
            &test.test.samples_a().to_string(),
            &test.test.samples_b().to_string(),
            &test.test.clusters_a().to_string(),
            &test.test.clusters_b().to_string(),
            &format!("{:.*}", decimals, test.test.mean_a()),
            &formatted(test.test.deviation_a(), decimals),
            &format!("{:.*}", decimals, test.test.mean_b()),
            &formatted(test.test.deviation_b(), decimals),
            &format!("{:.*}", decimals, test.test.difference()),
            &formatted(interval.map(|interval| interval.0), decimals),
            &formatted(interval.map(|interval| interval.1), decimals),
            &format!("{:.3}", test.test.t()),
            &format!("{:.2}", test.test.df()),
            &test.p_text(),
            &test.q_text(),
            &test
                .effect
                .map_or_else(String::new, |effect| format!("{effect:.3}")),
            &format!("{:.*}", decimals, test.rank.median_a()),
            &format!("{:.*}", decimals, test.rank.median_b()),
            &format!("{:.1}", test.rank.u()),
            &format!("{:.3}", test.rank.z()),
            &test.rank_p_text(),
            &test.rank_q_text(),
            &format!("{:.3}", test.rank.delta()),
            &test
                .design_effect()
                .map_or_else(String::new, |effect| format!("{effect:.2}")),
            test.mark(),
        ]);
        text.push_str(&line);
    }
    text
}

fn clusters(row: &Comparison, metric: Metric) -> Vec<ClusterSample> {
    row.samples
        .iter()
        .filter_map(|sample| metric.summary_of(sample))
        .filter_map(|summary| {
            ClusterSample::from_summary(summary.objects(), summary.mean(), summary.deviation())
        })
        .collect()
}

fn means(row: &Comparison, metric: Metric) -> Vec<f64> {
    row.samples
        .iter()
        .filter_map(|sample| metric.sample(sample))
        .collect()
}

fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}

fn scientific(value: f64) -> String {
    let text = format!("{value:.3e}");
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let exponent = exponent
        .parse::<i32>()
        .expect("a formatted exponent is an integer");
    format!("{mantissa}e{exponent:+03}")
}
