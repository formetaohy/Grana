use std::collections::BTreeMap;

use grana_measure::{Level, Mask, Measurement, Statistic, Table, channel_keys};
use serde::{Deserialize, Serialize};

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
    pub values: [Option<FrameValue>; Metric::COUNT],
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<QualityFrame>,
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

    pub fn value(self, row: &super::Comparison) -> Option<f64> {
        row.means[self.index()]
    }

    pub fn deviation(self, row: &super::Comparison) -> Option<f64> {
        row.deviations[self.index()]
    }

    pub fn sample(self, sample: &super::ComparisonSample) -> Option<f64> {
        self.summary_of(sample).map(Summary::mean)
    }

    pub fn summary_of(self, sample: &super::ComparisonSample) -> Option<Summary> {
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

pub fn of(table: &Table, rows: &[usize], input: &QualityInput<'_>, keys: &[String]) -> Quality {
    let intensity = Measurement::Intensity {
        channel: input.channel,
        statistic: Statistic::Mean,
    }
    .key(keys);
    let columns = [
        None,
        table.column("volume_um3"),
        table.column("area_um2"),
        table
            .column("diameter_um")
            .or_else(|| table.column("length_um")),
        table.column(&intensity),
    ];
    let mut admitted = vec![false; table.rows()];
    for row in rows {
        admitted[*row] = true;
    }
    let gated = Mask::of(table.rows(), |row| admitted[row]);
    Quality {
        dataset: input.dataset.to_string(),
        group: input.group.to_string(),
        key: input.key.to_string(),
        name: input.name.to_string(),
        kind: input.kind.to_string(),
        objects: rows.len(),
        volume: summary(table, rows, columns[Metric::Volume.index()]),
        area: summary(table, rows, columns[Metric::Area.index()]),
        size: summary(table, rows, columns[Metric::Size.index()]),
        intensity: summary(table, rows, columns[Metric::Intensity.index()]),
        frames: frame_series(table, &gated, &columns),
    }
}

fn summary(table: &Table, rows: &[usize], column: Option<usize>) -> Option<Summary> {
    let column = column?;
    let values = rows
        .iter()
        .map(|row| f64::from(table.value(*row, column)))
        .collect::<Vec<_>>();
    Summary::of(&values)
}

fn frame_series(
    table: &Table,
    gated: &Mask,
    columns: &[Option<usize>; Metric::COUNT],
) -> Vec<QualityFrame> {
    let trends = columns.map(|column| column.map(|column| table.trend(column, gated)));
    let counts = frame_counts(table, gated);
    let mut times = counts.keys().copied().collect::<Vec<_>>();
    for trend in trends.iter().flatten() {
        times.extend(trend.points().iter().map(|point| point.time()));
    }
    times.sort_unstable();
    times.dedup();
    times
        .into_iter()
        .map(|time| QualityFrame {
            time,
            values: std::array::from_fn(|index| {
                if index == Metric::Objects.index() {
                    return counts.get(&time).map(|count| FrameValue {
                        count: *count,
                        mean: *count as f64,
                        deviation: None,
                    });
                }
                trends[index]
                    .as_ref()
                    .and_then(|trend| trend.points().iter().find(|point| point.time() == time))
                    .map(|point| FrameValue {
                        count: point.count(),
                        mean: f64::from(point.value()),
                        deviation: point.deviation().map(f64::from),
                    })
            }),
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
        "volume_um3",
        "area_um2",
        "size_um",
        "intensity",
    ])];
    for dataset in datasets {
        lines.extend(dataset.quality.iter().map(|quality| {
            grana_measure::csv::line(&[
                &dataset.group,
                &dataset.name,
                &dataset.scene,
                dataset.status,
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

fn formatted(value: Option<f64>, decimals: usize) -> String {
    match value {
        Some(value) => format!("{value:.decimals$}"),
        None => String::new(),
    }
}
