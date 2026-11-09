use std::collections::BTreeMap;

use grana_measure::{Measurement, Statistic, Table, channel_keys};
use grana_scene::{ObjectSetId, Scene};
use serde::{Deserialize, Serialize};

use super::Plan;
use super::significance::{self, Significance};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quality {
    pub dataset: String,
    pub group: String,
    pub key: String,
    pub name: String,
    pub kind: String,
    pub objects: usize,
    pub volume: Option<f64>,
    pub area: Option<f64>,
    pub size: Option<f64>,
    pub intensity: Option<f64>,
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
        sample.values[self.index()]
    }

    pub(super) fn read(self, quality: &Quality) -> Option<f64> {
        match self {
            Self::Objects => Some(quality.objects as f64),
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
    let mean = |key: &str| -> Option<f64> {
        let column = table.column(key)?;
        if rows.is_empty() {
            return None;
        }
        let total = rows
            .iter()
            .map(|row| f64::from(table.value(*row, column)))
            .sum::<f64>();
        Some(total / rows.len() as f64)
    };
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
        volume: mean("volume_um3"),
        area: mean("area_um2"),
        size: mean("diameter_um").or_else(|| mean("length_um")),
        intensity: mean(&intensity),
    }
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
                &formatted(quality.volume, 3),
                &formatted(quality.area, 3),
                &formatted(quality.size, 3),
                &formatted(quality.intensity, 2),
            ])
        }));
    }
    lines.concat()
}

#[derive(Clone, Debug)]
pub struct ComparisonSample {
    pub dataset: String,
    pub values: [Option<f64>; Metric::COUNT],
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub group: String,
    pub key: String,
    pub name: String,
    pub kind: String,
    pub datasets: usize,
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
    let mut groups = BTreeMap::<(String, String, String), Summary>::new();
    for quality in qualities {
        groups
            .entry((
                quality.group.clone(),
                quality.key.clone(),
                quality.kind.clone(),
            ))
            .or_default()
            .push(quality);
    }
    let mut compared = Vec::with_capacity(groups.len());
    for ((group, key, kind), summary) in groups {
        compared.push(Comparison {
            group,
            key,
            name: summary.name,
            kind,
            datasets: summary.datasets,
            means: std::array::from_fn(|index| mean(&summary.values[index])),
            deviations: std::array::from_fn(|index| deviation(&summary.values[index])),
            samples: summary.samples,
        });
    }
    compared
}

pub(super) fn samples(comparison: &[Comparison]) -> String {
    let mut fields = vec!["group", "set_key", "set", "kind", "dataset"];
    fields.extend(Metric::ALL.map(Metric::key));
    let mut lines = vec![grana_measure::csv::line(&fields)];
    for row in comparison {
        for sample in &row.samples {
            let mut fields = vec![
                row.group.as_str(),
                row.key.as_str(),
                row.name.as_str(),
                row.kind.as_str(),
                sample.dataset.as_str(),
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
    let mut fields = vec!["group", "set_key", "set", "kind", "datasets"];
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
        comparison.datasets.to_string(),
    ];
    let means = Metric::ALL.map(|metric| formatted(metric.value(comparison), metric.decimals()));
    let deviations =
        Metric::ALL.map(|metric| formatted(metric.deviation(comparison), metric.decimals()));
    fields.extend(means);
    fields.extend(deviations);
    grana_measure::csv::line(&fields.iter().map(String::as_str).collect::<Vec<_>>())
}

#[derive(Default)]
struct Summary {
    name: String,
    datasets: usize,
    values: [Vec<f64>; Metric::COUNT],
    samples: Vec<ComparisonSample>,
}

impl Summary {
    fn push(&mut self, quality: &Quality) {
        self.datasets += 1;
        self.name.clone_from(&quality.name);
        let values = std::array::from_fn(|index| Metric::ALL[index].read(quality));
        for (bucket, value) in self.values.iter_mut().zip(values) {
            if let Some(value) = value {
                bucket.push(value);
            }
        }
        self.samples.push(ComparisonSample {
            dataset: quality.dataset.clone(),
            values,
        });
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    Some(values.iter().sum::<f64>() / values.len() as f64)
}

fn deviation(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = mean(values)?;
    let variance = values
        .iter()
        .map(|value| (value - mean) * (value - mean))
        .sum::<f64>()
        / (values.len() - 1) as f64;
    Some(variance.sqrt())
}

fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}
