use glam::Vec3;
use grana_analysis::{FilamentParams, SpotParams, SurfaceFilter, SurfaceParams, TrackParams};
use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, Shading, Stop, TransferFunction};
use grana_measure::{Aggregate, Filter, Group, MeasureView, RelationField, Scope, Table};
use grana_project::{Export, ViewState};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<TransferSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowSpec {
    pub low: f32,
    pub high: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransferSpec {
    pub stops: Vec<StopSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StopSpec {
    pub position: f32,
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderSpec {
    pub exposure: f32,
    pub step_scale: f32,
    pub shading: ShadingSpec,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadingSpec {
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
    pub shadows: f32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<String>,
    pub palette: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    pub camera: CameraSpec,
    pub section: SectionSpec,
    pub split: f32,
    #[serde(default)]
    pub time: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline: Option<TimelineSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<ExportSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSpec {
    pub width: u32,
    pub transparent: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineSpec {
    pub fps: u32,
    pub duration: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeySpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySpec {
    pub at: f32,
    pub time: u32,
    pub camera: CameraSpec,
    pub section: SectionSpec,
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
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: i32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default = "truth", skip_serializing_if = "is_true")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axes: Option<[String; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lasso: Option<LassoSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<FilterSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<SortSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LassoSpec {
    pub x: String,
    pub y: String,
    pub points: Vec<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortSpec {
    pub column: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub descending: bool,
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
    #[serde(default = "truth", skip_serializing_if = "is_true")]
    pub visible: bool,
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

    pub fn aggregate(&self) -> Option<Aggregate> {
        let aggregate = self.aggregate.as_deref()?;
        Aggregate::parse(aggregate)
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
                if self.aggregate.is_some() {
                    return invalid("names a reduction without a relation".to_string());
                }
            }
            (None, Some(target)) => {
                if target.trim().is_empty() {
                    return invalid("names an empty relation target".to_string());
                }
                if self.field().is_none() {
                    return invalid("names an unknown relation field".to_string());
                }
                if let Some(aggregate) = &self.aggregate
                    && Aggregate::parse(aggregate).is_none()
                {
                    return invalid("names an unknown reduction".to_string());
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
    pub fn of(view: &ViewState, time: u32) -> Self {
        Self {
            camera: CameraSpec::of(view.camera),
            section: SectionSpec::of(view.section),
            split: view.split,
            time,
            timeline: Some(TimelineSpec::of(&view.timeline)),
            export: Some(ExportSpec::of(view.export)),
        }
    }

    pub fn camera(&self) -> CameraState {
        self.camera.camera()
    }

    pub fn section(&self) -> SectionState {
        self.section.section()
    }

    pub fn timeline(&self) -> Option<Timeline> {
        self.timeline.as_ref().map(|timeline| {
            timeline
                .timeline()
                .expect("a checked timeline is plausible")
        })
    }

    pub fn export(&self) -> Option<Export> {
        self.export.map(ExportSpec::export)
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
        if let Some(timeline) = &self.timeline {
            timeline
                .timeline()
                .map_err(|detail| ProtocolError::Invalid(format!("a view {detail}")))?;
        }
        if let Some(export) = &self.export
            && !export.export().matches()
        {
            return Err(ProtocolError::Invalid(
                "a view exports at a plausible width".to_string(),
            ));
        }
        Ok(())
    }
}

impl ExportSpec {
    pub fn of(export: Export) -> Self {
        Self {
            width: export.width,
            transparent: export.transparent,
        }
    }

    pub fn export(self) -> Export {
        Export::new(self.width, self.transparent)
    }
}

impl CameraSpec {
    pub fn of(camera: CameraState) -> Self {
        Self {
            pivot: camera.pivot,
            yaw: camera.yaw,
            pitch: camera.pitch,
            distance: camera.distance,
            fov_y: camera.fov_y,
        }
    }

    pub fn camera(&self) -> CameraState {
        CameraState {
            pivot: self.pivot,
            yaw: self.yaw,
            pitch: self.pitch,
            distance: self.distance,
            fov_y: self.fov_y,
        }
    }
}

impl SectionSpec {
    pub fn of(section: SectionState) -> Self {
        Self {
            axis: AxisSpec::of(section.axis),
            span: section.span,
        }
    }

    pub fn section(&self) -> SectionState {
        SectionState {
            axis: self.axis.axis(),
            span: self.span,
        }
    }
}

impl TimelineSpec {
    pub fn of(timeline: &Timeline) -> Self {
        Self {
            fps: timeline.fps(),
            duration: timeline.duration(),
            keys: timeline
                .keys()
                .iter()
                .map(|key| KeySpec {
                    at: key.at,
                    time: key.time,
                    camera: CameraSpec::of(key.camera),
                    section: SectionSpec::of(key.section),
                })
                .collect(),
        }
    }

    pub fn timeline(&self) -> Result<Timeline, &'static str> {
        Timeline::from_parts(
            self.fps,
            self.duration,
            self.keys
                .iter()
                .map(|key| Keyframe {
                    at: key.at,
                    time: key.time,
                    camera: key.camera.camera(),
                    section: key.section.section(),
                })
                .collect(),
        )
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
        if let Some(transfer) = &self.transfer {
            transfer.checked(&self.name)?;
        }
        Ok(())
    }
}

impl TransferSpec {
    pub fn of(transfer: &TransferFunction) -> Self {
        Self {
            stops: transfer
                .stops()
                .iter()
                .map(|stop| StopSpec {
                    position: stop.position(),
                    color: stop.color(),
                    opacity: stop.opacity(),
                })
                .collect(),
        }
    }

    pub fn transfer(&self) -> TransferFunction {
        TransferFunction::new(
            self.stops
                .iter()
                .map(|stop| Stop::new(stop.position, stop.color, stop.opacity))
                .collect(),
        )
    }

    pub(super) fn checked(&self, channel: &str) -> Result<(), ProtocolError> {
        let invalid = |detail: String| {
            Err(ProtocolError::Invalid(format!(
                "the transfer of the channel {channel} {detail}"
            )))
        };
        if self.stops.is_empty() {
            return invalid("holds at least one stop".to_string());
        }
        for stop in &self.stops {
            if !stop.position.is_finite() || !(0.0..=1.0).contains(&stop.position) {
                return invalid("holds a stop inside the unit interval".to_string());
            }
            if stop
                .color
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return invalid("holds an srgb stop color".to_string());
            }
            if !stop.opacity.is_finite() || !(0.0..=1.0).contains(&stop.opacity) {
                return invalid("holds a unit stop opacity".to_string());
            }
        }
        Ok(())
    }
}

impl RenderSpec {
    pub fn of(shading: Shading, exposure: f32, step_scale: f32) -> Self {
        Self {
            exposure,
            step_scale,
            shading: ShadingSpec {
                ambient: shading.ambient,
                diffuse: shading.diffuse,
                specular: shading.specular,
                shininess: shading.shininess,
                shadows: shading.shadows,
            },
        }
    }

    pub fn shading(self) -> Shading {
        Shading {
            ambient: self.shading.ambient,
            diffuse: self.shading.diffuse,
            specular: self.shading.specular,
            shininess: self.shading.shininess,
            shadows: self.shading.shadows,
        }
    }

    pub(super) fn checked(self) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the rendering {detail}")));
        if !self.exposure.is_finite() || self.exposure <= 0.0 {
            return invalid("scales radiance by a positive exposure".to_string());
        }
        if !self.step_scale.is_finite() || self.step_scale <= 0.0 {
            return invalid("samples the volume at a positive step".to_string());
        }
        let shading = self.shading;
        for value in [shading.ambient, shading.diffuse, shading.specular] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return invalid("carries unit lighting weights".to_string());
            }
        }
        if !shading.shininess.is_finite() || shading.shininess <= 0.0 {
            return invalid("carries a positive shininess".to_string());
        }
        if !shading.shadows.is_finite() || !(0.0..=1.0).contains(&shading.shadows) {
            return invalid("carries a unit shadow strength".to_string());
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

    pub fn track_visible(&self) -> Option<bool> {
        self.track.map(|track| track.visible)
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

    pub fn scope(&self) -> Scope {
        self.scope.as_deref().map_or(Scope::All, |scope| {
            Scope::parse(scope).expect("a measure scope names a range")
        })
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn view(&self) -> Option<MeasureView> {
        self.view
            .as_deref()
            .map(|view| MeasureView::parse(view).expect("a measure view names a plot"))
    }

    pub fn axes(&self) -> Option<[&str; 2]> {
        self.axes.as_ref().map(|[x, y]| [x.as_str(), y.as_str()])
    }

    pub fn lasso(&self) -> Option<&LassoSpec> {
        self.lasso.as_ref()
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

    pub fn order(&self, table: &Table) -> Option<(usize, bool)> {
        let sort = self.sort.as_ref()?;
        Some((table.column(&sort.column)?, sort.descending))
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
        if let Some(scope) = &self.scope
            && Scope::parse(scope).is_none()
        {
            return invalid("names an unknown statistics range".to_string());
        }
        if let Some(view) = &self.view
            && MeasureView::parse(view).is_none()
        {
            return invalid("names an unknown statistics view".to_string());
        }
        if let Some([x, y]) = &self.axes
            && (x.trim().is_empty() || y.trim().is_empty())
        {
            return invalid("names an empty plot axis".to_string());
        }
        if let Some(lasso) = &self.lasso {
            if lasso.x.trim().is_empty() || lasso.y.trim().is_empty() {
                return invalid("names an empty lasso axis".to_string());
            }
            if lasso.x == lasso.y {
                return invalid("gates the plot on one axis twice".to_string());
            }
            if lasso.points.len() < 3 {
                return invalid("gates the plot with a polygon".to_string());
            }
            if lasso
                .points
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
            {
                return invalid("gates the plot with finite points".to_string());
            }
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
        if let Some(sort) = &self.sort
            && sort.column.trim().is_empty()
        {
            return invalid("sorts an unnamed column".to_string());
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

fn is_zero(value: &i32) -> bool {
    *value == 0
}

fn one() -> f32 {
    1.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}
