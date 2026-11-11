use std::collections::BTreeMap;

use grana_measure::Sample;

use crate::format::formatted;
use crate::interaction::{self, Interaction};
use crate::quality::{FrameValue, Metric, Quality, Summary};
use crate::significance::{self, Significance};

#[derive(Clone, Debug)]
pub struct ComparisonSample {
    pub dataset: String,
    pub objects: usize,
    pub values: [Option<Summary>; Metric::COUNT],
    pub frames: BTreeMap<u32, [Option<FrameValue>; Metric::COUNT]>,
}

#[derive(Clone, Copy, Debug)]
pub struct ComparisonFrame {
    pub mean: f64,
    pub lower: f64,
    pub upper: f64,
    pub images: usize,
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub group: String,
    pub key: String,
    pub name: String,
    pub kind: String,
    pub images: usize,
    pub objects: usize,
    pub means: [Option<f64>; Metric::COUNT],
    pub deviations: [Option<f64>; Metric::COUNT],
    pub samples: Vec<ComparisonSample>,
    pub frames: Vec<[Option<ComparisonFrame>; Metric::COUNT]>,
}

#[derive(Clone, Debug)]
pub struct ComparisonReport {
    pub groups: Vec<Comparison>,
    pub tests: Vec<Significance>,
    pub interactions: Vec<Interaction>,
}

pub fn comparison<'a>(qualities: impl Iterator<Item = &'a Quality>) -> (ComparisonReport, String) {
    let groups = comparison_of(qualities);
    let tests = significance::all(&groups);
    let interactions = interaction::all(&groups);
    let mut lines = vec![header()];
    for comparison in &groups {
        lines.push(line(comparison));
    }
    (
        ComparisonReport {
            groups,
            tests,
            interactions,
        },
        lines.concat(),
    )
}

pub fn comparison_of<'a>(qualities: impl Iterator<Item = &'a Quality>) -> Vec<Comparison> {
    let mut buckets = BTreeMap::<(String, String, String), Bucket>::new();
    for quality in qualities {
        buckets
            .entry((
                quality.group.clone(),
                quality.key.clone(),
                quality.kind.clone(),
            ))
            .or_default()
            .push(quality);
    }
    let mut compared = Vec::with_capacity(buckets.len());
    for ((group, key, kind), bucket) in buckets {
        let mut means = [None; Metric::COUNT];
        let mut deviations = [None; Metric::COUNT];
        for (index, metric) in Metric::ALL.iter().enumerate() {
            let (mean, deviation) = bucket.pooled(*metric);
            means[index] = mean;
            deviations[index] = deviation;
        }
        let frames = bucket.frames();
        compared.push(Comparison {
            group,
            key,
            name: bucket.name,
            kind,
            images: bucket.images,
            objects: bucket.objects,
            means,
            deviations,
            frames,
            samples: bucket.samples,
        });
    }
    compared
}

pub fn comparison_frames(comparison: &[Comparison]) -> String {
    let mut fields = vec![
        "group", "set_key", "set", "kind", "metric", "frame", "images",
    ];
    fields.extend(["value", "lower", "upper"]);
    let mut lines = vec![grana_measure::csv::line(&fields)];
    for row in comparison {
        for metric in Metric::ALL {
            for (time, frame) in row.frames.iter().enumerate() {
                let Some(frame) = frame[metric.index()] else {
                    continue;
                };
                let values = [
                    format!("{:.decimals$}", frame.mean, decimals = metric.decimals()),
                    format!("{:.decimals$}", frame.lower, decimals = metric.decimals()),
                    format!("{:.decimals$}", frame.upper, decimals = metric.decimals()),
                ];
                lines.push(grana_measure::csv::line(&[
                    row.group.as_str(),
                    row.key.as_str(),
                    row.name.as_str(),
                    row.kind.as_str(),
                    metric.key(),
                    &time.to_string(),
                    &frame.images.to_string(),
                    values[0].as_str(),
                    values[1].as_str(),
                    values[2].as_str(),
                ]));
            }
        }
    }
    lines.concat()
}

pub fn samples(comparison: &[Comparison]) -> String {
    let mut fields = vec!["group", "set_key", "set", "kind", "dataset", "objects"];
    fields.extend(Metric::ALL.map(Metric::key));
    let mut lines = vec![grana_measure::csv::line(&fields)];
    for row in comparison {
        for sample in &row.samples {
            let objects = sample.objects.to_string();
            let mut fields = vec![
                row.group.as_str(),
                row.key.as_str(),
                row.name.as_str(),
                row.kind.as_str(),
                sample.dataset.as_str(),
                objects.as_str(),
            ];
            let values =
                Metric::ALL.map(|metric| formatted(metric.sample(sample), metric.decimals()));
            fields.extend(values.iter().map(String::as_str));
            lines.push(grana_measure::csv::line(&fields));
        }
    }
    lines.concat()
}

fn header() -> String {
    let mut fields = vec!["group", "set_key", "set", "kind", "images", "objects"];
    let means = Metric::ALL.map(|metric| format!("{}_mean", metric.key()));
    let deviations = Metric::ALL.map(|metric| format!("{}_std", metric.key()));
    fields.extend(means.iter().map(String::as_str));
    fields.extend(deviations.iter().map(String::as_str));
    grana_measure::csv::line(&fields)
}

fn line(comparison: &Comparison) -> String {
    let mut fields = vec![
        comparison.group.clone(),
        comparison.key.clone(),
        comparison.name.clone(),
        comparison.kind.clone(),
        comparison.images.to_string(),
        comparison.objects.to_string(),
    ];
    let means = Metric::ALL.map(|metric| formatted(metric.value(comparison), metric.decimals()));
    let deviations =
        Metric::ALL.map(|metric| formatted(metric.deviation(comparison), metric.decimals()));
    fields.extend(means);
    fields.extend(deviations);
    grana_measure::csv::line(&fields.iter().map(String::as_str).collect::<Vec<_>>())
}

#[derive(Default)]
struct Bucket {
    name: String,
    images: usize,
    objects: usize,
    summaries: [Vec<Summary>; Metric::COUNT],
    samples: Vec<ComparisonSample>,
}

impl Bucket {
    fn push(&mut self, quality: &Quality) {
        self.images += 1;
        self.objects += quality.objects;
        self.name.clone_from(&quality.name);
        let values = std::array::from_fn(|index| Metric::ALL[index].summary(quality));
        for (bucket, value) in self.summaries.iter_mut().zip(&values) {
            if let Some(summary) = value {
                bucket.push(*summary);
            }
        }
        let frames = quality
            .frames
            .iter()
            .map(|frame| (frame.time, frame.values))
            .collect::<BTreeMap<_, _>>();
        self.samples.push(ComparisonSample {
            dataset: quality.dataset.clone(),
            objects: quality.objects,
            values,
            frames,
        });
    }

    fn pooled(&self, metric: Metric) -> (Option<f64>, Option<f64>) {
        pooled(&self.summaries[metric.index()])
    }

    fn frames(&self) -> Vec<[Option<ComparisonFrame>; Metric::COUNT]> {
        let span = self
            .samples
            .iter()
            .flat_map(|sample| sample.frames.keys())
            .max()
            .map_or(0, |time| time + 1) as usize;
        let mut frames = vec![[None; Metric::COUNT]; span];
        for metric in Metric::ALL {
            let mut pooled = BTreeMap::<u32, Vec<f64>>::new();
            for sample in &self.samples {
                for (time, values) in &sample.frames {
                    if let Some(value) = values[metric.index()] {
                        pooled.entry(*time).or_default().push(value.mean);
                    }
                }
            }
            for (time, means) in pooled {
                let Some(slot) = frames.get_mut(time as usize) else {
                    continue;
                };
                let mean = means.iter().sum::<f64>() / means.len() as f64;
                let (lower, upper) = Sample::of(&means)
                    .and_then(|sample| sample.mean_interval(0.95))
                    .unwrap_or((mean, mean));
                slot[metric.index()] = Some(ComparisonFrame {
                    mean,
                    lower,
                    upper,
                    images: means.len(),
                });
            }
        }
        frames
    }
}

fn pooled(summaries: &[Summary]) -> (Option<f64>, Option<f64>) {
    let total = summaries
        .iter()
        .map(|summary| summary.objects())
        .sum::<usize>();
    if total == 0 {
        return (None, None);
    }
    let mean = summaries
        .iter()
        .map(|summary| summary.objects() as f64 * summary.mean())
        .sum::<f64>()
        / total as f64;
    let deviation = (total > 1).then(|| {
        let within = summaries
            .iter()
            .filter_map(|summary| {
                summary
                    .deviation()
                    .map(|deviation| (summary.objects() - 1) as f64 * deviation * deviation)
            })
            .sum::<f64>();
        let between = summaries
            .iter()
            .map(|summary| {
                let offset = summary.mean() - mean;
                summary.objects() as f64 * offset * offset
            })
            .sum::<f64>();
        ((within + between) / (total - 1) as f64).max(0.0).sqrt()
    });
    (Some(mean), deviation)
}

pub(crate) fn pairs(groups: &[Comparison]) -> Vec<(&Comparison, &Comparison)> {
    let mut buckets = BTreeMap::<(&str, &str), Vec<&Comparison>>::new();
    for row in groups {
        buckets
            .entry((row.key.as_str(), row.kind.as_str()))
            .or_default()
            .push(row);
    }
    let mut pairs = Vec::new();
    for mut bucket in buckets.into_values() {
        bucket.sort_by(|left, right| left.group.cmp(&right.group));
        for (index, first) in bucket.iter().enumerate() {
            for second in &bucket[index + 1..] {
                pairs.push((*first, *second));
            }
        }
    }
    pairs
}
