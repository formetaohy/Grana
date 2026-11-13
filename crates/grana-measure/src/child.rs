use grana_object::SampleAccumulator;

use crate::column::{Column, Unit};
use crate::metric::{Measurement, Statistic};
use crate::table::Objects;

pub const UNASSIGNED: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChildMetric {
    Diameter,
    Contrast,
    Volume,
    Area,
    Sphericity,
    Voxels,
    Length,
    Branches,
    Terminals,
    Junctions,
    Nodes,
    Intensity(usize),
}

impl ChildMetric {
    pub fn of(measurement: Measurement) -> Option<Self> {
        match measurement {
            Measurement::Diameter => Some(Self::Diameter),
            Measurement::Contrast => Some(Self::Contrast),
            Measurement::Volume => Some(Self::Volume),
            Measurement::Area => Some(Self::Area),
            Measurement::Sphericity => Some(Self::Sphericity),
            Measurement::Voxels => Some(Self::Voxels),
            Measurement::Length => Some(Self::Length),
            Measurement::Branches => Some(Self::Branches),
            Measurement::Terminals => Some(Self::Terminals),
            Measurement::Junctions => Some(Self::Junctions),
            Measurement::Nodes => Some(Self::Nodes),
            Measurement::Intensity { channel, statistic } => {
                (statistic == Statistic::Mean).then_some(Self::Intensity(channel))
            }
            Measurement::Position(_)
            | Measurement::Precision(_)
            | Measurement::MaxDiameter
            | Measurement::HullVolume
            | Measurement::Ellipsoid(_)
            | Measurement::NucleusVolume
            | Measurement::NucleusArea
            | Measurement::NucleusSphericity
            | Measurement::CytoplasmVolume
            | Measurement::NuclearRatio => None,
        }
    }

    pub fn parse(key: &str, keys: &[String]) -> Option<Self> {
        let fixed = match key {
            "diameter_um" => Some(Self::Diameter),
            "contrast" => Some(Self::Contrast),
            "volume_um3" => Some(Self::Volume),
            "area_um2" => Some(Self::Area),
            "sphericity" => Some(Self::Sphericity),
            "voxels" => Some(Self::Voxels),
            "length_um" => Some(Self::Length),
            "branches" => Some(Self::Branches),
            "terminals" => Some(Self::Terminals),
            "junctions" => Some(Self::Junctions),
            "nodes" => Some(Self::Nodes),
            _ => None,
        };
        fixed.or_else(|| {
            keys.iter()
                .position(|channel| format!("{channel}_intensity") == key)
                .map(Self::Intensity)
        })
    }

    pub fn label(self, channels: &[String]) -> String {
        match self {
            Self::Diameter => "diameter".to_string(),
            Self::Contrast => "contrast".to_string(),
            Self::Volume => "volume".to_string(),
            Self::Area => "area".to_string(),
            Self::Sphericity => "sphericity".to_string(),
            Self::Voxels => "voxels".to_string(),
            Self::Length => "length".to_string(),
            Self::Branches => "branches".to_string(),
            Self::Terminals => "terminals".to_string(),
            Self::Junctions => "junctions".to_string(),
            Self::Nodes => "nodes".to_string(),
            Self::Intensity(channel) => format!("{} intensity", channels[channel]),
        }
    }

    pub fn key(self, keys: &[String]) -> String {
        match self {
            Self::Diameter => "diameter_um".to_string(),
            Self::Contrast => "contrast".to_string(),
            Self::Volume => "volume_um3".to_string(),
            Self::Area => "area_um2".to_string(),
            Self::Sphericity => "sphericity".to_string(),
            Self::Voxels => "voxels".to_string(),
            Self::Length => "length_um".to_string(),
            Self::Branches => "branches".to_string(),
            Self::Terminals => "terminals".to_string(),
            Self::Junctions => "junctions".to_string(),
            Self::Nodes => "nodes".to_string(),
            Self::Intensity(channel) => format!("{}_intensity", keys[channel]),
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Diameter | Self::Length => Unit::Micrometer,
            Self::Volume => Unit::CubicMicrometer,
            Self::Area => Unit::SquareMicrometer,
            Self::Contrast
            | Self::Sphericity
            | Self::Voxels
            | Self::Branches
            | Self::Terminals
            | Self::Junctions
            | Self::Nodes
            | Self::Intensity(_) => Unit::Count,
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Diameter | Self::Length | Self::Volume | Self::Area => 3,
            Self::Contrast | Self::Sphericity => 4,
            Self::Voxels | Self::Branches | Self::Terminals | Self::Junctions | Self::Nodes => 0,
            Self::Intensity(_) => 2,
        }
    }

    pub fn value(self, objects: Objects<'_>, object: usize) -> f32 {
        let intensities = objects.intensities();
        match objects {
            Objects::Spots(set) => {
                let spot = set.spots()[object];
                match self {
                    Self::Diameter => 2.0 * spot.radius(),
                    Self::Contrast => spot.quality(),
                    Self::Intensity(channel) => intensities.object(object, channel).mean(),
                    Self::Volume
                    | Self::Area
                    | Self::Sphericity
                    | Self::Voxels
                    | Self::Length
                    | Self::Branches
                    | Self::Terminals
                    | Self::Junctions
                    | Self::Nodes => {
                        panic!("the surface child metric {self:?} is read from spots")
                    }
                }
            }
            Objects::Surfaces(set) => {
                let statistics = set.objects()[object];
                match self {
                    Self::Volume => statistics.volume() as f32,
                    Self::Area => statistics.area() as f32,
                    Self::Sphericity => statistics.sphericity() as f32,
                    Self::Voxels => statistics.voxel_count() as f32,
                    Self::Intensity(channel) => intensities.object(object, channel).mean(),
                    Self::Diameter
                    | Self::Contrast
                    | Self::Length
                    | Self::Branches
                    | Self::Terminals
                    | Self::Junctions
                    | Self::Nodes => {
                        panic!("the spot child metric {self:?} is read from surfaces")
                    }
                }
            }
            Objects::Cells(set) => {
                let statistics = set.body(object);
                match self {
                    Self::Volume => statistics.volume() as f32,
                    Self::Area => statistics.area() as f32,
                    Self::Sphericity => statistics.sphericity() as f32,
                    Self::Voxels => statistics.voxel_count() as f32,
                    Self::Intensity(channel) => intensities.object(object, channel).mean(),
                    Self::Diameter
                    | Self::Contrast
                    | Self::Length
                    | Self::Branches
                    | Self::Terminals
                    | Self::Junctions
                    | Self::Nodes => {
                        panic!("the spot child metric {self:?} is read from cells")
                    }
                }
            }
            Objects::Filaments(set) => {
                let statistics = set.objects()[object];
                match self {
                    Self::Length => statistics.length() as f32,
                    Self::Diameter => statistics.diameter() as f32,
                    Self::Volume => statistics.volume() as f32,
                    Self::Branches => statistics.branches() as f32,
                    Self::Terminals => statistics.terminals() as f32,
                    Self::Junctions => statistics.junctions() as f32,
                    Self::Nodes => statistics.nodes() as f32,
                    Self::Intensity(channel) => intensities.object(object, channel).mean(),
                    Self::Contrast | Self::Area | Self::Sphericity | Self::Voxels => {
                        panic!("the filament child metric {self:?} is read from filaments")
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChildField {
    Count,
    Stat {
        metric: ChildMetric,
        statistic: Statistic,
    },
}

impl ChildField {
    pub fn fields(metrics: &[ChildMetric]) -> Vec<Self> {
        let mut fields = vec![Self::Count];
        for metric in metrics {
            for statistic in Statistic::ALL {
                fields.push(Self::Stat {
                    metric: *metric,
                    statistic,
                });
            }
        }
        fields
    }

    pub fn parse(metric: &str, statistic: Option<&str>, keys: &[String]) -> Option<Self> {
        if metric == "count" {
            return statistic.is_none().then_some(Self::Count);
        }
        let statistic = statistic?;
        let statistic = Statistic::ALL
            .into_iter()
            .find(|candidate| candidate.key() == statistic)?;
        Some(Self::Stat {
            metric: ChildMetric::parse(metric, keys)?,
            statistic,
        })
    }

    pub fn metric(self) -> Option<ChildMetric> {
        match self {
            Self::Count => None,
            Self::Stat { metric, .. } => Some(metric),
        }
    }

    pub fn statistic(self) -> Option<Statistic> {
        match self {
            Self::Count => None,
            Self::Stat { statistic, .. } => Some(statistic),
        }
    }

    pub fn label(self, channels: &[String]) -> String {
        match self {
            Self::Count => "count".to_string(),
            Self::Stat { metric, statistic } => {
                format!("{} {}", metric.label(channels), statistic.name())
            }
        }
    }

    pub fn key(self, keys: &[String]) -> String {
        match self {
            Self::Count => "count".to_string(),
            Self::Stat { metric, statistic } => {
                format!("{}_{}", metric.key(keys), statistic.key())
            }
        }
    }

    pub fn unit(self) -> Unit {
        match self {
            Self::Count => Unit::Count,
            Self::Stat { metric, .. } => metric.unit(),
        }
    }

    pub fn decimals(self) -> u8 {
        match self {
            Self::Count => 0,
            Self::Stat { metric, .. } => metric.decimals(),
        }
    }

    pub fn column(
        self,
        child_key: &str,
        child_name: &str,
        channels: &[String],
        keys: &[String],
    ) -> Column {
        Column::new(
            format!("{child_name} {}", self.label(channels)),
            format!("{child_key}_{}", self.key(keys)),
            self.unit(),
            self.decimals(),
        )
    }
}

#[derive(Clone, Debug)]
pub struct Child<'a> {
    key: &'a str,
    name: &'a str,
    fields: Vec<ChildField>,
    objects: Option<Objects<'a>>,
    members: Option<Vec<u32>>,
}

impl<'a> Child<'a> {
    pub fn new(
        key: &'a str,
        name: &'a str,
        fields: Vec<ChildField>,
        objects: Option<Objects<'a>>,
        members: Option<Vec<u32>>,
    ) -> Self {
        assert!(!key.trim().is_empty(), "a child names a stable key");
        if let Some(members) = &members {
            assert!(
                objects.is_some_and(|objects| objects.count() == members.len()),
                "the child assignments cover every child object"
            );
        }
        Self {
            key,
            name,
            fields,
            objects,
            members,
        }
    }

    pub fn key(&self) -> &str {
        self.key
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn fields(&self) -> &[ChildField] {
        &self.fields
    }

    pub fn values(&self, parents: usize, field: ChildField) -> Vec<f32> {
        let Some((objects, members)) = self.assignments() else {
            return vec![f32::NAN; parents];
        };
        match field {
            ChildField::Count => {
                let mut values = vec![0.0; parents];
                for member in members {
                    if *member != UNASSIGNED {
                        values[assert_parent(*member, parents)] += 1.0;
                    }
                }
                values
            }
            ChildField::Stat { metric, statistic } => {
                let mut accumulators = vec![SampleAccumulator::new(); parents];
                let mut counts = vec![0u32; parents];
                for (child, member) in members.iter().enumerate() {
                    if *member == UNASSIGNED {
                        continue;
                    }
                    let parent = assert_parent(*member, parents);
                    counts[parent] += 1;
                    accumulators[parent].push(metric.value(objects, child));
                }
                let mut values = vec![f32::NAN; parents];
                for parent in 0..parents {
                    if counts[parent] > 0 {
                        values[parent] = statistic.value(accumulators[parent].statistics());
                    }
                }
                values
            }
        }
    }

    pub fn columns(&self, parents: usize) -> Vec<Vec<f32>> {
        let mut values = vec![vec![f32::NAN; parents]; self.fields.len()];
        let Some((objects, members)) = self.assignments() else {
            return values;
        };
        let count = self
            .fields
            .iter()
            .position(|field| *field == ChildField::Count);
        let slots = self
            .fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| match field {
                ChildField::Count => None,
                ChildField::Stat { metric, statistic } => Some((index, *metric, *statistic)),
            })
            .collect::<Vec<_>>();
        let mut counts = vec![0u32; parents];
        let mut accumulators = vec![SampleAccumulator::new(); parents * slots.len()];
        for (child, member) in members.iter().enumerate() {
            if *member == UNASSIGNED {
                continue;
            }
            let parent = assert_parent(*member, parents);
            counts[parent] += 1;
            for (slot, (_, metric, _)) in slots.iter().enumerate() {
                accumulators[parent * slots.len() + slot].push(metric.value(objects, child));
            }
        }
        if let Some(index) = count {
            for parent in 0..parents {
                values[index][parent] = counts[parent] as f32;
            }
        }
        for parent in 0..parents {
            if counts[parent] == 0 {
                continue;
            }
            for (slot, (index, _, statistic)) in slots.iter().enumerate() {
                values[*index][parent] =
                    statistic.value(accumulators[parent * slots.len() + slot].statistics());
            }
        }
        values
    }

    fn assignments(&self) -> Option<(Objects<'a>, &[u32])> {
        Some((self.objects?, self.members.as_deref()?))
    }
}

fn assert_parent(member: u32, parents: usize) -> usize {
    let parent = member as usize;
    assert!(
        parent < parents,
        "a child assignment names a parent object of its frame"
    );
    parent
}
