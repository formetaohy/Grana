use grana_object::{
    FilamentSet, IntensityTable, SampleAccumulator, SampleStatistics, SpotSet, SurfaceSet, TrackSet,
};

use crate::column::{Column, Unit};
use crate::csv;
use crate::filter::Filter;
use crate::lasso::Lasso;
use crate::metric::{Measurement, TrackMeasurement};
use crate::relation::{RelationField, RelationRow};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregate {
    Mean,
    Sum,
    Minimum,
    Maximum,
    Deviation,
}

impl Aggregate {
    pub const ALL: [Self; 5] = [
        Self::Mean,
        Self::Sum,
        Self::Minimum,
        Self::Maximum,
        Self::Deviation,
    ];

    pub fn index(self) -> u8 {
        match self {
            Self::Mean => 0,
            Self::Sum => 1,
            Self::Minimum => 2,
            Self::Maximum => 3,
            Self::Deviation => 4,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Sum => "sum",
            Self::Minimum => "minimum",
            Self::Maximum => "maximum",
            Self::Deviation => "deviation",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Sum => "sum",
            Self::Minimum => "min",
            Self::Maximum => "max",
            Self::Deviation => "std",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|aggregate| aggregate.key() == key)
    }

    pub fn apply(self, statistics: SampleStatistics) -> f32 {
        match self {
            Self::Mean => statistics.mean(),
            Self::Sum => statistics.sum() as f32,
            Self::Minimum => statistics.minimum(),
            Self::Maximum => statistics.maximum(),
            Self::Deviation => statistics.deviation(),
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
}

impl<'a> Frame<'a> {
    pub fn new(time: u32, objects: Objects<'a>, relations: &'a [RelationRow<'a>]) -> Self {
        Self {
            time,
            objects,
            relations,
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
}

impl ObjectCell {
    fn column(
        &self,
        channels: &[String],
        keys: &[String],
        relations: &[RelationRow<'_>],
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
        }
    }

    fn value(
        &self,
        frame: &Frame<'_>,
        object: usize,
        tracks: Option<&TrackSet>,
        frame_interval: f32,
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
        }
        let keys = channel_keys(channels);
        match group {
            Group::Object => object_table(frames, tracks, channels, &keys, frame_interval),
            Group::Track => track_table(frames, tracks, channels, &keys, aggregate),
            Group::Timepoint => timepoint_table(frames, channels, &keys, frame_interval, aggregate),
        }
    }

    fn new(
        columns: Vec<Column>,
        identity: usize,
        observations: Vec<Observation>,
        values: Vec<f32>,
    ) -> Self {
        assert_eq!(
            values.len(),
            observations.len() * columns.len(),
            "every observation carries every column"
        );
        assert!(!columns.is_empty(), "a table has at least one column");
        assert!(identity < columns.len(), "a table keeps an identity column");
        Self {
            columns,
            identity,
            observations,
            values,
        }
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

    pub fn matching(
        &self,
        filters: &[Filter],
        lasso: Option<&Lasso>,
        frame: Option<u32>,
    ) -> Vec<usize> {
        (0..self.rows())
            .filter(|row| {
                frame.is_none_or(|frame| self.observations[*row].time() == Some(frame))
                    && filters
                        .iter()
                        .all(|filter| filter.contains(self.value(*row, filter.column())))
                    && lasso.is_none_or(|lasso| {
                        lasso.contains(self.value(*row, lasso.x()), self.value(*row, lasso.y()))
                    })
            })
            .collect()
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
    let columns = cells
        .iter()
        .map(|cell| cell.column(channels, keys, frames[0].relations))
        .collect::<Vec<_>>();
    let mut observations = Vec::new();
    let mut values = Vec::new();
    for frame in frames {
        for object in 0..frame.objects.count() {
            let track = tracks.and_then(|set| set.track_at(frame.time, object));
            observations.push(Observation {
                time: Some(frame.time),
                object: Some(object),
                track,
            });
            for cell in &cells {
                values.push(cell.value(frame, object, tracks, frame_interval));
            }
        }
    }
    Table::new(columns, 4, observations, values)
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
        return Table::new(columns, TrackMeasurement::ALL.len(), Vec::new(), Vec::new());
    };
    let rows = set.len();
    let slots = measurements.len() + relations.len();
    let mut accumulators = vec![SampleAccumulator::new(); rows * slots];
    let mut counts = vec![0u32; rows * relations.len()];
    for frame in frames {
        for object in 0..frame.objects.count() {
            let Some(track) = set.track_at(frame.time, object) else {
                continue;
            };
            for (index, measurement) in measurements.iter().enumerate() {
                accumulators[track * slots + index].push(measurement.value(frame.objects, object));
            }
            for (slot, (index, field)) in relations.iter().enumerate() {
                let value = field.value(frame.relations[*index], object);
                if value.is_finite() {
                    accumulators[track * slots + measurements.len() + slot].push(value);
                    counts[track * relations.len() + slot] += 1;
                }
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
        for index in 0..measurements.len() {
            values.push(aggregate.apply(accumulators[track * slots + index].statistics()));
        }
        for slot in 0..relations.len() {
            values.push(if counts[track * relations.len() + slot] == 0 {
                f32::NAN
            } else {
                aggregate
                    .apply(accumulators[track * slots + measurements.len() + slot].statistics())
            });
        }
    }
    Table::new(columns, TrackMeasurement::ALL.len(), observations, values)
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
            let mut accumulator = SampleAccumulator::new();
            for object in 0..frame.objects.count() {
                accumulator.push(measurement.value(frame.objects, object));
            }
            values.push(aggregate.apply(accumulator.statistics()));
        }
        for (index, field) in &relations {
            let mut accumulator = SampleAccumulator::new();
            let mut count = 0u32;
            for object in 0..frame.objects.count() {
                let value = field.value(frame.relations[*index], object);
                if value.is_finite() {
                    accumulator.push(value);
                    count += 1;
                }
            }
            values.push(if count == 0 {
                f32::NAN
            } else {
                aggregate.apply(accumulator.statistics())
            });
        }
    }
    Table::new(columns, TimeField::ALL.len(), observations, values)
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
    const FIELDS: [RelationField; 3] = [
        RelationField::Distance,
        RelationField::Inside,
        RelationField::Contact,
    ];
    let mut fields = Vec::with_capacity(relations.len() * FIELDS.len());
    for index in 0..relations.len() {
        for field in FIELDS {
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

fn same_kind(left: Objects<'_>, right: Objects<'_>) -> bool {
    matches!(
        (left, right),
        (Objects::Spots(_), Objects::Spots(_))
            | (Objects::Surfaces(_), Objects::Surfaces(_))
            | (Objects::Filaments(_), Objects::Filaments(_))
    )
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
