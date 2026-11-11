use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use grana_object::{FilamentSet, IntensityTable, SpotSet, SurfaceSet, TrackSet};

use crate::child::Child;
use crate::column::{Column, Unit};
use crate::csv;
use crate::filter::Filter;
use crate::lasso::Lasso;
use crate::mask::Mask;
use crate::metric::{Measurement, TrackMeasurement};
use crate::relation::{RelationField, RelationRow};
use crate::sample::Sample;
use crate::spatial;

pub const NEIGHBOUR_KEY: &str = "nearest_distance_um";
pub const NEIGHBOUR_LABEL: &str = "nearest neighbour";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Object,
    Track,
    Timepoint,
}

impl Group {
    pub const ALL: [Self; 3] = [Self::Object, Self::Track, Self::Timepoint];

    pub fn index(self) -> u8 {
        match self {
            Self::Object => 0,
            Self::Track => 1,
            Self::Timepoint => 2,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Object => "objects",
            Self::Track => "tracks",
            Self::Timepoint => "timepoints",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.key() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Aggregate {
    kind: Kind,
    percent: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Mean,
    Sum,
    Minimum,
    Maximum,
    Deviation,
    Mad,
    Quantile,
}

impl Aggregate {
    pub const MEAN: Self = Self {
        kind: Kind::Mean,
        percent: 0,
    };

    pub const SUM: Self = Self {
        kind: Kind::Sum,
        percent: 0,
    };

    pub const MINIMUM: Self = Self {
        kind: Kind::Minimum,
        percent: 0,
    };

    pub const MAXIMUM: Self = Self {
        kind: Kind::Maximum,
        percent: 0,
    };

    pub const DEVIATION: Self = Self {
        kind: Kind::Deviation,
        percent: 0,
    };

    pub const MAD: Self = Self {
        kind: Kind::Mad,
        percent: 0,
    };

    pub const QUANTILES: [u8; 7] = [5, 10, 25, 50, 75, 90, 95];

    pub const ALL: [Self; 13] = [
        Self::MEAN,
        Self::SUM,
        Self::MINIMUM,
        Self::MAXIMUM,
        Self::DEVIATION,
        Self::MAD,
        Self::quantile_unchecked(5),
        Self::quantile_unchecked(10),
        Self::quantile_unchecked(25),
        Self::quantile_unchecked(50),
        Self::quantile_unchecked(75),
        Self::quantile_unchecked(90),
        Self::quantile_unchecked(95),
    ];

    const fn quantile_unchecked(percent: u8) -> Self {
        Self {
            kind: Kind::Quantile,
            percent,
        }
    }

    pub fn quantile(percent: u8) -> Option<Self> {
        Self::QUANTILES
            .contains(&percent)
            .then_some(Self::quantile_unchecked(percent))
    }

    pub fn index(self) -> u8 {
        Self::ALL
            .iter()
            .position(|aggregate| *aggregate == self)
            .expect("an aggregate belongs to the catalog") as u8
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self.kind {
            Kind::Mean => "mean",
            Kind::Sum => "sum",
            Kind::Minimum => "minimum",
            Kind::Maximum => "maximum",
            Kind::Deviation => "deviation",
            Kind::Mad => "MAD",
            Kind::Quantile if self.percent == 50 => "median",
            Kind::Quantile => "quantile",
        }
    }

    pub fn label(self) -> String {
        match self.kind {
            Kind::Mean => "Mean".to_string(),
            Kind::Sum => "Sum".to_string(),
            Kind::Minimum => "Min".to_string(),
            Kind::Maximum => "Max".to_string(),
            Kind::Deviation => "SD (σ)".to_string(),
            Kind::Mad => "MAD".to_string(),
            Kind::Quantile if self.percent == 50 => "Median".to_string(),
            Kind::Quantile => format!("P{}", self.percent),
        }
    }

    pub fn key(self) -> String {
        match self.kind {
            Kind::Mean => "mean".to_string(),
            Kind::Sum => "sum".to_string(),
            Kind::Minimum => "min".to_string(),
            Kind::Maximum => "max".to_string(),
            Kind::Deviation => "std".to_string(),
            Kind::Mad => "mad".to_string(),
            Kind::Quantile if self.percent == 50 => "median".to_string(),
            Kind::Quantile => format!("p{:02}", self.percent),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|aggregate| aggregate.key() == key)
    }

    pub fn apply(self, values: &[f32]) -> f32 {
        let finite = || values.iter().copied().filter(|value| value.is_finite());
        match self.kind {
            Kind::Mean => {
                let mut count = 0u64;
                let mut sum = 0.0f64;
                for value in finite() {
                    count += 1;
                    sum += f64::from(value);
                }
                if count == 0 {
                    f32::NAN
                } else {
                    (sum / count as f64) as f32
                }
            }
            Kind::Sum => finite().map(f64::from).sum::<f64>() as f32,
            Kind::Minimum => finite().fold(f32::NAN, f32::min),
            Kind::Maximum => finite().fold(f32::NAN, f32::max),
            Kind::Deviation => {
                let collected = finite().map(f64::from).collect::<Vec<_>>();
                let Some(sample) = Sample::new(collected) else {
                    return f32::NAN;
                };
                sample.deviation().unwrap_or(0.0) as f32
            }
            Kind::Mad => {
                let collected = finite().map(f64::from).collect::<Vec<_>>();
                Sample::new(collected).map_or(f32::NAN, |sample| sample.mad() as f32)
            }
            Kind::Quantile => {
                let collected = finite().map(f64::from).collect::<Vec<_>>();
                Sample::new(collected).map_or(f32::NAN, |sample| {
                    sample.quantile(self.percent as f64) as f32
                })
            }
        }
    }

    pub(crate) fn of_sample(self, sample: &Sample) -> (f64, Option<(f64, f64)>) {
        match self.kind {
            Kind::Mean => (sample.mean(), sample.mean_interval(0.95)),
            Kind::Sum => (
                sample.mean() * sample.len() as f64,
                sample.mean_interval(0.95).map(|(lower, upper)| {
                    (lower * sample.len() as f64, upper * sample.len() as f64)
                }),
            ),
            Kind::Quantile => (
                sample.quantile(self.percent as f64),
                sample.quantile_interval(self.percent as f64, 0.95),
            ),
            Kind::Minimum => (sample.minimum(), None),
            Kind::Maximum => (sample.maximum(), None),
            Kind::Deviation => (sample.deviation().unwrap_or(0.0), None),
            Kind::Mad => (sample.mad(), None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observation {
    time: Option<u32>,
    object: Option<usize>,
    track: Option<usize>,
}

impl Observation {
    pub fn time(self) -> Option<u32> {
        self.time
    }

    pub fn object(self) -> Option<usize> {
        self.object
    }

    pub fn track(self) -> Option<usize> {
        self.track
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Objects<'a> {
    Spots(&'a SpotSet),
    Surfaces(&'a SurfaceSet),
    Filaments(&'a FilamentSet),
}

impl<'a> Objects<'a> {
    pub fn count(self) -> usize {
        match self {
            Self::Spots(set) => set.len(),
            Self::Surfaces(set) => set.object_count(),
            Self::Filaments(set) => set.object_count(),
        }
    }

    pub fn channels(self) -> usize {
        self.intensities().channels()
    }

    pub(crate) fn intensities(self) -> &'a IntensityTable {
        match self {
            Self::Spots(set) => set.intensities(),
            Self::Surfaces(set) => set.intensities(),
            Self::Filaments(set) => set.intensities(),
        }
    }
}

pub struct Frame<'a> {
    time: u32,
    objects: Objects<'a>,
    relations: &'a [RelationRow<'a>],
    children: &'a [Child<'a>],
}

impl<'a> Frame<'a> {
    pub fn new(
        time: u32,
        objects: Objects<'a>,
        relations: &'a [RelationRow<'a>],
        children: &'a [Child<'a>],
    ) -> Self {
        Self {
            time,
            objects,
            relations,
            children,
        }
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn objects(&self) -> Objects<'a> {
        self.objects
    }
}

enum ObjectCell {
    Frame,
    Time,
    Object,
    Track,
    Measure(Measurement),
    Relation { index: usize, field: RelationField },
    Child { index: usize, field: usize },
}

impl ObjectCell {
    fn column(
        &self,
        channels: &[String],
        keys: &[String],
        relations: &[RelationRow<'_>],
        children: &[Child<'_>],
    ) -> Column {
        match self {
            Self::Frame => Column::new("frame", "frame", Unit::Count, 0),
            Self::Time => Column::new("time", "time_s", Unit::Second, 3),
            Self::Object => Column::new("object", "object", Unit::Count, 0),
            Self::Track => Column::new("track", "track", Unit::Count, 0),
            Self::Measure(measurement) => measurement.column(channels, keys),
            Self::Relation { index, field } => {
                let row = relations[*index];
                field.column(row.key(), row.target(), row.shift())
            }
            Self::Child { index, field } => {
                let child = &children[*index];
                child.fields()[*field].column(child.key(), child.name(), channels, keys)
            }
        }
    }

    fn value(
        &self,
        frame: &Frame<'_>,
        object: usize,
        tracks: Option<&TrackSet>,
        frame_interval: f32,
        children: &[Vec<Vec<f32>>],
    ) -> f32 {
        match self {
            Self::Frame => (frame.time + 1) as f32,
            Self::Time => frame.time as f32 * frame_interval,
            Self::Object => (object + 1) as f32,
            Self::Track => tracks
                .and_then(|set| set.track_at(frame.time, object))
                .map_or(0.0, |track| (track + 1) as f32),
            Self::Measure(measurement) => measurement.value(frame.objects, object),
            Self::Relation { index, field } => field.value(frame.relations[*index], object),
            Self::Child { index, field } => children[*index][*field][object],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimeField {
    Frame,
    Time,
    Objects,
}

impl TimeField {
    const ALL: [Self; 3] = [Self::Frame, Self::Time, Self::Objects];

    fn column(self) -> Column {
        match self {
            Self::Frame => Column::new("frame", "frame", Unit::Count, 0),
            Self::Time => Column::new("time", "time_s", Unit::Second, 3),
            Self::Objects => Column::new("objects", "objects", Unit::Count, 0),
        }
    }

    fn value(self, frame: &Frame<'_>, frame_interval: f32) -> f32 {
        match self {
            Self::Frame => (frame.time + 1) as f32,
            Self::Time => frame.time as f32 * frame_interval,
            Self::Objects => frame.objects.count() as f32,
        }
    }
}

pub struct Table {
    columns: Vec<Column>,
    identity: usize,
    observations: Vec<Observation>,
    values: Vec<f32>,
    group: Group,
    aggregate: Aggregate,
    basis: Option<Arc<Table>>,
    sources: Vec<Option<usize>>,
}

impl Table {
    pub fn build(
        frames: &[Frame<'_>],
        tracks: Option<&TrackSet>,
        channels: &[String],
        frame_interval: f32,
        group: Group,
        aggregate: Aggregate,
    ) -> Self {
        assert!(!frames.is_empty(), "a table covers at least one frame");
        assert!(!channels.is_empty(), "a table names at least one channel");
        assert!(
            frame_interval.is_finite() && frame_interval > 0.0,
            "the frame interval is a positive duration"
        );
        for frame in &frames[1..] {
            assert!(
                same_kind(frames[0].objects, frame.objects),
                "a table covers one object kind"
            );
        }
        for frame in frames {
            assert_eq!(
                frame.objects.channels(),
                channels.len(),
                "every frame measures every named channel"
            );
            assert!(
                frame.relations.len() == frames[0].relations.len()
                    && frame
                        .relations
                        .iter()
                        .zip(frames[0].relations)
                        .all(|(row, first)| row.target() == first.target()
                            && row.shift() == first.shift()),
                "every frame carries the relations of its object set"
            );
            assert!(
                same_children(frame.children, frames[0].children),
                "every frame carries the children of its object set"
            );
        }
        let keys = channel_keys(channels);
        let mut objects = object_table(frames, tracks, channels, &keys, frame_interval);
        objects.attach_neighbours();
        match group {
            Group::Object => objects,
            Group::Track => {
                let mut table = track_table(frames, tracks, channels, &keys, aggregate);
                table.attach(Arc::new(objects));
                table
            }
            Group::Timepoint => {
                let mut table = timepoint_table(frames, channels, &keys, frame_interval, aggregate);
                table.attach(Arc::new(objects));
                table
            }
        }
    }

    fn new(
        columns: Vec<Column>,
        identity: usize,
        observations: Vec<Observation>,
        values: Vec<f32>,
        group: Group,
        aggregate: Aggregate,
    ) -> Self {
        assert_eq!(
            values.len(),
            observations.len() * columns.len(),
            "every observation carries every column"
        );
        assert!(!columns.is_empty(), "a table has at least one column");
        assert!(identity < columns.len(), "a table keeps an identity column");
        let sources = vec![None; columns.len()];
        Self {
            columns,
            identity,
            observations,
            values,
            group,
            aggregate,
            basis: None,
            sources,
        }
    }

    pub fn with_column(mut self, column: Column, values: Vec<f32>) -> Self {
        self.append(column, values, None);
        self
    }

    fn append(&mut self, column: Column, values: Vec<f32>, source: Option<usize>) {
        let rows = self.observations.len();
        assert_eq!(
            values.len(),
            rows,
            "an appended column covers every observation"
        );
        assert!(
            self.columns
                .iter()
                .all(|existing| existing.key() != column.key()),
            "a table names each column once"
        );
        let width = self.columns.len();
        let mut appended = Vec::with_capacity(rows * (width + 1));
        for (row, value) in values.into_iter().enumerate() {
            appended.extend_from_slice(&self.values[row * width..(row + 1) * width]);
            appended.push(value);
        }
        self.columns.push(column);
        self.sources.push(source);
        self.values = appended;
    }

    fn attach_neighbours(&mut self) {
        let (Some(x), Some(y), Some(z)) = (
            self.column("x_um"),
            self.column("y_um"),
            self.column("z_um"),
        ) else {
            return;
        };
        let mut values = vec![f32::NAN; self.rows()];
        let mut frames = BTreeMap::<u32, Vec<(usize, [f32; 3])>>::new();
        for row in 0..self.rows() {
            let point = [self.value(row, x), self.value(row, y), self.value(row, z)];
            if point.iter().any(|value| !value.is_finite()) {
                continue;
            }
            let Some(time) = self.observation(row).time() else {
                continue;
            };
            frames.entry(time).or_default().push((row, point));
        }
        for members in frames.into_values() {
            let points = members.iter().map(|(_, point)| *point).collect::<Vec<_>>();
            let distances = spatial::neighbour_distances(&points);
            for ((row, _), distance) in members.iter().zip(distances) {
                values[*row] = distance;
            }
        }
        self.append(
            Column::new(NEIGHBOUR_LABEL, NEIGHBOUR_KEY, Unit::Micrometer, 3),
            values,
            None,
        );
    }

    fn attach(&mut self, basis: Arc<Table>) {
        self.sources = self
            .columns
            .iter()
            .map(|column| aggregated_source(&basis, column, self.aggregate))
            .collect();
        if let Some(source) = basis.column(NEIGHBOUR_KEY) {
            let base = &basis.columns()[source];
            let column = aggregate_column(base, self.aggregate);
            let values = self.aggregate_basis(&basis, source);
            self.append(column, values, Some(source));
        }
        self.basis = Some(basis);
    }

    fn aggregate_basis(&self, basis: &Table, column: usize) -> Vec<f32> {
        if self.group == Group::Object {
            return Vec::new();
        }
        let key = |row: usize| match self.group {
            Group::Object => None,
            Group::Track => basis.observation(row).track(),
            Group::Timepoint => basis.observation(row).time().map(|time| time as usize),
        };
        let mut buckets = BTreeMap::<usize, Vec<f32>>::new();
        for row in 0..basis.rows() {
            let value = basis.value(row, column);
            if !value.is_finite() {
                continue;
            }
            if let Some(key) = key(row) {
                buckets.entry(key).or_default().push(value);
            }
        }
        (0..self.rows())
            .map(|row| {
                let key = match self.group {
                    Group::Object => None,
                    Group::Track => self.observation(row).track(),
                    Group::Timepoint => self.observation(row).time().map(|time| time as usize),
                };
                key.and_then(|key| buckets.get(&key))
                    .map_or(f32::NAN, |values| self.aggregate.apply(values))
            })
            .collect()
    }

    pub(crate) fn group(&self) -> Group {
        self.group
    }

    pub(crate) fn aggregate(&self) -> Aggregate {
        self.aggregate
    }

    pub(crate) fn basis(&self) -> Option<&Table> {
        self.basis.as_deref()
    }

    pub(crate) fn source(&self, column: usize) -> Option<usize> {
        self.sources.get(column).copied().flatten()
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn column(&self, key: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.key() == key)
    }

    pub fn identity_columns(&self) -> usize {
        self.identity
    }

    pub fn rows(&self) -> usize {
        self.observations.len()
    }

    pub fn value(&self, row: usize, column: usize) -> f32 {
        assert!(
            row < self.rows() && column < self.columns.len(),
            "the cell lies inside the table"
        );
        self.values[row * self.columns.len() + column]
    }

    pub fn observation(&self, row: usize) -> Observation {
        self.observations[row]
    }

    pub fn position(&self, row: usize) -> Option<[f32; 3]> {
        let axis = |key: &str| self.column(key).map(|column| self.value(row, column));
        Some([axis("x_um")?, axis("y_um")?, axis("z_um")?])
    }

    pub(crate) fn gated_points(&self, gated: &Mask) -> Vec<Vec<[f32; 3]>> {
        let basis = self.basis().unwrap_or(self);
        let (Some(x), Some(y), Some(z)) = (
            basis.column("x_um"),
            basis.column("y_um"),
            basis.column("z_um"),
        ) else {
            return Vec::new();
        };
        let explicit = self.basis().is_none();
        let frames = match self.group() {
            Group::Timepoint if !explicit => gated
                .iter()
                .filter_map(|row| self.observation(row).time())
                .collect::<BTreeSet<_>>(),
            _ => BTreeSet::new(),
        };
        let tracks = match self.group() {
            Group::Track if !explicit => gated
                .iter()
                .filter_map(|row| self.observation(row).track())
                .collect::<BTreeSet<_>>(),
            _ => BTreeSet::new(),
        };
        let mut slots = BTreeMap::<u32, Vec<[f32; 3]>>::new();
        for row in 0..basis.rows() {
            let admitted = if explicit {
                gated.contains(row)
            } else {
                match self.group() {
                    Group::Object => gated.contains(row),
                    Group::Timepoint => basis
                        .observation(row)
                        .time()
                        .is_some_and(|time| frames.contains(&time)),
                    Group::Track => basis
                        .observation(row)
                        .track()
                        .is_some_and(|track| tracks.contains(&track)),
                }
            };
            if !admitted {
                continue;
            }
            let point = [
                basis.value(row, x),
                basis.value(row, y),
                basis.value(row, z),
            ];
            if point.iter().any(|value| !value.is_finite()) {
                continue;
            }
            let Some(time) = basis.observation(row).time() else {
                continue;
            };
            slots.entry(time).or_default().push(point);
        }
        slots.into_values().collect()
    }

    pub fn matching(
        &self,
        filters: &[Filter],
        lasso: Option<&Lasso>,
        frame: Option<u32>,
    ) -> Vec<usize> {
        let mut mask = self.frame_mask(frame);
        for filter in filters {
            mask.intersect(&self.filter_mask(*filter));
        }
        if let Some(lasso) = lasso {
            mask.intersect(&self.lasso_mask(lasso));
        }
        mask.to_rows()
    }

    pub fn frame_mask(&self, frame: Option<u32>) -> Mask {
        match frame {
            None => Mask::all(self.rows()),
            Some(frame) => Mask::of(self.rows(), |row| {
                self.observations[row].time() == Some(frame)
            }),
        }
    }

    pub fn filter_mask(&self, filter: Filter) -> Mask {
        assert!(
            filter.column() < self.columns.len(),
            "a filter names a column of its table"
        );
        let column = filter.column();
        Mask::of(self.rows(), |row| filter.contains(self.value(row, column)))
    }

    pub fn lasso_mask(&self, lasso: &Lasso) -> Mask {
        assert!(
            lasso.x() < self.columns.len() && lasso.y() < self.columns.len(),
            "a lasso names columns of its table"
        );
        Mask::of(self.rows(), |row| {
            lasso.contains(self.value(row, lasso.x()), self.value(row, lasso.y()))
        })
    }

    pub fn count(&self, frame: Option<u32>) -> usize {
        match frame {
            None => self.rows(),
            Some(frame) => self
                .observations
                .iter()
                .filter(|observation| observation.time() == Some(frame))
                .count(),
        }
    }

    pub fn nearest(
        &self,
        rows: &[usize],
        axes: &[usize],
        target: &[f32],
        pixels: &[f32],
    ) -> Option<usize> {
        assert!(
            matches!(axes.len(), 1 | 2),
            "a probe names one or two plot axes"
        );
        assert_eq!(target.len(), axes.len(), "a probe targets every plot axis");
        assert_eq!(pixels.len(), axes.len(), "a probe measures every plot axis");
        assert!(
            target.iter().all(|value| value.is_finite()),
            "a probe targets finite values"
        );
        assert!(
            pixels
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
            "a probe measures non-negative pixels"
        );
        let mut scales = Vec::with_capacity(axes.len());
        for (index, axis) in axes.iter().enumerate() {
            let (low, high) = self.extent(*axis)?;
            scales.push(if high > low {
                pixels[index] / (high - low)
            } else {
                0.0
            });
        }
        let mut nearest = None;
        let mut closest = f32::INFINITY;
        for row in rows {
            let mut distance = 0.0;
            let mut finite = true;
            for (index, axis) in axes.iter().enumerate() {
                let value = self.value(*row, *axis);
                if !value.is_finite() {
                    finite = false;
                    break;
                }
                let delta = (value - target[index]) * scales[index];
                distance += delta * delta;
            }
            if finite && distance < closest {
                closest = distance;
                nearest = Some(*row);
            }
        }
        nearest
    }

    pub fn sort(&self, rows: &mut [usize], column: usize, descending: bool) {
        rows.sort_by(|left, right| {
            let ordering = self
                .value(*left, column)
                .total_cmp(&self.value(*right, column));
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }

    pub fn extent(&self, column: usize) -> Option<(f32, f32)> {
        assert!(column < self.columns.len(), "the column exists");
        (0..self.rows()).fold(None, |range, row| {
            let value = self.value(row, column);
            if !value.is_finite() {
                return range;
            }
            Some(match range {
                None => (value, value),
                Some((minimum, maximum)) => (minimum.min(value), maximum.max(value)),
            })
        })
    }

    pub fn csv(&self, rows: &[usize]) -> String {
        csv::write(&self.columns, rows.len(), |row, column| {
            self.value(rows[row], column)
        })
    }
}

fn object_table(
    frames: &[Frame<'_>],
    tracks: Option<&TrackSet>,
    channels: &[String],
    keys: &[String],
    frame_interval: f32,
) -> Table {
    let mut cells = vec![
        ObjectCell::Frame,
        ObjectCell::Time,
        ObjectCell::Object,
        ObjectCell::Track,
    ];
    cells.extend(
        measurements(frames[0].objects, channels.len())
            .into_iter()
            .map(ObjectCell::Measure),
    );
    for index in 0..frames[0].relations.len() {
        for field in RelationField::ALL {
            cells.push(ObjectCell::Relation { index, field });
        }
    }
    for (index, child) in frames[0].children.iter().enumerate() {
        for field in 0..child.fields().len() {
            cells.push(ObjectCell::Child { index, field });
        }
    }
    let columns = cells
        .iter()
        .map(|cell| cell.column(channels, keys, frames[0].relations, frames[0].children))
        .collect::<Vec<_>>();
    let mut observations = Vec::new();
    let mut values = Vec::new();
    for frame in frames {
        let parents = frame.objects.count();
        let children = frame
            .children
            .iter()
            .map(|child| child.columns(parents))
            .collect::<Vec<_>>();
        for object in 0..parents {
            let track = tracks.and_then(|set| set.track_at(frame.time, object));
            observations.push(Observation {
                time: Some(frame.time),
                object: Some(object),
                track,
            });
            for cell in &cells {
                values.push(cell.value(frame, object, tracks, frame_interval, &children));
            }
        }
    }
    Table::new(
        columns,
        4,
        observations,
        values,
        Group::Object,
        Aggregate::MEAN,
    )
}

fn track_table(
    frames: &[Frame<'_>],
    tracks: Option<&TrackSet>,
    channels: &[String],
    keys: &[String],
    aggregate: Aggregate,
) -> Table {
    let measurements = aggregated_measurements(frames[0].objects, channels.len());
    let relations = aggregated_relations(frames[0].relations);
    let mut columns = TrackMeasurement::ALL
        .iter()
        .map(|field| field.column())
        .collect::<Vec<_>>();
    columns.extend(
        measurements
            .iter()
            .map(|measurement| measurement.aggregate_column(channels, keys, aggregate)),
    );
    columns.extend(relations.iter().map(|(index, field)| {
        aggregate_relation_column(frames[0].relations[*index], *field, aggregate)
    }));
    let Some(set) = tracks else {
        return Table::new(
            columns,
            TrackMeasurement::ALL.len(),
            Vec::new(),
            Vec::new(),
            Group::Track,
            aggregate,
        );
    };
    let rows = set.len();
    let slots = measurements.len() + relations.len();
    let mut collected = vec![Vec::<f32>::new(); rows * slots];
    for frame in frames {
        for object in 0..frame.objects.count() {
            let Some(track) = set.track_at(frame.time, object) else {
                continue;
            };
            for (index, measurement) in measurements.iter().enumerate() {
                collected[track * slots + index].push(measurement.value(frame.objects, object));
            }
            for (slot, (index, field)) in relations.iter().enumerate() {
                collected[track * slots + measurements.len() + slot]
                    .push(field.value(frame.relations[*index], object));
            }
        }
    }
    let mut observations = Vec::with_capacity(rows);
    let mut values = Vec::with_capacity(rows * columns.len());
    for track in 0..rows {
        observations.push(Observation {
            time: None,
            object: None,
            track: Some(track),
        });
        for field in &TrackMeasurement::ALL {
            values.push(field.value(track, set));
        }
        for cell in 0..slots {
            values.push(aggregate.apply(&collected[track * slots + cell]));
        }
    }
    Table::new(
        columns,
        TrackMeasurement::ALL.len(),
        observations,
        values,
        Group::Track,
        aggregate,
    )
}

fn timepoint_table(
    frames: &[Frame<'_>],
    channels: &[String],
    keys: &[String],
    frame_interval: f32,
    aggregate: Aggregate,
) -> Table {
    let measurements = aggregated_measurements(frames[0].objects, channels.len());
    let relations = aggregated_relations(frames[0].relations);
    let mut columns = TimeField::ALL
        .iter()
        .map(|field| field.column())
        .collect::<Vec<_>>();
    columns.extend(
        measurements
            .iter()
            .map(|measurement| measurement.aggregate_column(channels, keys, aggregate)),
    );
    columns.extend(relations.iter().map(|(index, field)| {
        aggregate_relation_column(frames[0].relations[*index], *field, aggregate)
    }));
    let mut observations = Vec::with_capacity(frames.len());
    let mut values = Vec::with_capacity(frames.len() * columns.len());
    for frame in frames {
        observations.push(Observation {
            time: Some(frame.time),
            object: None,
            track: None,
        });
        for field in &TimeField::ALL {
            values.push(field.value(frame, frame_interval));
        }
        for measurement in &measurements {
            let mut collected = Vec::with_capacity(frame.objects.count());
            for object in 0..frame.objects.count() {
                collected.push(measurement.value(frame.objects, object));
            }
            values.push(aggregate.apply(&collected));
        }
        for (index, field) in &relations {
            let mut collected = Vec::with_capacity(frame.objects.count());
            for object in 0..frame.objects.count() {
                collected.push(field.value(frame.relations[*index], object));
            }
            values.push(aggregate.apply(&collected));
        }
    }
    Table::new(
        columns,
        TimeField::ALL.len(),
        observations,
        values,
        Group::Timepoint,
        aggregate,
    )
}

fn measurements(objects: Objects<'_>, channels: usize) -> Vec<Measurement> {
    match objects {
        Objects::Spots(_) => Measurement::spots(channels),
        Objects::Surfaces(_) => Measurement::surfaces(channels),
        Objects::Filaments(_) => Measurement::filaments(channels),
    }
}

fn aggregated_measurements(objects: Objects<'_>, channels: usize) -> Vec<Measurement> {
    measurements(objects, channels)
        .into_iter()
        .filter(|measurement| measurement.aggregates())
        .collect()
}

fn aggregated_relations(relations: &[RelationRow<'_>]) -> Vec<(usize, RelationField)> {
    let mut fields = Vec::with_capacity(relations.len() * RelationField::CONTINUOUS.len());
    for index in 0..relations.len() {
        for field in RelationField::CONTINUOUS {
            fields.push((index, field));
        }
    }
    fields
}

fn aggregate_relation_column(
    row: RelationRow<'_>,
    field: RelationField,
    aggregate: Aggregate,
) -> Column {
    let base = field.column(row.key(), row.target(), row.shift());
    Column::new(
        base.label().to_string(),
        format!("{}_{}", base.key(), aggregate.key()),
        base.unit(),
        base.decimals(),
    )
}

fn aggregate_column(base: &Column, aggregate: Aggregate) -> Column {
    Column::new(
        base.label().to_string(),
        format!("{}_{}", base.key(), aggregate.key()),
        base.unit(),
        base.decimals(),
    )
}

fn aggregated_source(basis: &Table, column: &Column, aggregate: Aggregate) -> Option<usize> {
    let suffix = format!("_{}", aggregate.key());
    let base = column.key().strip_suffix(&suffix)?;
    basis.column(base)
}

fn same_kind(left: Objects<'_>, right: Objects<'_>) -> bool {
    matches!(
        (left, right),
        (Objects::Spots(_), Objects::Spots(_))
            | (Objects::Surfaces(_), Objects::Surfaces(_))
            | (Objects::Filaments(_), Objects::Filaments(_))
    )
}

fn same_children(left: &[Child<'_>], right: &[Child<'_>]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.key() == right.key() && left.fields() == right.fields())
}

pub fn channel_keys(channels: &[String]) -> Vec<String> {
    let mut keys = Vec::with_capacity(channels.len());
    for (index, name) in channels.iter().enumerate() {
        let mut key = sanitize(name);
        if key.is_empty() {
            key = format!("channel{}", index + 1);
        }
        while keys.contains(&key) {
            key = format!("{key}_{}", index + 1);
        }
        keys.push(key);
    }
    keys
}

pub(crate) fn sanitize(name: &str) -> String {
    let key = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    key.trim_matches('_').to_string()
}
