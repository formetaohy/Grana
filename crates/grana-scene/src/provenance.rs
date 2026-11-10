use grana_measure::{Filter, Group, Lasso, Table};
use grana_object::{MaskEdit, SurfaceEdit, TrackSet};

use crate::curation::Curation;
use crate::object::{ObjectKind, ObjectParams};
use crate::object_set::ObjectSetId;
use crate::seed::Seeds;
use crate::segment::SegmentParams;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IntensitySource {
    #[default]
    Statistics,
    Sampled,
}

impl IntensitySource {
    pub fn index(self) -> u8 {
        match self {
            Self::Statistics => 0,
            Self::Sampled => 1,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Self::Statistics),
            1 => Some(Self::Sampled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Provenance {
    Detection,
    Segmented {
        params: SegmentParams,
    },
    Imported {
        label: String,
    },
    Interop {
        file: String,
    },
    Attached {
        file: String,
        intensities: IntensitySource,
    },
    Filtered {
        source: ObjectSetId,
        criteria: Criteria,
    },
    Copied {
        source: ObjectSetId,
    },
    Edited {
        source: ObjectSetId,
        edits: Vec<SurfaceEdit>,
    },
    Masked {
        source: ObjectSetId,
        strokes: Vec<MaskEdit>,
    },
    Merged {
        parts: Vec<Recipe>,
    },
}

impl Provenance {
    pub fn algorithm(&self) -> grana_object::Algorithm {
        match self {
            Self::Detection => grana_object::Algorithm::Detection,
            Self::Segmented { .. } => grana_object::Algorithm::Segmentation,
            Self::Imported { .. } => grana_object::Algorithm::Import,
            Self::Interop { .. } => grana_object::Algorithm::Interop,
            Self::Attached { .. } => grana_object::Algorithm::Attachment,
            Self::Filtered { .. } => grana_object::Algorithm::Filter,
            Self::Copied { .. } => grana_object::Algorithm::Copy,
            Self::Edited { .. } => grana_object::Algorithm::Edit,
            Self::Masked { .. } => grana_object::Algorithm::Mask,
            Self::Merged { .. } => grana_object::Algorithm::Merge,
        }
    }

    pub fn detection() -> Self {
        Self::Detection
    }

    pub fn segmented(params: SegmentParams) -> Self {
        Self::Segmented { params }
    }

    pub fn imported(label: &str) -> Self {
        Self::Imported {
            label: label.to_string(),
        }
    }

    pub fn interop(file: &str) -> Self {
        Self::Interop {
            file: file.to_string(),
        }
    }

    pub fn attached(file: &str, intensities: IntensitySource) -> Self {
        Self::Attached {
            file: file.to_string(),
            intensities,
        }
    }

    pub fn is_detection(&self) -> bool {
        matches!(self, Self::Detection | Self::Segmented { .. })
    }

    pub fn is_segmented(&self) -> bool {
        matches!(self, Self::Segmented { .. })
    }

    pub fn segment(&self) -> Option<&SegmentParams> {
        match self {
            Self::Segmented { params } => Some(params),
            _ => None,
        }
    }

    pub fn is_imported(&self) -> bool {
        matches!(self, Self::Imported { .. })
    }

    pub fn is_interop(&self) -> bool {
        matches!(self, Self::Interop { .. })
    }

    pub fn is_attached(&self) -> bool {
        matches!(self, Self::Attached { .. })
    }

    pub fn is_external(&self) -> bool {
        matches!(
            self,
            Self::Imported { .. } | Self::Interop { .. } | Self::Attached { .. }
        )
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Imported { label } => Some(label),
            _ => None,
        }
    }

    pub fn file(&self) -> Option<&str> {
        match self {
            Self::Interop { file } | Self::Attached { file, .. } => Some(file),
            _ => None,
        }
    }

    pub fn intensities(&self) -> Option<IntensitySource> {
        match self {
            Self::Attached { intensities, .. } => Some(*intensities),
            _ => None,
        }
    }

    pub fn is_merged(&self) -> bool {
        matches!(self, Self::Merged { .. })
    }

    pub fn edits(&self) -> &[SurfaceEdit] {
        match self {
            Self::Edited { edits, .. } => edits,
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Filtered { .. }
            | Self::Copied { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => &[],
        }
    }

    pub fn strokes(&self) -> &[MaskEdit] {
        match self {
            Self::Masked { strokes, .. } => strokes,
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Filtered { .. }
            | Self::Copied { .. }
            | Self::Edited { .. }
            | Self::Merged { .. } => &[],
        }
    }

    pub fn editing(&self) -> Option<(ObjectSetId, &[SurfaceEdit])> {
        match self {
            Self::Edited { source, edits } => Some((*source, edits)),
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Filtered { .. }
            | Self::Copied { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn masking(&self) -> Option<(ObjectSetId, &[MaskEdit])> {
        match self {
            Self::Masked { source, strokes } => Some((*source, strokes)),
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Filtered { .. }
            | Self::Copied { .. }
            | Self::Edited { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn source(&self) -> Option<ObjectSetId> {
        match self {
            Self::Filtered { source, .. }
            | Self::Copied { source }
            | Self::Edited { source, .. }
            | Self::Masked { source, .. } => Some(*source),
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn criteria(&self) -> Option<&Criteria> {
        match self {
            Self::Filtered { criteria, .. } => Some(criteria),
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Copied { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn parts(&self) -> &[Recipe] {
        match self {
            Self::Merged { parts } => parts,
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Filtered { .. }
            | Self::Copied { .. }
            | Self::Edited { .. }
            | Self::Masked { .. } => &[],
        }
    }

    pub fn references(&self, set: ObjectSetId) -> bool {
        match self {
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. } => false,
            Self::Filtered { source, .. }
            | Self::Copied { source }
            | Self::Edited { source, .. }
            | Self::Masked { source, .. } => *source == set,
            Self::Merged { parts } => parts.iter().any(|part| part.provenance().references(set)),
        }
    }

    pub fn depends_on(&self, set: ObjectSetId) -> bool {
        match self {
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Copied { .. } => false,
            Self::Filtered { source, .. }
            | Self::Edited { source, .. }
            | Self::Masked { source, .. } => *source == set,
            Self::Merged { parts } => parts.iter().any(|part| part.provenance().depends_on(set)),
        }
    }

    pub(crate) fn forget_copied(&mut self, removed: &[ObjectSetId]) {
        match self {
            Self::Copied { source } if removed.contains(source) => *self = Self::Detection,
            Self::Edited { source, .. } if removed.contains(source) => *self = Self::Detection,
            Self::Masked { source, .. } if removed.contains(source) => *self = Self::Detection,
            Self::Merged { parts } => {
                for part in parts {
                    part.provenance.forget_copied(removed);
                }
            }
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. }
            | Self::Filtered { .. }
            | Self::Copied { .. }
            | Self::Edited { .. }
            | Self::Masked { .. } => {}
        }
    }

    pub fn sources(&self) -> Vec<ObjectSetId> {
        let mut sources = Vec::new();
        self.collect_sources(&mut sources);
        sources
    }

    fn collect_sources(&self, sources: &mut Vec<ObjectSetId>) {
        match self {
            Self::Detection
            | Self::Segmented { .. }
            | Self::Imported { .. }
            | Self::Interop { .. }
            | Self::Attached { .. } => {}
            Self::Filtered { source, .. }
            | Self::Copied { source }
            | Self::Edited { source, .. }
            | Self::Masked { source, .. } => sources.push(*source),
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
    seeds: Seeds,
    curation: Curation,
    provenance: Provenance,
}

impl Recipe {
    pub fn new(
        name: String,
        channel: usize,
        params: ObjectParams,
        seeds: Seeds,
        curation: Curation,
        provenance: Provenance,
    ) -> Self {
        assert!(
            !name.trim().is_empty(),
            "a merged part names its object set"
        );
        match &seeds {
            Seeds::Points(frames) => {
                assert!(
                    params.kind() == ObjectKind::Surfaces || frames.iter().all(Vec::is_empty),
                    "only a surface part carries seed points"
                );
                assert!(
                    frames
                        .iter()
                        .flatten()
                        .all(|seed| seed.iter().all(|value| value.is_finite())),
                    "a merged part seeds finite points"
                );
            }
            Seeds::Source(_) => assert!(
                params.kind() == ObjectKind::Surfaces,
                "only a surface part seeds from another object set"
            ),
        }
        Self {
            name,
            channel,
            params,
            seeds,
            curation,
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

    pub fn seeds(&self) -> &Seeds {
        &self.seeds
    }

    pub fn curation(&self) -> &Curation {
        &self.curation
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn forget_copied(&mut self, removed: &[ObjectSetId]) {
        if let Seeds::Source(source) = &self.seeds
            && removed.contains(source)
        {
            self.seeds = Seeds::Points(Vec::new());
        }
        self.provenance.forget_copied(removed);
    }
}
