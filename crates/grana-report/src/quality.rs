use std::collections::BTreeMap;

use grana_measure::{Column, Level, Mask, Measurement, Statistic, Table, Unit, channel_keys};
use serde::{Deserialize, Serialize};

impl std::hash::Hash for Summary {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.objects.hash(state);
        self.mean.to_bits().hash(state);
        self.deviation.map(f64::to_bits).hash(state);
    }
}

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

impl std::hash::Hash for FrameValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.count.hash(state);
        self.mean.to_bits().hash(state);
        self.deviation.map(f64::to_bits).hash(state);
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameValue {
    pub count: u64,
    pub mean: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deviation: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityFrame {
    pub time: u32,
    pub values: Vec<Option<FrameValue>>,
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
    pub metrics: Vec<Metric>,
    pub values: Vec<Option<Summary>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<QualityFrame>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    key: String,
    label: String,
    unit: Unit,
    decimals: u8,
    level: Level,
}

impl Metric {
    pub fn objects() -> Self {
        Self {
            key: "objects".to_string(),
            label: "objects".to_string(),
            unit: Unit::Count,
            decimals: 0,
            level: Level::Image,
        }
    }

    pub fn column(column: &Column) -> Self {
        Self {
            key: column.key().to_string(),
            label: column.label().to_string(),
            unit: column.unit(),
            decimals: column.decimals(),
            level: Level::Object,
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn name(&self) -> &str {
        &self.label
    }

    pub fn unit(&self) -> Unit {
        self.unit
    }

    pub fn decimals(&self) -> usize {
        usize::from(self.decimals)
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn format(&self, value: f64) -> String {
        let number = format!("{value:.*}", self.decimals());
        match self.unit.suffix() {
            "" => number,
            unit => format!("{number} {unit}"),
        }
    }
}

pub struct QualityInput<'a> {
    pub dataset: &'a str,
    pub group: &'a str,
    pub key: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    pub channel: usize,
}

pub struct QualityDataset<'a> {
    pub group: String,
    pub name: String,
    pub scene: String,
    pub status: &'static str,
    pub quality: &'a [Quality],
}

pub fn keys(channels: &[String]) -> Vec<String> {
    channel_keys(channels)
}

fn columns(
    table: &Table,
    channel: usize,
    keys: &[String],
    wanted: &[String],
) -> Result<Vec<Column>, String> {
    if wanted.is_empty() {
        return Ok(default_columns(table, channel, keys));
    }
    wanted
        .iter()
        .map(|key| {
            table
                .column(key)
                .map(|index| table.columns()[index].clone())
                .ok_or_else(|| format!("the table has no column named {key}"))
        })
        .collect()
}

fn default_columns(table: &Table, channel: usize, keys: &[String]) -> Vec<Column> {
    let intensity = Measurement::Intensity {
        channel,
        statistic: Statistic::Mean,
    }
    .key(keys);
    let size = if table.column("diameter_um").is_some() {
        "diameter_um"
    } else {
        "length_um"
    };
    [
        Some("volume_um3"),
        Some("area_um2"),
        Some(size),
        Some(intensity.as_str()),
    ]
    .into_iter()
    .flatten()
    .filter_map(|key| {
        table
            .column(key)
            .map(|index| table.columns()[index].clone())
    })
    .collect()
}

pub fn of(
    table: &Table,
    rows: &[usize],
    input: &QualityInput<'_>,
    keys: &[String],
    wanted: &[String],
) -> Result<Quality, String> {
    let columns = columns(table, input.channel, keys, wanted)?;
    let indices = columns
        .iter()
        .map(|column| {
            table
                .column(column.key())
                .expect("a reported column belongs to the table")
        })
        .collect::<Vec<_>>();
    let mut metrics = vec![Metric::objects()];
    metrics.extend(columns.iter().map(Metric::column));
    let mut values = vec![Summary::counted(rows.len())];
    values.extend(indices.iter().map(|index| summary(table, rows, *index)));
    let mut admitted = vec![false; table.rows()];
    for row in rows {
        admitted[*row] = true;
    }
    let gated = Mask::of(table.rows(), |row| admitted[row]);
    Ok(Quality {
        dataset: input.dataset.to_string(),
        group: input.group.to_string(),
        key: input.key.to_string(),
        name: input.name.to_string(),
        kind: input.kind.to_string(),
        objects: rows.len(),
        frames: frame_series(table, &gated, &metrics, &indices),
        metrics,
        values,
    })
}

fn summary(table: &Table, rows: &[usize], column: usize) -> Option<Summary> {
    let values = rows
        .iter()
        .map(|row| f64::from(table.value(*row, column)))
        .collect::<Vec<_>>();
    Summary::of(&values)
}

fn frame_series(
    table: &Table,
    gated: &Mask,
    metrics: &[Metric],
    indices: &[usize],
) -> Vec<QualityFrame> {
    let trends = indices
        .iter()
        .map(|index| table.trend(*index, gated))
        .collect::<Vec<_>>();
    let counts = frame_counts(table, gated);
    let mut times = counts.keys().copied().collect::<Vec<_>>();
    for trend in &trends {
        times.extend(trend.points().iter().map(|point| point.time()));
    }
    times.sort_unstable();
    times.dedup();
    times
        .into_iter()
        .map(|time| QualityFrame {
            time,
            values: metrics
                .iter()
                .zip([None].into_iter().chain(trends.iter().map(Some)))
                .map(|(metric, trend)| {
                    if metric.level() == Level::Image {
                        return counts.get(&time).map(|count| FrameValue {
                            count: *count,
                            mean: *count as f64,
                            deviation: None,
                        });
                    }
                    trend
                        .and_then(|trend| trend.points().iter().find(|point| point.time() == time))
                        .map(|point| FrameValue {
                            count: point.count(),
                            mean: f64::from(point.value()),
                            deviation: point.deviation().map(f64::from),
                        })
                })
                .collect(),
        })
        .collect()
}

fn frame_counts(table: &Table, gated: &Mask) -> BTreeMap<u32, u64> {
    let first = table.column("first_frame");
    let last = table.column("last_frame");
    let mut counts = BTreeMap::<u32, u64>::new();
    for row in gated.iter() {
        let observation = table.observation(row);
        if let Some(time) = observation.time() {
            *counts.entry(time).or_default() += 1;
            continue;
        }
        let (Some(first), Some(last)) = (first, last) else {
            continue;
        };
        let start = table.value(row, first);
        let end = table.value(row, last);
        if !(start.is_finite() && end.is_finite() && start >= 1.0 && end >= start) {
            continue;
        }
        for time in (start as u32 - 1)..=(end as u32 - 1) {
            *counts.entry(time).or_default() += 1;
        }
    }
    counts
}

pub fn table(datasets: &[QualityDataset<'_>]) -> String {
    let mut lines = vec![grana_measure::csv::line(&[
        "group",
        "dataset",
        "scene",
        "status",
        "set_key",
        "set",
        "kind",
        "objects",
        "metric",
        "unit",
        "mean",
        "deviation",
    ])];
    for dataset in datasets {
        for quality in dataset.quality {
            for (index, metric) in quality.metrics.iter().enumerate() {
                let value = quality.values.get(index).copied().flatten();
                lines.push(grana_measure::csv::line(&[
                    &dataset.group,
                    &dataset.name,
                    &dataset.scene,
                    dataset.status,
                    &quality.key,
                    &quality.name,
                    &quality.kind,
                    &quality.objects.to_string(),
                    metric.key(),
                    metric.unit().suffix(),
                    &value.map_or_else(String::new, |value| {
                        format!("{:.*}", metric.decimals(), value.mean())
                    }),
                    &value.map_or_else(String::new, |value| {
                        value.deviation().map_or_else(String::new, |deviation| {
                            format!("{:.*}", metric.decimals(), deviation)
                        })
                    }),
                ]));
            }
        }
    }
    lines.concat()
}
