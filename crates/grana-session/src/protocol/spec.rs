use glam::Vec3;
use grana_analysis::{FilamentParams, SpotParams, SurfaceFilter, SurfaceParams, TrackParams};
use grana_display::{CameraState, Palette, SectionState};
use grana_measure::{Aggregate, Filter, Group, RelationField, Table};
use grana_scene::{ObjectKind, ObjectParams};
use grana_volume::Axis;
use serde::{Deserialize, Serialize};

use super::ProtocolError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    pub name: String,
    pub enabled: bool,
    pub opacity: f32,
    pub window: WindowSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowSpec {
    pub low: f32,
    pub high: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColorSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub palette: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f32; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    pub camera: CameraSpec,
    pub section: SectionSpec,
    pub split: f32,
    #[serde(default)]
    pub time: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSpec {
    pub pivot: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionSpec {
    pub axis: AxisSpec,
    pub span: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AxisSpec {
    X,
    Y,
    Z,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationSpec {
    pub source: String,
    pub target: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSpec {
    pub low: [f32; 3],
    pub high: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSpec {
    pub name: String,
    pub channel: String,
    pub color: [f32; 3],
    #[serde(default = "truth", skip_serializing_if = "is_true")]
    pub visible: bool,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_by: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seeds: Vec<Vec<[f32; 3]>>,
    pub detection: DetectionSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<MeasureSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasureSpec {
    pub group: String,
    pub aggregate: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<FilterSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    pub column: String,
    pub minimum: f32,
    pub maximum: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DetectionSpec {
    Spots {
        diameter: f32,
        contrast: f32,
    },
    Surfaces {
        threshold: f32,
        grain: f32,
        split: f32,
        min_volume: f32,
        opening: f32,
        closing: f32,
        min_sphericity: f32,
        min_mean_intensity: f32,
    },
    Filaments {
        threshold: f32,
        min_length: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSpec {
    pub max_step: f32,
    pub max_gap: u32,
    pub division: bool,
    pub division_range: f32,
    pub thickness: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocSpec {
    pub settings: ColocSettingsSpec,
    pub pairs: Vec<ColocPairSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocSettingsSpec {
    pub permutations: u32,
    pub block: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocPairSpec {
    pub a: String,
    pub b: String,
    pub thresholds: [f32; 2],
}

impl ColocSpec {
    pub fn settings(&self) -> grana_measure::ColocSettings {
        grana_measure::ColocSettings::new(self.settings.permutations, self.settings.block)
    }
}

impl AxisSpec {
    pub fn axis(self) -> Axis {
        match self {
            Self::X => Axis::X,
            Self::Y => Axis::Y,
            Self::Z => Axis::Z,
        }
    }

    pub fn of(axis: Axis) -> Self {
        match axis {
            Axis::X => Self::X,
            Axis::Y => Self::Y,
            Axis::Z => Self::Z,
        }
    }
}

impl ColorSpec {
    pub fn palette(&self) -> Palette {
        palette(&self.palette).expect("a checked color names a palette")
    }

    pub fn field(&self) -> Option<RelationField> {
        let field = self.field.as_deref()?;
        RelationField::ALL
            .into_iter()
            .find(|candidate| candidate.key() == field)
    }

    pub(super) fn checked(&self, what: &str) -> Result<(), ProtocolError> {
        let invalid = |detail: String| {
            Err(ProtocolError::Invalid(format!(
                "the color of {what} {detail}"
            )))
        };
        if !Palette::ALL
            .iter()
            .any(|palette| palette.name().eq_ignore_ascii_case(&self.palette))
        {
            return invalid(format!("names an unknown palette {}", self.palette));
        }
        match (&self.key, &self.target) {
            (Some(key), None) => {
                if key.trim().is_empty() {
                    return invalid("names an empty measurement".to_string());
                }
                if self.field.is_some() {
                    return invalid("names a relation field without a target".to_string());
                }
            }
            (None, Some(target)) => {
                if target.trim().is_empty() {
                    return invalid("names an empty relation target".to_string());
                }
                if self.field().is_none() {
                    return invalid("names an unknown relation field".to_string());
                }
            }
            _ => return invalid("names exactly one measurement or relation".to_string()),
        }
        if let Some([low, high]) = self.range {
            if !(low.is_finite() && high.is_finite()) {
                return invalid("names a range that is not finite".to_string());
            }
            if low > high {
                return invalid("names a range that is not ordered".to_string());
            }
        }
        Ok(())
    }
}

impl ViewSpec {
    pub fn of(camera: CameraState, section: SectionState, split: f32, time: u32) -> Self {
        Self {
            camera: CameraSpec {
                pivot: camera.pivot,
                yaw: camera.yaw,
                pitch: camera.pitch,
                distance: camera.distance,
                fov_y: camera.fov_y,
            },
            section: SectionSpec {
                axis: AxisSpec::of(section.axis),
                span: section.span,
            },
            split,
            time,
        }
    }

    pub fn camera(self) -> CameraState {
        CameraState {
            pivot: self.camera.pivot,
            yaw: self.camera.yaw,
            pitch: self.camera.pitch,
            distance: self.camera.distance,
            fov_y: self.camera.fov_y,
        }
    }

    pub fn section(self) -> SectionState {
        SectionState {
            axis: self.section.axis.axis(),
            span: self.section.span,
        }
    }

    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        let camera = &self.camera;
        if !camera.pivot.iter().all(|value| value.is_finite())
            || !camera.yaw.is_finite()
            || !camera.pitch.is_finite()
            || !camera.distance.is_finite()
            || camera.distance <= 0.0
            || !camera.fov_y.is_finite()
            || camera.fov_y <= 1.0f32.to_radians()
            || camera.fov_y >= 179.0f32.to_radians()
        {
            return Err(ProtocolError::Invalid(
                "a view describes a plausible camera".to_string(),
            ));
        }
        if !self.section.span.is_finite() || self.section.span <= 0.0 {
            return Err(ProtocolError::Invalid(
                "a view describes a positive section span".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.split) {
            return Err(ProtocolError::Invalid(
                "a view splits the viewport at a unit fraction".to_string(),
            ));
        }
        Ok(())
    }
}

impl ChannelSpec {
    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        if self.name.trim().is_empty() {
            return Err(ProtocolError::Invalid(
                "a channel specification names a channel".to_string(),
            ));
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(ProtocolError::Invalid(format!(
                "the channel {} carries a unit opacity",
                self.name
            )));
        }
        if !self.window.low.is_finite()
            || !self.window.high.is_finite()
            || self.window.low > self.window.high
        {
            return Err(ProtocolError::Invalid(format!(
                "the window of {} is a finite ordered range",
                self.name
            )));
        }
        Ok(())
    }
}

impl RegionSpec {
    pub fn of(region: grana_volume::Region, world: Vec3) -> Option<Self> {
        if !region.enabled() {
            return None;
        }
        let half = 0.5 * world;
        let axis =
            |values: Vec3| [0, 1, 2].map(|index| (values[index] + half[index]) / world[index]);
        Some(Self {
            low: axis(region.min()),
            high: axis(region.max()),
        })
    }

    pub fn region(self, world: Vec3) -> grana_volume::Region {
        let half = 0.5 * world;
        let axis = |values: [f32; 3]| {
            Vec3::new(
                values[0] * world.x - half.x,
                values[1] * world.y - half.y,
                values[2] * world.z - half.z,
            )
        };
        grana_volume::Region::new(axis(self.low), axis(self.high))
    }

    pub fn percent(self) -> f32 {
        let span = (0..3).fold(1.0f32, |product, axis| {
            product * (self.high[axis] - self.low[axis]).max(0.0)
        });
        span * 100.0
    }

    pub(super) fn checked(self) -> Result<(), ProtocolError> {
        for axis in 0..3 {
            let low = self.low[axis];
            let high = self.high[axis];
            if !(low.is_finite() && high.is_finite()) {
                return Err(ProtocolError::Invalid("a region box is finite".to_string()));
            }
            if !(0.0..=1.0).contains(&low) || !(0.0..=1.0).contains(&high) {
                return Err(ProtocolError::Invalid(
                    "a region box lies inside the dataset".to_string(),
                ));
            }
            if low >= high {
                return Err(ProtocolError::Invalid(
                    "a region box holds a positive extent on every axis".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl SetSpec {
    pub fn kind(&self) -> ObjectKind {
        self.detection.kind()
    }

    pub fn params(&self) -> ObjectParams {
        self.detection.params()
    }

    pub fn mask(&self) -> Option<&str> {
        self.mask.as_deref()
    }

    pub fn seeds(&self) -> &[Vec<[f32; 3]>] {
        &self.seeds
    }

    pub fn track_params(&self) -> Option<TrackParams> {
        self.track.map(|track| {
            TrackParams::new(
                track.max_step,
                track.max_gap,
                track.division,
                track.division_range,
            )
        })
    }

    pub fn thickness(&self) -> Option<f32> {
        self.track.map(|track| track.thickness)
    }

    pub fn measure(&self) -> Option<&MeasureSpec> {
        self.measure.as_ref()
    }

    pub fn group(&self) -> Group {
        self.measure
            .as_ref()
            .map_or(Group::Object, MeasureSpec::group)
    }

    pub fn aggregate(&self) -> Aggregate {
        self.measure
            .as_ref()
            .map_or(Aggregate::Mean, MeasureSpec::aggregate)
    }

    pub fn filters(&self, table: &Table) -> Result<Vec<Filter>, String> {
        match &self.measure {
            Some(measure) => measure.filters(table),
            None => Ok(Vec::new()),
        }
    }
}

impl MeasureSpec {
    pub fn group(&self) -> Group {
        Group::parse(&self.group).expect("a measure group names a table grouping")
    }

    pub fn aggregate(&self) -> Aggregate {
        Aggregate::parse(&self.aggregate).expect("a measure aggregate names a reduction")
    }

    pub fn filters(&self, table: &Table) -> Result<Vec<Filter>, String> {
        self.filters
            .iter()
            .map(|filter| {
                let column = table
                    .column(&filter.column)
                    .ok_or_else(|| format!("the table has no column named {}", filter.column))?;
                Ok(Filter::new(column, filter.minimum, filter.maximum))
            })
            .collect()
    }

    pub(super) fn checked(&self, set: &str, tracked: bool) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the set {set} {detail}")));
        let group = Group::parse(&self.group).ok_or_else(|| {
            ProtocolError::Invalid(format!("the set {set} names an unknown grouping"))
        })?;
        let aggregate = Aggregate::parse(&self.aggregate).ok_or_else(|| {
            ProtocolError::Invalid(format!("the set {set} names an unknown aggregate"))
        })?;
        if group == Group::Object && aggregate != Aggregate::Mean {
            return invalid("measures objects without an aggregate".to_string());
        }
        if group == Group::Track && !tracked {
            return invalid("groups by tracks without tracking".to_string());
        }
        for filter in &self.filters {
            if filter.column.trim().is_empty() {
                return invalid("filters an unnamed column".to_string());
            }
            if !(filter.minimum.is_finite() && filter.maximum.is_finite()) {
                return invalid("filters a range that is not finite".to_string());
            }
            if filter.minimum > filter.maximum {
                return invalid("filters a range that is not ordered".to_string());
            }
        }
        for (index, column) in self.columns.iter().enumerate() {
            if column.trim().is_empty() {
                return invalid("shows an unnamed column".to_string());
            }
            if self.columns[..index].contains(column) {
                return invalid(format!("shows the column {column} twice"));
            }
        }
        Ok(())
    }
}

impl DetectionSpec {
    pub fn kind(&self) -> ObjectKind {
        match self {
            Self::Spots { .. } => ObjectKind::Spots,
            Self::Surfaces { .. } => ObjectKind::Surfaces,
            Self::Filaments { .. } => ObjectKind::Filaments,
        }
    }

    pub fn params(&self) -> ObjectParams {
        match *self {
            Self::Spots { diameter, contrast } => {
                ObjectParams::Spots(SpotParams::new(diameter, contrast))
            }
            Self::Surfaces {
                threshold,
                grain,
                split,
                min_volume,
                opening,
                closing,
                min_sphericity,
                min_mean_intensity,
            } => ObjectParams::Surfaces(SurfaceParams::new(
                threshold,
                grain,
                split,
                min_volume,
                SurfaceFilter::new(opening, closing, min_sphericity, min_mean_intensity),
            )),
            Self::Filaments {
                threshold,
                min_length,
            } => ObjectParams::Filaments(FilamentParams::new(threshold, min_length)),
        }
    }

    pub(super) fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the set {set} {detail}")));
        match *self {
            Self::Spots { diameter, contrast } => {
                if !(diameter.is_finite() && diameter > 0.0) {
                    return invalid("needs a positive spot diameter".to_string());
                }
                if !(0.0..=1.0).contains(&contrast) {
                    return invalid("needs a unit contrast threshold".to_string());
                }
            }
            Self::Surfaces {
                threshold,
                grain,
                split,
                min_volume,
                opening,
                closing,
                min_sphericity,
                min_mean_intensity,
            } => {
                if !(0.0..=1.0).contains(&threshold) {
                    return invalid("needs a unit surface threshold".to_string());
                }
                if !(grain.is_finite() && grain >= 0.0) {
                    return invalid("needs a non-negative surface grain".to_string());
                }
                if !(0.0..=1.0).contains(&split) {
                    return invalid("needs a unit surface split".to_string());
                }
                if !(min_volume.is_finite() && min_volume >= 0.0) {
                    return invalid("needs a non-negative minimum volume".to_string());
                }
                if !(opening.is_finite() && opening >= 0.0) {
                    return invalid("needs a non-negative surface opening radius".to_string());
                }
                if !(closing.is_finite() && closing >= 0.0) {
                    return invalid("needs a non-negative surface closing radius".to_string());
                }
                if !(0.0..=1.0).contains(&min_sphericity) {
                    return invalid("needs a unit sphericity filter".to_string());
                }
                if !(0.0..=1.0).contains(&min_mean_intensity) {
                    return invalid("needs a unit intensity filter".to_string());
                }
            }
            Self::Filaments {
                threshold,
                min_length,
            } => {
                if !(0.0..=1.0).contains(&threshold) {
                    return invalid("needs a unit filament threshold".to_string());
                }
                if !(min_length.is_finite() && min_length > 0.0) {
                    return invalid("needs a positive minimum length".to_string());
                }
            }
        }
        Ok(())
    }
}

impl TrackSpec {
    pub(super) fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        let invalid = |detail: String| {
            Err(ProtocolError::Invalid(format!(
                "the track of {set} {detail}"
            )))
        };
        if !(self.max_step.is_finite() && self.max_step > 0.0) {
            return invalid("needs a positive step".to_string());
        }
        if !(self.division_range.is_finite() && self.division_range > 0.0) {
            return invalid("needs a positive division range".to_string());
        }
        if self.division && self.division_range < self.max_step {
            return invalid("needs a division range that covers the step".to_string());
        }
        if !(self.thickness.is_finite() && self.thickness > 0.0) {
            return invalid("needs a positive tube thickness".to_string());
        }
        Ok(())
    }
}

fn truth() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

pub(super) fn palette(name: &str) -> Option<Palette> {
    Palette::ALL
        .into_iter()
        .find(|palette| palette.name().eq_ignore_ascii_case(name))
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn one() -> f32 {
    1.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}
