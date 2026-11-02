use grana_object::{
    FilamentSet, IntensityTable, SampleAccumulator, SampleStatistics, SpotSet, SurfaceSet, TrackSet,
};

use crate::column::{Column, Unit};
use crate::csv;
use crate::filter::Filter;
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

    fn apply(self, statistics: SampleStatistics) -> f32 {
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
                let target = relations[*index].target();
                field.column(target, &sanitize(target))
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
                        .all(|(row, first)| row.target() == first.target()),
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

    pub fn matching(&self, filters: &[Filter]) -> Vec<usize> {
        (0..self.rows())
            .filter(|row| {
                filters
                    .iter()
                    .all(|filter| filter.contains(self.value(*row, filter.column())))
            })
            .collect()
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
    let mut columns = TrackMeasurement::ALL
        .iter()
        .map(|field| field.column())
        .collect::<Vec<_>>();
    columns.extend(
        measurements
            .iter()
            .map(|measurement| measurement.aggregate_column(channels, keys, aggregate)),
    );
    let Some(set) = tracks else {
        return Table::new(columns, TrackMeasurement::ALL.len(), Vec::new(), Vec::new());
    };
    let rows = set.len();
    let mut accumulators = vec![SampleAccumulator::new(); rows * measurements.len()];
    for frame in frames {
        for object in 0..frame.objects.count() {
            let Some(track) = set.track_at(frame.time, object) else {
                continue;
            };
            for (index, measurement) in measurements.iter().enumerate() {
                accumulators[track * measurements.len() + index]
                    .push(measurement.value(frame.objects, object));
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
            values.push(
                aggregate.apply(accumulators[track * measurements.len() + index].statistics()),
            );
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
    let mut columns = TimeField::ALL
        .iter()
        .map(|field| field.column())
        .collect::<Vec<_>>();
    columns.extend(
        measurements
            .iter()
            .map(|measurement| measurement.aggregate_column(channels, keys, aggregate)),
    );
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

fn same_kind(left: Objects<'_>, right: Objects<'_>) -> bool {
    matches!(
        (left, right),
        (Objects::Spots(_), Objects::Spots(_))
            | (Objects::Surfaces(_), Objects::Surfaces(_))
            | (Objects::Filaments(_), Objects::Filaments(_))
    )
}

fn channel_keys(channels: &[String]) -> Vec<String> {
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

fn sanitize(name: &str) -> String {
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
