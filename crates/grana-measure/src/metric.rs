use grana_object::{SampleStatistics, TrackSet};

use crate::column::{Column, Unit};
use crate::table::Objects;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Statistic {
    Mean,
    Sum,
    Minimum,
    Maximum,
    Deviation,
}

impl Statistic {
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

    pub fn value(self, statistics: SampleStatistics) -> f32 {
        match self {
            Self::Mean => statistics.mean(),
            Self::Sum => statistics.sum() as f32,
            Self::Minimum => statistics.minimum(),
            Self::Maximum => statistics.maximum(),
            Self::Deviation => statistics.deviation(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Measurement {
    Position(usize),
    Diameter,
    Contrast,
    Volume,
    Area,
    Sphericity,
    MaxDiameter,
    HullVolume,
    Ellipsoid(usize),
    Voxels,
    Length,
    Branches,
    Terminals,
    Junctions,
    Nodes,
    Intensity {
        channel: usize,
        statistic: Statistic,
    },
}

impl Measurement {
    pub fn spots(channels: usize) -> Vec<Self> {
        Self::tail(channels, &[Self::Diameter, Self::Contrast])
    }

    pub fn surfaces(channels: usize) -> Vec<Self> {
        Self::tail(
            channels,
            &[
                Self::Volume,
                Self::Area,
                Self::Sphericity,
                Self::MaxDiameter,
                Self::HullVolume,
                Self::Ellipsoid(0),
                Self::Ellipsoid(1),
                Self::Ellipsoid(2),
                Self::Voxels,
            ],
        )
    }

    pub fn filaments(channels: usize) -> Vec<Self> {
        Self::tail(
            channels,
            &[
                Self::Length,
                Self::Diameter,
                Self::Volume,
                Self::Branches,
                Self::Terminals,
                Self::Junctions,
                Self::Nodes,
            ],
        )
    }

    pub fn label(self, channels: &[String]) -> String {
        match self {
            Self::Position(axis) => ["x", "y", "z"][axis].to_string(),
            Self::Diameter => "diameter".to_string(),
            Self::Contrast => "contrast".to_string(),
            Self::Volume => "volume".to_string(),
            Self::Area => "area".to_string(),
            Self::Sphericity => "sphericity".to_string(),
            Self::MaxDiameter => "max diameter".to_string(),
            Self::HullVolume => "convex hull volume".to_string(),
            Self::Ellipsoid(axis) => format!("ellipsoid axis {}", axis + 1),
            Self::Voxels => "voxels".to_string(),
            Self::Length => "length".to_string(),
            Self::Branches => "branches".to_string(),
            Self::Terminals => "terminals".to_string(),
            Self::Junctions => "junctions".to_string(),
            Self::Nodes => "nodes".to_string(),
            Self::Intensity { channel, statistic } => {
                format!("{} · {}", channels[channel], statistic.name())
            }
        }
    }

    pub fn key(self, keys: &[String]) -> String {
        match self {
            Self::Position(axis) => format!("{}_um", ["x", "y", "z"][axis]),
            Self::Diameter => "diameter_um".to_string(),
            Self::Contrast => "contrast".to_string(),
            Self::Volume => "volume_um3".to_string(),
            Self::Area => "area_um2".to_string(),
            Self::Sphericity => "sphericity".to_string(),
            Self::MaxDiameter => "max_diameter_um".to_string(),
            Self::HullVolume => "hull_volume_um3".to_string(),
            Self::Ellipsoid(axis) => format!("ellipsoid_axis_{}_um", axis + 1),
            Self::Voxels => "voxels".to_string(),
            Self::Length => "length_um".to_string(),
            Self::Branches => "branches".to_string(),
            Self::Terminals => "terminals".to_string(),
            Self::Junctions => "junctions".to_string(),
            Self::Nodes => "nodes".to_string(),
            Self::Intensity { channel, statistic } => {
                format!("{}_intensity_{}", keys[channel], statistic.key())
            }
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Position(_)
            | Self::Diameter
            | Self::Length
            | Self::MaxDiameter
            | Self::Ellipsoid(_) => Unit::Micrometer,
            Self::Volume | Self::HullVolume => Unit::CubicMicrometer,
            Self::Area => Unit::SquareMicrometer,
            Self::Contrast
            | Self::Sphericity
            | Self::Voxels
            | Self::Branches
            | Self::Terminals
            | Self::Junctions
            | Self::Nodes
            | Self::Intensity { .. } => Unit::Count,
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Position(_) | Self::Diameter | Self::Length => 3,
            Self::MaxDiameter | Self::Ellipsoid(_) | Self::HullVolume => 3,
            Self::Contrast | Self::Sphericity => 4,
            Self::Volume | Self::Area => 3,
            Self::Voxels | Self::Branches | Self::Terminals | Self::Junctions | Self::Nodes => 0,
            Self::Intensity { .. } => 2,
        }
    }

    pub fn aggregates(self) -> bool {
        !matches!(self, Self::Position(_))
    }

    pub fn colorable(self) -> bool {
        !matches!(self, Self::Position(_))
    }

    pub fn value(self, objects: Objects<'_>, object: usize) -> f32 {
        let intensities = objects.intensities();
        match objects {
            Objects::Spots(set) => {
                let spot = set.spots()[object];
                match self {
                    Self::Position(axis) => spot.center()[axis],
                    Self::Diameter => 2.0 * spot.radius(),
                    Self::Contrast => spot.quality(),
                    Self::Intensity { channel, statistic } => {
                        statistic.value(intensities.object(object, channel))
                    }
                    Self::Volume
                    | Self::Area
                    | Self::Sphericity
                    | Self::MaxDiameter
                    | Self::HullVolume
                    | Self::Ellipsoid(_)
                    | Self::Voxels
                    | Self::Length
                    | Self::Branches
                    | Self::Terminals
                    | Self::Junctions
                    | Self::Nodes => {
                        panic!("the surface measurement {self:?} is read from spots")
                    }
                }
            }
            Objects::Surfaces(set) => {
                let statistics = set.objects()[object];
                match self {
                    Self::Position(axis) => statistics.centroid()[axis],
                    Self::Volume => statistics.volume() as f32,
                    Self::Area => statistics.area() as f32,
                    Self::Sphericity => statistics.sphericity() as f32,
                    Self::MaxDiameter => statistics
                        .shape()
                        .map_or(f32::NAN, |shape| shape.diameter() as f32),
                    Self::HullVolume => statistics
                        .shape()
                        .map_or(f32::NAN, |shape| shape.hull_volume() as f32),
                    Self::Ellipsoid(axis) => statistics
                        .shape()
                        .map_or(f32::NAN, |shape| shape.axes()[axis] as f32),
                    Self::Voxels => statistics.voxel_count() as f32,
                    Self::Intensity { channel, statistic } => {
                        statistic.value(intensities.object(object, channel))
                    }
                    Self::Diameter
                    | Self::Contrast
                    | Self::Length
                    | Self::Branches
                    | Self::Terminals
                    | Self::Junctions
                    | Self::Nodes => {
                        panic!("the spot measurement {self:?} is read from surfaces")
                    }
                }
            }
            Objects::Filaments(set) => {
                let statistics = set.objects()[object];
                match self {
                    Self::Position(axis) => statistics.centroid()[axis],
                    Self::Length => statistics.length() as f32,
                    Self::Diameter => statistics.diameter() as f32,
                    Self::Volume => statistics.volume() as f32,
                    Self::Branches => statistics.branches() as f32,
                    Self::Terminals => statistics.terminals() as f32,
                    Self::Junctions => statistics.junctions() as f32,
                    Self::Nodes => statistics.nodes() as f32,
                    Self::Intensity { channel, statistic } => {
                        statistic.value(intensities.object(object, channel))
                    }
                    Self::Contrast
                    | Self::Area
                    | Self::Sphericity
                    | Self::MaxDiameter
                    | Self::HullVolume
                    | Self::Ellipsoid(_)
                    | Self::Voxels => {
                        panic!("the filament measurement {self:?} is read from filaments")
                    }
                }
            }
        }
    }

    pub(crate) fn column(self, channels: &[String], keys: &[String]) -> Column {
        Column::new(
            self.label(channels),
            self.key(keys),
            self.unit(),
            self.decimals(),
        )
    }

    pub(crate) fn aggregate_column(
        self,
        channels: &[String],
        keys: &[String],
        aggregate: crate::table::Aggregate,
    ) -> Column {
        Column::new(
            self.label(channels),
            format!("{}_{}", self.key(keys), aggregate.key()),
            self.unit(),
            self.decimals(),
        )
    }

    fn tail(channels: usize, measurements: &[Self]) -> Vec<Self> {
        let mut list = vec![Self::Position(0), Self::Position(1), Self::Position(2)];
        list.extend_from_slice(measurements);
        for channel in 0..channels {
            for statistic in Statistic::ALL {
                list.push(Self::Intensity { channel, statistic });
            }
        }
        list
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackMeasurement {
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

impl TrackMeasurement {
    pub const ALL: [Self; 12] = [
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

    pub fn index(self) -> u8 {
        match self {
            Self::Track => 0,
            Self::Parent => 1,
            Self::Daughters => 2,
            Self::First => 3,
            Self::Last => 4,
            Self::Nodes => 5,
            Self::Frames => 6,
            Self::Duration => 7,
            Self::Length => 8,
            Self::Displacement => 9,
            Self::Speed => 10,
            Self::Straightness => 11,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Parent => "parent",
            Self::Daughters => "daughters",
            Self::First => "first frame",
            Self::Last => "last frame",
            Self::Nodes => "nodes",
            Self::Frames => "frames",
            Self::Duration => "duration",
            Self::Length => "length",
            Self::Displacement => "displacement",
            Self::Speed => "speed",
            Self::Straightness => "straightness",
        }
    }

    pub fn column(self) -> Column {
        let (key, unit, decimals) = self.parts();
        Column::new(self.label(), key, unit, decimals)
    }

    pub fn unit(self) -> Unit {
        self.parts().1
    }

    pub fn decimals(self) -> u8 {
        self.parts().2
    }

    pub fn colorable(self) -> bool {
        !matches!(self, Self::Track | Self::Parent)
    }

    fn parts(self) -> (&'static str, Unit, u8) {
        match self {
            Self::Track => ("track", Unit::Count, 0),
            Self::Parent => ("parent", Unit::Count, 0),
            Self::Daughters => ("daughters", Unit::Count, 0),
            Self::First => ("first_frame", Unit::Count, 0),
            Self::Last => ("last_frame", Unit::Count, 0),
            Self::Nodes => ("nodes", Unit::Count, 0),
            Self::Frames => ("frames", Unit::Count, 0),
            Self::Duration => ("duration_s", Unit::Second, 3),
            Self::Length => ("length_um", Unit::Micrometer, 3),
            Self::Displacement => ("displacement_um", Unit::Micrometer, 3),
            Self::Speed => ("speed_um_s", Unit::MicrometerPerSecond, 4),
            Self::Straightness => ("straightness", Unit::Count, 4),
        }
    }

    pub fn value(self, track: usize, set: &TrackSet) -> f32 {
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
