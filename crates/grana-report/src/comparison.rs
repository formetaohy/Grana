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
    pub values: Vec<Option<Summary>>,
    pub frames: BTreeMap<u32, Vec<Option<FrameValue>>>,
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
    pub means: Vec<Option<f64>>,
    pub deviations: Vec<Option<f64>>,
    pub samples: Vec<ComparisonSample>,
    pub frames: Vec<Vec<Option<ComparisonFrame>>>,
}

impl Comparison {
    pub fn value(&self, index: usize) -> Option<f64> {
        self.means.get(index).copied().flatten()
    }

    pub fn deviation(&self, index: usize) -> Option<f64> {
        self.deviations.get(index).copied().flatten()
    }
}

#[derive(Clone, Debug)]
pub struct ComparisonReport {
    pub metrics: Vec<Metric>,
    pub groups: Vec<Comparison>,
    pub tests: Vec<Significance>,
    pub interactions: Vec<Interaction>,
}

pub fn comparison<'a>(qualities: impl Iterator<Item = &'a Quality>) -> (ComparisonReport, String) {
    let report = comparison_of(qualities);
    let mut lines = vec![header(&report.metrics)];
    for row in &report.groups {
        lines.push(line(&report.metrics, row));
    }
    (report, lines.concat())
}

pub fn comparison_of<'a>(qualities: impl Iterator<Item = &'a Quality>) -> ComparisonReport {
    let qualities = qualities.collect::<Vec<_>>();
    let metrics = union(&qualities);
    let groups = comparison_of_metrics(&qualities, &metrics);
    let tests = significance::all(&metrics, &groups);
    let interactions = interaction::all(&metrics, &groups);
    ComparisonReport {
        metrics,
        groups,
        tests,
        interactions,
    }
}

fn union(qualities: &[&Quality]) -> Vec<Metric> {
    let mut metrics = Vec::new();
    for quality in qualities {
        for metric in &quality.metrics {
            if !metrics.contains(metric) {
                metrics.push(metric.clone());
            }
        }
    }
    metrics
}

fn comparison_of_metrics(qualities: &[&Quality], metrics: &[Metric]) -> Vec<Comparison> {
    let mut buckets = BTreeMap::<(String, String, String), Vec<&Quality>>::new();
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
        let summaries = metrics
            .iter()
            .map(|metric| {
                bucket
                    .iter()
                    .filter_map(|quality| metric_summary(quality, metric))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let means = summaries
            .iter()
            .map(|summaries| pooled(summaries).0)
            .collect::<Vec<_>>();
        let deviations = summaries
            .iter()
            .map(|summaries| pooled(summaries).1)
            .collect::<Vec<_>>();
        let samples = bucket
            .iter()
            .map(|quality| sample_of(quality, metrics))
            .collect::<Vec<_>>();
        compared.push(Comparison {
            group,
            key,
            name: bucket
                .last()
                .map(|quality| quality.name.clone())
                .unwrap_or_default(),
            kind,
            images: bucket.len(),
            objects: bucket.iter().map(|quality| quality.objects).sum(),
            means,
            deviations,
            frames: frames_of(&samples, metrics.len()),
            samples,
        });
    }
    compared
}

fn metric_summary(quality: &Quality, metric: &Metric) -> Option<Summary> {
    let index = quality.metrics.iter().position(|other| other == metric)?;
    quality.values.get(index).copied().flatten()
}

fn sample_of(quality: &Quality, metrics: &[Metric]) -> ComparisonSample {
    let values = metrics
        .iter()
        .map(|metric| metric_summary(quality, metric))
        .collect::<Vec<_>>();
    let frames = quality
        .frames
        .iter()
        .map(|frame| {
            let values = metrics
                .iter()
                .map(|metric| {
                    let index = quality.metrics.iter().position(|other| other == metric)?;
                    frame.values.get(index).copied().flatten()
                })
                .collect::<Vec<_>>();
            (frame.time, values)
        })
        .collect::<BTreeMap<_, _>>();
    ComparisonSample {
        dataset: quality.dataset.clone(),
        objects: quality.objects,
        values,
        frames,
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

fn frames_of(samples: &[ComparisonSample], metrics: usize) -> Vec<Vec<Option<ComparisonFrame>>> {
    let span = samples
        .iter()
        .flat_map(|sample| sample.frames.keys())
        .max()
        .map_or(0, |time| time + 1) as usize;
    let mut frames = vec![vec![None; metrics]; span];
    for (index, frame) in frames.iter_mut().enumerate() {
        for (metric, slot) in frame.iter_mut().enumerate() {
            let pooled = samples
                .iter()
                .filter_map(|sample| {
                    let value = sample
                        .frames
                        .get(&(index as u32))
                        .and_then(|values| values.get(metric))
                        .copied()
                        .flatten()?;
                    Some(value.mean)
                })
                .collect::<Vec<_>>();
            if pooled.is_empty() {
                continue;
            }
            let mean = pooled.iter().sum::<f64>() / pooled.len() as f64;
            let (lower, upper) = Sample::of(&pooled)
                .and_then(|sample| sample.mean_interval(0.95))
                .unwrap_or((mean, mean));
            *slot = Some(ComparisonFrame {
                mean,
                lower,
                upper,
                images: pooled.len(),
            });
        }
    }
    frames
}

pub fn comparison_frames(metrics: &[Metric], comparison: &[Comparison]) -> String {
    let mut fields = vec![
        "group", "set_key", "set", "kind", "metric", "frame", "images",
    ];
    fields.extend(["value", "lower", "upper"]);
    let mut lines = vec![grana_measure::csv::line(&fields)];
    for row in comparison {
        for (index, metric) in metrics.iter().enumerate() {
            for (time, frame) in row.frames.iter().enumerate() {
                let Some(frame) = frame.get(index).copied().flatten() else {
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

pub fn samples(metrics: &[Metric], comparison: &[Comparison]) -> String {
    let mut fields = vec!["group", "set_key", "set", "kind", "dataset", "objects"];
    fields.extend(metrics.iter().map(Metric::key));
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
            let values = metrics
                .iter()
                .enumerate()
                .map(|(index, metric)| {
                    formatted(
                        sample
                            .values
                            .get(index)
                            .copied()
                            .flatten()
                            .map(Summary::mean),
                        metric.decimals(),
                    )
                })
                .collect::<Vec<_>>();
            fields.extend(values.iter().map(String::as_str));
            lines.push(grana_measure::csv::line(&fields));
        }
    }
    lines.concat()
}

fn header(metrics: &[Metric]) -> String {
    let mut fields = vec!["group", "set_key", "set", "kind", "images", "objects"];
    let means = metrics
        .iter()
        .map(|metric| format!("{}_mean", metric.key()))
        .collect::<Vec<_>>();
    let deviations = metrics
        .iter()
        .map(|metric| format!("{}_std", metric.key()))
        .collect::<Vec<_>>();
    fields.extend(means.iter().map(String::as_str));
    fields.extend(deviations.iter().map(String::as_str));
    grana_measure::csv::line(&fields)
}

fn line(metrics: &[Metric], comparison: &Comparison) -> String {
    let mut fields = vec![
        comparison.group.clone(),
        comparison.key.clone(),
        comparison.name.clone(),
        comparison.kind.clone(),
        comparison.images.to_string(),
        comparison.objects.to_string(),
    ];
    let means = metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| formatted(comparison.value(index), metric.decimals()))
        .collect::<Vec<_>>();
    let deviations = metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| formatted(comparison.deviation(index), metric.decimals()))
        .collect::<Vec<_>>();
    fields.extend(means);
    fields.extend(deviations);
    grana_measure::csv::line(&fields.iter().map(String::as_str).collect::<Vec<_>>())
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
