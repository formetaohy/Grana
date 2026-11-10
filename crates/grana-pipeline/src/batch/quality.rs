use std::collections::BTreeMap;

use grana_measure::{Level, Measurement, Statistic, Table, channel_keys};
use grana_scene::{ObjectSetId, Scene};
use serde::{Deserialize, Serialize};

use super::Plan;
use super::significance::{self, Significance};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    objects: usize,
    mean: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deviation: Option<f64>,
}

impl Summary {
    pub fn new(objects: usize, mean: f64, deviation: Option<f64>) -> Option<Self> {
        if objects == 0
            || !mean.is_finite()
            || deviation.is_some_and(|deviation| !deviation.is_finite() || deviation < 0.0)
        {
            return None;
        }
        Some(Self {
            objects,
            mean,
            deviation,
        })
    }

    pub fn of(values: &[f64]) -> Option<Self> {
        if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
            return None;
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let deviation = (values.len() > 1).then(|| {
            let variance = values
                .iter()
                .map(|value| (value - mean) * (value - mean))
                .sum::<f64>()
                / (values.len() - 1) as f64;
            variance.max(0.0).sqrt()
        });
        Self::new(values.len(), mean, deviation)
    }

    pub fn counted(objects: usize) -> Option<Self> {
        Self::new(1, objects as f64, None)
    }

    pub fn objects(self) -> usize {
        self.objects
    }

    pub fn mean(self) -> f64 {
        self.mean
    }

    pub fn deviation(self) -> Option<f64> {
        self.deviation
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub dataset: String,
    pub group: String,
    pub key: String,
    pub name: String,
    pub kind: String,
    pub objects: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Summary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<Summary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Summary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intensity: Option<Summary>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Metric {
    #[default]
    Objects,
    Volume,
    Area,
    Size,
    Intensity,
}

pub(super) struct Row {
    pub(super) status: &'static str,
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
}

impl Metric {
    pub const ALL: [Self; 5] = [
        Self::Objects,
        Self::Volume,
        Self::Area,
        Self::Size,
        Self::Intensity,
    ];

    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        match self {
            Self::Objects => 0,
            Self::Volume => 1,
            Self::Area => 2,
            Self::Size => 3,
            Self::Intensity => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Objects => "Objects",
            Self::Volume => "Volume",
            Self::Area => "Area",
            Self::Size => "Size",
            Self::Intensity => "Intensity",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Objects => "objects",
            Self::Volume => "volume_um3",
            Self::Area => "area_um2",
            Self::Size => "size_um",
            Self::Intensity => "intensity",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Self::Objects | Self::Intensity => "",
            Self::Volume => "µm³",
            Self::Area => "µm²",
            Self::Size => "µm",
        }
    }

    pub fn decimals(self) -> usize {
        match self {
            Self::Objects => 1,
            Self::Volume | Self::Area | Self::Size => 3,
            Self::Intensity => 2,
        }
    }

    pub fn level(self) -> Level {
        match self {
            Self::Objects => Level::Image,
            Self::Volume | Self::Area | Self::Size | Self::Intensity => Level::Object,
        }
    }

    pub fn format(self, value: f64) -> String {
        let number = format!("{value:.*}", self.decimals());
        match self.unit() {
            "" => number,
            unit => format!("{number} {unit}"),
        }
    }

    pub fn value(self, row: &Comparison) -> Option<f64> {
        row.means[self.index()]
    }

    pub fn deviation(self, row: &Comparison) -> Option<f64> {
        row.deviations[self.index()]
    }

    pub fn sample(self, sample: &ComparisonSample) -> Option<f64> {
        self.summary_of(sample).map(Summary::mean)
    }

    pub fn summary_of(self, sample: &ComparisonSample) -> Option<Summary> {
        sample.values[self.index()]
    }

    pub fn summary(self, quality: &Quality) -> Option<Summary> {
        match self {
            Self::Objects => Summary::counted(quality.objects),
            Self::Volume => quality.volume,
            Self::Area => quality.area,
            Self::Size => quality.size,
            Self::Intensity => quality.intensity,
        }
    }
}

pub(super) fn keys(scene: &Scene) -> Vec<String> {
    channel_keys(
        &scene
            .dataset
            .channels()
            .iter()
            .map(|channel| channel.name().to_string())
            .collect::<Vec<_>>(),
    )
}

pub(super) fn of(
    scene: &Scene,
    set: ObjectSetId,
    keys: &[String],
    table: &Table,
    rows: &[usize],
    dataset: &str,
    group: &str,
) -> Quality {
    let object = scene
        .objects
        .get(set)
        .expect("the batch quality names its object set");
    let intensity = Measurement::Intensity {
        channel: object.channel(),
        statistic: Statistic::Mean,
    }
    .key(keys);
    Quality {
        dataset: dataset.to_string(),
        group: group.to_string(),
        key: object.key().as_str().to_string(),
        name: object.name().to_string(),
        kind: object.kind().name().to_string(),
        objects: rows.len(),
        volume: summary(table, rows, Some("volume_um3")),
        area: summary(table, rows, Some("area_um2")),
        size: summary(table, rows, Some("diameter_um"))
            .or_else(|| summary(table, rows, Some("length_um"))),
        intensity: summary(table, rows, Some(intensity.as_str())),
    }
}

fn summary(table: &Table, rows: &[usize], key: Option<&str>) -> Option<Summary> {
    let column = table.column(key?)?;
    let values = rows
        .iter()
        .map(|row| f64::from(table.value(*row, column)))
        .collect::<Vec<_>>();
    Summary::of(&values)
}

pub(super) fn table(plan: &Plan, rows: &[Option<Row>]) -> String {
    let mut lines = vec![grana_measure::csv::line(&[
        "group",
        "dataset",
        "scene",
        "status",
        "set_key",
        "set",
        "kind",
        "objects",
        "volume_um3",
        "area_um2",
        "size_um",
        "intensity",
    ])];
    for (index, row) in rows.iter().enumerate() {
        let Some(row) = row else {
            continue;
        };
        let dataset = &plan.datasets[index];
        lines.extend(row.quality.iter().map(|quality| {
            grana_measure::csv::line(&[
                dataset.group(),
                &dataset.name(),
                &dataset.scene_label(),
                row.status,
                &quality.key,
                &quality.name,
                &quality.kind,
                &quality.objects.to_string(),
                &formatted(quality.volume.map(Summary::mean), 3),
                &formatted(quality.area.map(Summary::mean), 3),
                &formatted(quality.size.map(Summary::mean), 3),
                &formatted(quality.intensity.map(Summary::mean), 2),
            ])
        }));
    }
    lines.concat()
}

#[derive(Clone, Debug)]
pub struct ComparisonSample {
    pub dataset: String,
    pub objects: usize,
    pub values: [Option<Summary>; Metric::COUNT],
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
}

#[derive(Clone, Debug)]
pub struct ComparisonReport {
    pub groups: Vec<Comparison>,
    pub tests: Vec<Significance>,
}

pub(super) fn comparison(rows: &[Option<Row>]) -> (ComparisonReport, String) {
    let groups = comparison_of(rows.iter().flatten().flat_map(|row| {
        let settled = row.status == "ok" || row.status == "resumed";
        row.quality.iter().filter(move |_| settled)
    }));
    let tests = significance::tests(&groups);
    let mut lines = vec![header()];
    for comparison in &groups {
        lines.push(line(comparison));
    }
    (ComparisonReport { groups, tests }, lines.concat())
}

pub(super) fn comparison_of<'a>(qualities: impl Iterator<Item = &'a Quality>) -> Vec<Comparison> {
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
        compared.push(Comparison {
            group,
            key,
            name: bucket.name,
            kind,
            images: bucket.images,
            objects: bucket.objects,
            means,
            deviations,
            samples: bucket.samples,
        });
    }
    compared
}

pub(super) fn samples(comparison: &[Comparison]) -> String {
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
        self.samples.push(ComparisonSample {
            dataset: quality.dataset.clone(),
            objects: quality.objects,
            values,
        });
    }

    fn pooled(&self, metric: Metric) -> (Option<f64>, Option<f64>) {
        pooled(&self.summaries[metric.index()])
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

fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}
