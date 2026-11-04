use grana_measure::{Filter, Group, Lasso, Table};
use grana_object::TrackSet;

use crate::object::{ObjectKind, ObjectParams};
use crate::object_set::ObjectSetId;

#[derive(Clone, Debug, PartialEq)]
pub struct FilterRange {
    key: String,
    minimum: f32,
    maximum: f32,
}

impl FilterRange {
    pub fn new(key: &str, minimum: f32, maximum: f32) -> Self {
        assert!(
            !key.trim().is_empty(),
            "a filter range names a measurement column"
        );
        assert!(
            minimum.is_finite() && maximum.is_finite(),
            "a filter range is finite"
        );
        assert!(minimum <= maximum, "a filter range is ordered");
        Self {
            key: key.to_string(),
            minimum,
            maximum,
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn minimum(&self) -> f32 {
        self.minimum
    }

    pub fn maximum(&self) -> f32 {
        self.maximum
    }

    fn filter(&self, table: &Table) -> Result<Filter, String> {
        let column = table.column(&self.key).ok_or_else(|| {
            format!(
                "the derived object set filters the missing column {}",
                self.key
            )
        })?;
        Ok(Filter::new(column, self.minimum, self.maximum))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LassoPath {
    x: String,
    y: String,
    points: Vec<[f32; 2]>,
}

impl LassoPath {
    pub fn new(x: &str, y: &str, points: Vec<[f32; 2]>) -> Option<Self> {
        let finite = points.iter().flatten().all(|value| value.is_finite());
        if x == y || x.trim().is_empty() || y.trim().is_empty() || points.len() < 3 || !finite {
            return None;
        }
        Some(Self {
            x: x.to_string(),
            y: y.to_string(),
            points,
        })
    }

    pub fn x(&self) -> &str {
        &self.x
    }

    pub fn y(&self) -> &str {
        &self.y
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.points
    }

    fn path(&self, table: &Table) -> Result<Lasso, String> {
        let axis = |key: &str| {
            table
                .column(key)
                .ok_or_else(|| format!("the derived object set gates the missing column {key}"))
        };
        Lasso::new(axis(&self.x)?, axis(&self.y)?, self.points.clone())
            .ok_or_else(|| "the derived object set gates a degenerate polygon".to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Criteria {
    group: Group,
    filters: Vec<FilterRange>,
    lasso: Option<LassoPath>,
}

impl Criteria {
    pub fn new(group: Group, filters: Vec<FilterRange>, lasso: Option<LassoPath>) -> Self {
        assert!(
            group != Group::Timepoint,
            "a derived object set filters objects or tracks"
        );
        assert!(
            !filters.is_empty() || lasso.is_some(),
            "a derived object set filters at least one measurement"
        );
        Self {
            group,
            filters,
            lasso,
        }
    }

    pub fn group(&self) -> Group {
        self.group
    }

    pub fn filters(&self) -> &[FilterRange] {
        &self.filters
    }

    pub fn lasso(&self) -> Option<&LassoPath> {
        self.lasso.as_ref()
    }

    pub fn keep(
        &self,
        table: &Table,
        tracks: Option<&TrackSet>,
        counts: &[usize],
    ) -> Result<Vec<Vec<bool>>, String> {
        let filters = self
            .filters
            .iter()
            .map(|filter| filter.filter(table))
            .collect::<Result<Vec<_>, String>>()?;
        let lasso = match &self.lasso {
            Some(lasso) => Some(lasso.path(table)?),
            None => None,
        };
        let mut keep = counts
            .iter()
            .map(|count| vec![false; *count])
            .collect::<Vec<_>>();
        for row in table.matching(&filters, lasso.as_ref(), None) {
            let observation = table.observation(row);
            match self.group {
                Group::Object => {
                    if let (Some(time), Some(object)) = (observation.time(), observation.object())
                        && let Some(mask) = keep.get_mut(time as usize)
                        && let Some(slot) = mask.get_mut(object)
                    {
                        *slot = true;
                    }
                }
                Group::Track => {
                    let Some(track) = observation
                        .track()
                        .and_then(|index| tracks.and_then(|tracks| tracks.track(index)))
                    else {
                        continue;
                    };
                    for node in track.nodes() {
                        if let Some(mask) = keep.get_mut(node.time() as usize)
                            && let Some(slot) = mask.get_mut(node.object())
                        {
                            *slot = true;
                        }
                    }
                }
                Group::Timepoint => unreachable!("a derived object set filters no timepoints"),
            }
        }
        Ok(keep)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Provenance {
    Detection,
    Filtered {
        source: ObjectSetId,
        criteria: Criteria,
    },
    Copied {
        source: ObjectSetId,
    },
    Merged {
        parts: Vec<Recipe>,
    },
}

impl Provenance {
    pub fn detection() -> Self {
        Self::Detection
    }

    pub fn is_detection(&self) -> bool {
        matches!(self, Self::Detection)
    }

    pub fn is_merged(&self) -> bool {
        matches!(self, Self::Merged { .. })
    }

    pub fn source(&self) -> Option<ObjectSetId> {
        match self {
            Self::Filtered { source, .. } | Self::Copied { source } => Some(*source),
            Self::Detection | Self::Merged { .. } => None,
        }
    }

    pub fn criteria(&self) -> Option<&Criteria> {
        match self {
            Self::Filtered { criteria, .. } => Some(criteria),
            Self::Detection | Self::Copied { .. } | Self::Merged { .. } => None,
        }
    }

    pub fn parts(&self) -> &[Recipe] {
        match self {
            Self::Merged { parts } => parts,
            Self::Detection | Self::Filtered { .. } | Self::Copied { .. } => &[],
        }
    }

    pub fn references(&self, set: ObjectSetId) -> bool {
        match self {
            Self::Detection => false,
            Self::Filtered { source, .. } | Self::Copied { source } => *source == set,
            Self::Merged { parts } => parts.iter().any(|part| part.provenance().references(set)),
        }
    }

    pub fn depends_on(&self, set: ObjectSetId) -> bool {
        match self {
            Self::Detection | Self::Copied { .. } => false,
            Self::Filtered { source, .. } => *source == set,
            Self::Merged { parts } => parts.iter().any(|part| part.provenance().depends_on(set)),
        }
    }

    pub(crate) fn forget_copied(&mut self, removed: &[ObjectSetId]) {
        match self {
            Self::Copied { source } if removed.contains(source) => *self = Self::Detection,
            Self::Merged { parts } => {
                for part in parts {
                    part.provenance.forget_copied(removed);
                }
            }
            Self::Detection | Self::Filtered { .. } | Self::Copied { .. } => {}
        }
    }

    pub fn sources(&self) -> Vec<ObjectSetId> {
        let mut sources = Vec::new();
        self.collect_sources(&mut sources);
        sources
    }

    fn collect_sources(&self, sources: &mut Vec<ObjectSetId>) {
        match self {
            Self::Detection => {}
            Self::Filtered { source, .. } | Self::Copied { source } => sources.push(*source),
            Self::Merged { parts } => {
                for part in parts {
                    part.provenance().collect_sources(sources);
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    name: String,
    channel: usize,
    params: ObjectParams,
    seeds: Vec<Vec<[f32; 3]>>,
    provenance: Provenance,
}

impl Recipe {
    pub fn new(
        name: String,
        channel: usize,
        params: ObjectParams,
        seeds: Vec<Vec<[f32; 3]>>,
        provenance: Provenance,
    ) -> Self {
        assert!(
            !name.trim().is_empty(),
            "a merged part names its object set"
        );
        assert!(
            params.kind() == ObjectKind::Surfaces || seeds.iter().all(Vec::is_empty),
            "only a surface part carries seed points"
        );
        assert!(
            seeds
                .iter()
                .flatten()
                .all(|seed| seed.iter().all(|value| value.is_finite())),
            "a merged part seeds finite points"
        );
        Self {
            name,
            channel,
            params,
            seeds,
            provenance,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn kind(&self) -> ObjectKind {
        self.params.kind()
    }

    pub fn params(&self) -> ObjectParams {
        self.params
    }

    pub fn seeds(&self) -> &[Vec<[f32; 3]>] {
        &self.seeds
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn forget_copied(&mut self, removed: &[ObjectSetId]) {
        self.provenance.forget_copied(removed);
    }
}
