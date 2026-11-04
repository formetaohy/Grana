use std::collections::BTreeMap;

use grana_measure::{Measurement, Statistic, Table, channel_keys};
use grana_scene::{ObjectSetId, Scene};
use serde::{Deserialize, Serialize};

use super::Plan;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Quality {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) objects: usize,
    pub(super) volume: Option<f64>,
    pub(super) area: Option<f64>,
    pub(super) size: Option<f64>,
    pub(super) intensity: Option<f64>,
}

pub(super) struct Row {
    pub(super) status: &'static str,
    pub(super) objects: usize,
    pub(super) detail: String,
    pub(super) quality: Vec<Quality>,
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
            line(
                dataset.group(),
                &dataset.name(),
                &dataset.scene_label(),
                row.status,
                quality,
            )
        }));
    }
    lines.concat()
}

#[derive(Clone, Debug)]
pub struct Comparison {
    pub group: String,
    pub name: String,
    pub kind: String,
    pub datasets: usize,
    pub objects: Option<f64>,
    pub volume: Option<f64>,
    pub volume_deviation: Option<f64>,
    pub area: Option<f64>,
    pub area_deviation: Option<f64>,
    pub size: Option<f64>,
    pub size_deviation: Option<f64>,
    pub intensity: Option<f64>,
    pub intensity_deviation: Option<f64>,
}

pub(super) fn comparison(plan: &Plan, rows: &[Option<Row>]) -> (Vec<Comparison>, String) {
    let mut groups = BTreeMap::<(String, String, String), Summary>::new();
    for (index, row) in rows.iter().enumerate() {
        let Some(row) = row else {
            continue;
        };
        if row.status != "ok" && row.status != "resumed" {
            continue;
        }
        let group = plan.datasets[index].group();
        for quality in &row.quality {
            groups
                .entry((
                    group.to_string(),
                    quality.name.clone(),
                    quality.kind.clone(),
                ))
                .or_default()
                .push(quality);
        }
    }
    let mut compared = Vec::with_capacity(groups.len());
    for ((group, name, kind), summary) in groups {
        let (volume, volume_deviation) = summary.mean_deviation(&summary.volume);
        let (area, area_deviation) = summary.mean_deviation(&summary.area);
        let (size, size_deviation) = summary.mean_deviation(&summary.size);
        let (intensity, intensity_deviation) = summary.mean_deviation(&summary.intensity);
        compared.push(Comparison {
            group,
            name,
            kind,
            datasets: summary.datasets,
            objects: mean(&summary.objects),
            volume,
            volume_deviation,
            area,
            area_deviation,
            size,
            size_deviation,
            intensity,
            intensity_deviation,
        });
    }
    let mut lines = vec![grana_measure::csv::line(&[
        "group",
        "set",
        "kind",
        "datasets",
        "objects",
        "volume_um3_mean",
        "volume_um3_std",
        "area_um2_mean",
        "area_um2_std",
        "size_um_mean",
        "size_um_std",
        "intensity_mean",
        "intensity_std",
    ])];
    for comparison in &compared {
        lines.push(grana_measure::csv::line(&[
            &comparison.group,
            &comparison.name,
            &comparison.kind,
            &comparison.datasets.to_string(),
            &formatted(comparison.objects, 1),
            &formatted(comparison.volume, 3),
            &formatted(comparison.volume_deviation, 3),
            &formatted(comparison.area, 3),
            &formatted(comparison.area_deviation, 3),
            &formatted(comparison.size, 3),
            &formatted(comparison.size_deviation, 3),
            &formatted(comparison.intensity, 2),
            &formatted(comparison.intensity_deviation, 2),
        ]));
    }
    (compared, lines.concat())
}

#[derive(Default)]
struct Summary {
    datasets: usize,
    objects: Vec<f64>,
    volume: Vec<f64>,
    area: Vec<f64>,
    size: Vec<f64>,
    intensity: Vec<f64>,
}

impl Summary {
    fn push(&mut self, quality: &Quality) {
        self.datasets += 1;
        self.objects.push(quality.objects as f64);
        for (values, value) in [
            (&mut self.volume, quality.volume),
            (&mut self.area, quality.area),
            (&mut self.size, quality.size),
            (&mut self.intensity, quality.intensity),
        ] {
            if let Some(value) = value {
                values.push(value);
            }
        }
    }

    fn mean_deviation(&self, values: &[f64]) -> (Option<f64>, Option<f64>) {
        (mean(values), deviation(values))
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

fn line(group: &str, dataset: &str, scene: &str, status: &str, quality: &Quality) -> String {
    grana_measure::csv::line(&[
        group,
        dataset,
        scene,
        status,
        &quality.name,
        &quality.kind,
        &quality.objects.to_string(),
        &formatted(quality.volume, 3),
        &formatted(quality.area, 3),
        &formatted(quality.size, 3),
        &formatted(quality.intensity, 2),
    ])
}

fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}
