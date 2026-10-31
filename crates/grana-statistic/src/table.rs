use grana_object::{
    IntensityTable, SampleAccumulator, SampleStatistics, SpotSet, SurfaceSet, TrackSet,
};

use crate::column::{Column, Unit};
use crate::csv;
use crate::filter::Filter;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Object,
    Track,
    Timepoint,
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
}

impl<'a> Objects<'a> {
    pub fn count(self) -> usize {
        match self {
            Self::Spots(set) => set.len(),
            Self::Surfaces(set) => set.object_count(),
        }
    }

    pub fn channels(self) -> usize {
        self.intensities().channels()
    }

    fn intensities(self) -> &'a IntensityTable {
        match self {
            Self::Spots(set) => set.intensities(),
            Self::Surfaces(set) => set.intensities(),
        }
    }

    fn value(self, object: usize, measurement: Measurement) -> f32 {
        let intensities = self.intensities();
        match self {
            Self::Spots(set) => {
                let spot = set.spots()[object];
                match measurement {
                    Measurement::Position(axis) => spot.center()[axis],
                    Measurement::Diameter => 2.0 * spot.radius(),
                    Measurement::Contrast => spot.quality(),
                    Measurement::Intensity { channel, statistic } => {
                        statistic.value(intensities.object(object, channel))
                    }
                    Measurement::Volume
                    | Measurement::Area
                    | Measurement::Sphericity
                    | Measurement::Voxels => {
                        panic!("the surface measurement {measurement:?} is read from spots")
                    }
                }
            }
            Self::Surfaces(set) => {
                let statistics = set.objects()[object];
                match measurement {
                    Measurement::Position(axis) => statistics.centroid()[axis],
                    Measurement::Volume => statistics.volume() as f32,
                    Measurement::Area => statistics.area() as f32,
                    Measurement::Sphericity => statistics.sphericity() as f32,
                    Measurement::Voxels => statistics.voxel_count() as f32,
                    Measurement::Intensity { channel, statistic } => {
                        statistic.value(intensities.object(object, channel))
                    }
                    Measurement::Diameter | Measurement::Contrast => {
                        panic!("the spot measurement {measurement:?} is read from surfaces")
                    }
                }
            }
        }
    }
}

pub struct Frame<'a> {
    time: u32,
    objects: Objects<'a>,
}

impl<'a> Frame<'a> {
    pub fn new(time: u32, objects: Objects<'a>) -> Self {
        Self { time, objects }
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn objects(&self) -> Objects<'a> {
        self.objects
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Statistic {
    Mean,
    Sum,
    Minimum,
    Maximum,
    Deviation,
}

impl Statistic {
    const ALL: [Self; 5] = [
        Self::Mean,
        Self::Sum,
        Self::Minimum,
        Self::Maximum,
        Self::Deviation,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Sum => "sum",
            Self::Minimum => "minimum",
            Self::Maximum => "maximum",
            Self::Deviation => "deviation",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Mean => "mean",
            Self::Sum => "sum",
            Self::Minimum => "min",
            Self::Maximum => "max",
            Self::Deviation => "std",
        }
    }

    fn value(self, statistics: SampleStatistics) -> f32 {
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
enum Measurement {
    Position(usize),
    Diameter,
    Contrast,
    Volume,
    Area,
    Sphericity,
    Voxels,
    Intensity {
        channel: usize,
        statistic: Statistic,
    },
}

impl Measurement {
    fn label(self, channels: &[String]) -> String {
        match self {
            Self::Position(axis) => ["x", "y", "z"][axis].to_string(),
            Self::Diameter => "diameter".to_string(),
            Self::Contrast => "contrast".to_string(),
            Self::Volume => "volume".to_string(),
            Self::Area => "area".to_string(),
            Self::Sphericity => "sphericity".to_string(),
            Self::Voxels => "voxels".to_string(),
            Self::Intensity { channel, statistic } => {
                format!("{} · {}", channels[channel], statistic.name())
            }
        }
    }

    fn key(self, keys: &[String]) -> String {
        match self {
            Self::Position(axis) => format!("{}_um", ["x", "y", "z"][axis]),
            Self::Diameter => "diameter_um".to_string(),
            Self::Contrast => "contrast".to_string(),
            Self::Volume => "volume_um3".to_string(),
            Self::Area => "area_um2".to_string(),
            Self::Sphericity => "sphericity".to_string(),
            Self::Voxels => "voxels".to_string(),
            Self::Intensity { channel, statistic } => {
                format!("{}_intensity_{}", keys[channel], statistic.key())
            }
        }
    }

    fn unit(self) -> Unit {
        match self {
            Self::Position(_) | Self::Diameter => Unit::Micrometer,
            Self::Volume => Unit::CubicMicrometer,
            Self::Area => Unit::SquareMicrometer,
            Self::Contrast | Self::Sphericity | Self::Voxels | Self::Intensity { .. } => {
                Unit::Count
            }
        }
    }

    fn decimals(self) -> u8 {
        match self {
            Self::Position(_) | Self::Diameter => 3,
            Self::Contrast | Self::Sphericity => 4,
            Self::Volume | Self::Area => 3,
            Self::Voxels => 0,
            Self::Intensity { .. } => 2,
        }
    }

    fn aggregates(self) -> bool {
        !matches!(self, Self::Position(_))
    }

    fn column(self, channels: &[String], keys: &[String]) -> Column {
        Column::new(
            self.label(channels),
            self.key(keys),
            self.unit(),
            self.decimals(),
        )
    }

    fn aggregate_column(
        self,
        channels: &[String],
        keys: &[String],
        aggregate: Aggregate,
    ) -> Column {
        Column::new(
            self.label(channels),
            format!("{}_{}", self.key(keys), aggregate.key()),
            self.unit(),
            self.decimals(),
        )
    }
}

enum ObjectCell {
    Frame,
    Time,
    Object,
    Track,
    Measure(Measurement),
}

impl ObjectCell {
    fn column(&self, channels: &[String], keys: &[String]) -> Column {
        match self {
            Self::Frame => Column::new("frame", "frame", Unit::Count, 0),
            Self::Time => Column::new("time", "time_s", Unit::Second, 3),
            Self::Object => Column::new("object", "object", Unit::Count, 0),
            Self::Track => Column::new("track", "track", Unit::Count, 0),
            Self::Measure(measurement) => measurement.column(channels, keys),
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
            Self::Measure(measurement) => frame.objects.value(object, *measurement),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackField {
    Track,
    Parent,
    Daughters,
    First,
    Last,
    Nodes,
    Frames,
    Duration,
    Length,
    Displacement,
    Speed,
    Straightness,
}

impl TrackField {
    const ALL: [Self; 12] = [
        Self::Track,
        Self::Parent,
        Self::Daughters,
        Self::First,
        Self::Last,
        Self::Nodes,
        Self::Frames,
        Self::Duration,
        Self::Length,
        Self::Displacement,
        Self::Speed,
        Self::Straightness,
    ];

    fn column(self) -> Column {
        match self {
            Self::Track => Column::new("track", "track", Unit::Count, 0),
            Self::Parent => Column::new("parent", "parent", Unit::Count, 0),
            Self::Daughters => Column::new("daughters", "daughters", Unit::Count, 0),
            Self::First => Column::new("first frame", "first_frame", Unit::Count, 0),
            Self::Last => Column::new("last frame", "last_frame", Unit::Count, 0),
            Self::Nodes => Column::new("nodes", "nodes", Unit::Count, 0),
            Self::Frames => Column::new("frames", "frames", Unit::Count, 0),
            Self::Duration => Column::new("duration", "duration_s", Unit::Second, 3),
            Self::Length => Column::new("length", "length_um", Unit::Micrometer, 3),
            Self::Displacement => {
                Column::new("displacement", "displacement_um", Unit::Micrometer, 3)
            }
            Self::Speed => Column::new("speed", "speed_um_s", Unit::MicrometerPerSecond, 4),
            Self::Straightness => Column::new("straightness", "straightness", Unit::Count, 4),
        }
    }

    fn value(self, track: usize, set: &TrackSet) -> f32 {
        let entry = set.track(track).expect("the track exists");
        let statistics = set.statistics(track).expect("the track has statistics");
        match self {
            Self::Track => (track + 1) as f32,
            Self::Parent => entry.parent().map_or(0.0, |parent| (parent + 1) as f32),
            Self::Daughters => set.children(track).count() as f32,
            Self::First => (entry.start() + 1) as f32,
            Self::Last => (entry.end() + 1) as f32,
            Self::Nodes => entry.nodes().len() as f32,
            Self::Frames => statistics.frames() as f32,
            Self::Duration => statistics.duration() as f32,
            Self::Length => statistics.length() as f32,
            Self::Displacement => statistics.displacement() as f32,
            Self::Speed => statistics.speed() as f32,
            Self::Straightness => statistics.straightness() as f32,
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
    let columns = cells
        .iter()
        .map(|cell| cell.column(channels, keys))
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
    let mut columns = TrackField::ALL
        .iter()
        .map(|field| field.column())
        .collect::<Vec<_>>();
    columns.extend(
        measurements
            .iter()
            .map(|measurement| measurement.aggregate_column(channels, keys, aggregate)),
    );
    let Some(set) = tracks else {
        return Table::new(columns, TrackField::ALL.len(), Vec::new(), Vec::new());
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
                    .push(frame.objects.value(object, *measurement));
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
        for field in &TrackField::ALL {
            values.push(field.value(track, set));
        }
        for index in 0..measurements.len() {
            values.push(
                aggregate.apply(accumulators[track * measurements.len() + index].statistics()),
            );
        }
    }
    Table::new(columns, TrackField::ALL.len(), observations, values)
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
                accumulator.push(frame.objects.value(object, *measurement));
            }
            values.push(aggregate.apply(accumulator.statistics()));
        }
    }
    Table::new(columns, TimeField::ALL.len(), observations, values)
}

fn measurements(objects: Objects<'_>, channels: usize) -> Vec<Measurement> {
    let mut list = vec![
        Measurement::Position(0),
        Measurement::Position(1),
        Measurement::Position(2),
    ];
    match objects {
        Objects::Spots(_) => {
            list.push(Measurement::Diameter);
            list.push(Measurement::Contrast);
        }
        Objects::Surfaces(_) => {
            list.push(Measurement::Volume);
            list.push(Measurement::Area);
            list.push(Measurement::Sphericity);
            list.push(Measurement::Voxels);
        }
    }
    for channel in 0..channels {
        for statistic in Statistic::ALL {
            list.push(Measurement::Intensity { channel, statistic });
        }
    }
    list
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
        (Objects::Spots(_), Objects::Spots(_)) | (Objects::Surfaces(_), Objects::Surfaces(_))
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
