use std::collections::BTreeMap;

use grana_measure::{WelchTest, benjamini_hochberg, hedges_g, stars};

use super::quality::{Comparison, Metric};

#[derive(Clone, Debug)]
pub struct Significance {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub metric: Metric,
    pub group_a: String,
    pub group_b: String,
    pub test: WelchTest,
    pub effect: Option<f64>,
    pub adjusted: f64,
}

impl Significance {
    pub fn p(&self) -> f64 {
        self.test.p()
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
                    let a = sample_metric(first, metric);
                    let b = sample_metric(second, metric);
                    let Some(test) = WelchTest::of(&a, &b) else {
                        continue;
                    };
                    tested.push(Significance {
                        key: first.key.clone(),
                        name: first.name.clone(),
                        kind: first.kind.clone(),
                        metric,
                        group_a: first.group.clone(),
                        group_b: second.group.clone(),
                        test,
                        effect: hedges_g(&a, &b),
                        adjusted: 1.0,
                    });
                }
            }
        }
    }
    let corrected = benjamini_hochberg(&tested.iter().map(Significance::p).collect::<Vec<_>>());
    for (test, adjusted) in tested.iter_mut().zip(corrected) {
        test.adjusted = adjusted;
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
        "datasets_a",
        "datasets_b",
        "mean_a",
        "std_a",
        "mean_b",
        "std_b",
        "difference",
        "t",
        "df",
        "p",
        "q",
        "hedges_g",
        "mark",
    ]);
    let mut text = header;
    for test in tests {
        let metric = test.metric;
        let decimals = metric.decimals();
        let line = grana_measure::csv::line(&[
            &test.group_a,
            &test.group_b,
            &test.key,
            &test.name,
            &test.kind,
            metric.name(),
            &test.test.n_a().to_string(),
            &test.test.n_b().to_string(),
            &format!("{:.*}", decimals, test.test.mean_a()),
            &format!("{:.*}", decimals, test.test.deviation_a()),
            &format!("{:.*}", decimals, test.test.mean_b()),
            &format!("{:.*}", decimals, test.test.deviation_b()),
            &format!("{:.*}", decimals, test.test.difference()),
            &format!("{:.3}", test.test.t()),
            &format!("{:.2}", test.test.df()),
            &test.p_text(),
            &test.q_text(),
            &test
                .effect
                .map_or_else(String::new, |effect| format!("{effect:.3}")),
            test.mark(),
        ]);
        text.push_str(&line);
    }
    text
}

fn sample_metric(row: &Comparison, metric: Metric) -> Vec<f64> {
    row.samples
        .iter()
        .filter_map(|sample| metric.sample(sample))
        .collect()
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
