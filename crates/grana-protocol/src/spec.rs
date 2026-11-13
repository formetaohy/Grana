use glam::Vec3;
use grana_animation::{Keyframe, Timeline};
use grana_display::{
    CameraState, Material, OutputSize, Palette, Shading, Stop, TransferFunction, VolumeMode,
    VolumeStyle,
};
use grana_measure::{
    Aggregate, ChildField, ColocMetric, Filter, Group, MeasureView, RelationField, Scope,
    Statistic, Table,
};
use grana_mesh::Format as MeshFormat;
use grana_object::{
    CellParams, FilamentParams, SpotParams, SurfaceFilter, SurfaceParams, TrackParams,
};
use grana_project::{Export, ViewState};
use grana_scene::{
    Annotation, ObjectColor, ObjectKind, ObjectParams, ObjectSet, Scene, SegmentParams,
};
use grana_volume::Clip;
use grana_warp::{Grid, Warp};
use serde::{Deserialize, Serialize};

use crate::Protocol;
use crate::ProtocolError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSpec {
    pub name: String,
    pub enabled: bool,
    pub opacity: f32,
    pub window: WindowSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer: Option<TransferSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<VolumeSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeSpec {
    pub mode: String,
    pub isovalue: f32,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<ClipSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClipSpec {
    pub normal: [f32; 3],
    pub offset: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialSpec {
    pub roughness: f32,
    pub metallic: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShadingSpec {
    pub ambient: f32,
    pub diffuse: f32,
    pub shadows: f32,
    #[serde(default = "default_azimuth")]
    pub azimuth: f32,
    #[serde(default = "default_elevation")]
    pub elevation: f32,
}

fn default_azimuth() -> f32 {
    grana_display::DEFAULT_AZIMUTH
}

fn default_segment_model() -> String {
    grana_scene::SegmentModel::preferred()
        .expect("a build licenses at least one segmentation model")
        .name()
        .to_string()
}

fn default_elevation() -> f32 {
    grana_display::DEFAULT_ELEVATION
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairSpec {
    pub a: String,
    pub b: String,
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub child: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair: Option<PairSpec>,
    pub palette: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reversed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[f32; 2]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedViewSpec {
    pub name: String,
    pub view: ViewSpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewSpec {
    pub camera: CameraSpec,
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
    pub image: u32,
    pub movie: u32,
    pub transparent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSpec {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub transparent: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub movies: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sequences: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineSpec {
    pub fps: u32,
    pub duration: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<KeySpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeySpec {
    pub at: f32,
    pub time: u32,
    pub camera: CameraSpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraSpec {
    pub pivot: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub projection: String,
    pub aperture: f32,
    pub focus: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<SliceSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceSpec {
    pub axis: u8,
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub step: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationSpec {
    pub source: String,
    pub target: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskFilterSpec {
    pub source: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: i32,
    pub field: String,
    pub minimum: f32,
    pub maximum: f32,
}

impl MaskFilterSpec {
    pub fn field(&self) -> Option<grana_measure::RelationField> {
        grana_measure::RelationField::CONTINUOUS
            .into_iter()
            .find(|field| field.key() == self.field)
    }

    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        for (name, value) in [("source", &self.source), ("target", &self.target)] {
            if value.trim().is_empty() {
                return Err(ProtocolError::Invalid(format!(
                    "an analysis mask filter names a {name}"
                )));
            }
        }
        let Some(field) = self.field() else {
            return Err(ProtocolError::Invalid(
                "an analysis mask filter keeps a continuous relation field".to_string(),
            ));
        };
        if !self.minimum.is_finite() || !self.maximum.is_finite() || self.minimum > self.maximum {
            return Err(ProtocolError::Invalid(format!(
                "the analysis mask filter on {} keeps a finite range of relation values",
                field.label()
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum AnnotationSpec {
    Point {
        name: String,
        #[serde(default = "truth", skip_serializing_if = "is_true")]
        visible: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<u32>,
        at: [f32; 3],
    },
    Distance {
        name: String,
        #[serde(default = "truth", skip_serializing_if = "is_true")]
        visible: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        frame: Option<u32>,
        from: [f32; 3],
        to: [f32; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<ProfileSpec>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSpec {
    pub spacing: f32,
    pub channels: Vec<String>,
    pub samples: Vec<ProfileSampleSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSampleSpec {
    pub offset: f32,
    pub values: Vec<f32>,
}

impl ProfileSpec {
    pub fn of(profile: &grana_profile::Profile) -> Self {
        Self {
            spacing: profile.spacing(),
            channels: profile.names().to_vec(),
            samples: profile
                .samples()
                .iter()
                .map(|sample| ProfileSampleSpec {
                    offset: sample.offset(),
                    values: sample.values().to_vec(),
                })
                .collect(),
        }
    }

    pub fn profile(&self, line: grana_profile::Line) -> grana_profile::Profile {
        grana_profile::Profile::new(
            line,
            self.spacing,
            self.channels.clone(),
            self.samples
                .iter()
                .map(|sample| grana_profile::Sample::new(sample.offset, sample.values.clone()))
                .collect(),
        )
    }

    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        let invalid = |detail: String| Err(ProtocolError::Invalid(format!("the profile {detail}")));
        if !self.spacing.is_finite() || self.spacing <= 0.0 {
            return invalid("samples at a positive spacing".to_string());
        }
        if self.channels.is_empty() || self.channels.iter().any(|name| name.trim().is_empty()) {
            return invalid("names at least one channel".to_string());
        }
        if self.samples.is_empty() {
            return invalid("carries at least one sample".to_string());
        }
        let mut previous = 0.0f32;
        for sample in &self.samples {
            if !sample.offset.is_finite() || sample.offset < 0.0 || sample.offset < previous {
                return invalid("stands its samples in order along the line".to_string());
            }
            previous = sample.offset;
            if sample.values.len() != self.channels.len() {
                return invalid("measures every channel at every sample".to_string());
            }
        }
        Ok(())
    }
}

impl AnnotationSpec {
    pub fn of(annotation: &Annotation) -> Self {
        let name = annotation.name().to_string();
        let visible = annotation.visible();
        let frame = annotation.frame();
        match annotation.kind().as_point() {
            Some(at) => Self::Point {
                name,
                visible,
                frame,
                at,
            },
            None => Self::Distance {
                name,
                visible,
                frame,
                from: annotation.kind().anchor(),
                to: annotation
                    .kind()
                    .end()
                    .expect("a distance annotation names its end"),
                profile: annotation.profile().map(ProfileSpec::of),
            },
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Point { name, .. } | Self::Distance { name, .. } => name,
        }
    }

    pub fn visible(&self) -> bool {
        match self {
            Self::Point { visible, .. } | Self::Distance { visible, .. } => *visible,
        }
    }

    pub fn frame(&self) -> Option<u32> {
        match self {
            Self::Point { frame, .. } | Self::Distance { frame, .. } => *frame,
        }
    }

    pub fn profile(&self) -> Option<&ProfileSpec> {
        match self {
            Self::Point { .. } => None,
            Self::Distance { profile, .. } => profile.as_ref(),
        }
    }

    pub fn kind(&self) -> grana_scene::AnnotationKind {
        match *self {
            Self::Point { at, .. } => grana_scene::AnnotationKind::point(at),
            Self::Distance { from, to, .. } => grana_scene::AnnotationKind::distance(from, to),
        }
    }

    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        if self.name().trim().is_empty() {
            return Err(ProtocolError::Invalid(
                "an annotation names itself".to_string(),
            ));
        }
        let finite = |point: [f32; 3]| point.iter().all(|value| value.is_finite());
        match *self {
            Self::Point { at, .. } => {
                if !finite(at) {
                    return Err(ProtocolError::Invalid(
                        "an annotation point lies in the volume".to_string(),
                    ));
                }
            }
            Self::Distance { from, to, .. } => {
                if !finite(from) || !finite(to) || from == to {
                    return Err(ProtocolError::Invalid(
                        "an annotation distance spans two distinct points".to_string(),
                    ));
                }
            }
        }
        if let Self::Distance {
            profile: Some(profile),
            ..
        } = self
        {
            profile.checked()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSpec {
    pub center: [f32; 3],
    pub axes: [[f32; 3]; 3],
    pub half: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StitchSpec {
    pub rows: usize,
    pub columns: usize,
    pub channel: String,
}

impl StitchSpec {
    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        if self.rows == 0 || self.columns == 0 {
            return Err(ProtocolError::Invalid(
                "a tile grid holds at least one row and one column".to_string(),
            ));
        }
        if self.rows * self.columns < 2 {
            return Err(ProtocolError::Invalid(
                "a tile grid spans at least two tiles".to_string(),
            ));
        }
        if self.channel.trim().is_empty() {
            return Err(ProtocolError::Invalid(
                "a tile registration names a channel".to_string(),
            ));
        }
        Ok(())
    }

    pub fn tiles(&self) -> usize {
        self.rows * self.columns
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskSpec {
    pub set: String,
    pub channel: String,
    pub name: String,
    pub tint: [f32; 3],
}

impl MaskSpec {
    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        for (label, value) in [
            ("set", &self.set),
            ("channel", &self.channel),
            ("name", &self.name),
        ] {
            if value.trim().is_empty() {
                return Err(ProtocolError::Invalid(format!(
                    "a mask channel names a {label}"
                )));
            }
        }
        if self.tint.iter().any(|value| !(0.0..=1.0).contains(value)) {
            return Err(ProtocolError::Invalid(format!(
                "the tint of the mask channel {} is not an srgb triple",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DerivedSpec {
    Mask(MaskSpec),
    Restore(RestoreSpec),
    Smooth(SmoothSpec),
    Background(BackgroundSpec),
    Ratio(RatioSpec),
    Bleach(BleachSpec),
}

impl DerivedSpec {
    pub fn name(&self) -> &str {
        match self {
            Self::Mask(spec) => &spec.name,
            Self::Restore(spec) => &spec.name,
            Self::Smooth(spec) => &spec.name,
            Self::Background(spec) => &spec.name,
            Self::Ratio(spec) => &spec.name,
            Self::Bleach(spec) => &spec.name,
        }
    }

    pub fn channel(&self) -> &str {
        match self {
            Self::Mask(spec) => &spec.channel,
            Self::Restore(spec) => &spec.channel,
            Self::Smooth(spec) => &spec.channel,
            Self::Background(spec) => &spec.channel,
            Self::Ratio(spec) => &spec.channel,
            Self::Bleach(spec) => &spec.channel,
        }
    }

    pub fn checked(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Mask(spec) => spec.checked(),
            Self::Restore(spec) => spec.checked(),
            Self::Smooth(spec) => spec.checked(),
            Self::Background(spec) => spec.checked(),
            Self::Ratio(spec) => spec.checked(),
            Self::Bleach(spec) => spec.checked(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreSpec {
    pub channel: String,
    pub name: String,
    pub tint: [f32; 3],
    pub intensity: [f32; 2],
    pub numerical_aperture: f32,
    pub wavelength: f32,
    pub immersion: f32,
    pub sample: f32,
    pub depth: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<MeasuredSpec>,
    pub iterations: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasuredSpec {
    pub spacing: [f32; 3],
    pub dims: [u32; 3],
    pub values: Vec<f32>,
}

impl RestoreSpec {
    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        for (label, value) in [("channel", &self.channel), ("name", &self.name)] {
            if value.trim().is_empty() {
                return Err(ProtocolError::Invalid(format!(
                    "a restored channel names a {label}"
                )));
            }
        }
        if self.tint.iter().any(|value| !(0.0..=1.0).contains(value)) {
            return Err(ProtocolError::Invalid(format!(
                "the tint of the restored channel {} is not an srgb triple",
                self.name
            )));
        }
        if !(self.intensity[0].is_finite() && self.intensity[1].is_finite())
            || self.intensity[0] >= self.intensity[1]
        {
            return Err(ProtocolError::Invalid(format!(
                "the intensity range of the restored channel {} is unordered",
                self.name
            )));
        }
        let physical = [
            self.numerical_aperture,
            self.wavelength,
            self.immersion,
            self.sample,
            self.depth,
        ];
        if self.measured.is_none() && physical.iter().any(|value| !value.is_finite()) {
            return Err(ProtocolError::Invalid(format!(
                "the optics of the restored channel {} are not finite",
                self.name
            )));
        }
        if let Some(measured) = &self.measured {
            let finite = measured.spacing.iter().all(|value| value.is_finite());
            let dims = measured
                .dims
                .iter()
                .all(|extent| *extent % 2 == 1 && (1..=63).contains(extent));
            let voxels =
                measured.dims[0] as u64 * measured.dims[1] as u64 * measured.dims[2] as u64;
            let layers = (measured.values.len() as u64)
                .checked_div(voxels)
                .unwrap_or(0);
            let holds = voxels > 0
                && measured.values.len() as u64 == voxels * layers
                && (1..=16).contains(&layers)
                && measured
                    .values
                    .iter()
                    .all(|value| value.is_finite() && *value >= 0.0)
                && (0..layers).all(|layer| {
                    let start = (layer * voxels) as usize;
                    let total: f32 = measured.values[start..start + voxels as usize].iter().sum();
                    total > 0.0
                });
            if !(finite && dims && holds) {
                return Err(ProtocolError::Invalid(format!(
                    "the measured point spread function of the restored channel {} is malformed",
                    self.name
                )));
            }
        }
        if !(1..=1000).contains(&self.iterations) {
            return Err(ProtocolError::Invalid(format!(
                "the restored channel {} runs an unbounded number of iterations",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmoothSpec {
    pub channel: String,
    pub name: String,
    pub tint: [f32; 3],
    pub intensity: [f32; 2],
    pub sigma: f32,
}

impl SmoothSpec {
    fn checked(&self) -> Result<(), ProtocolError> {
        derived_common(
            &self.channel,
            &self.name,
            self.tint,
            self.intensity,
            "smoothed",
        )?;
        if !(self.sigma.is_finite() && self.sigma > 0.0) {
            return Err(ProtocolError::Invalid(format!(
                "the smoothed channel {} blurs by a positive distance",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackgroundSpec {
    pub channel: String,
    pub name: String,
    pub tint: [f32; 3],
    pub intensity: [f32; 2],
    pub radius: f32,
}

impl BackgroundSpec {
    fn checked(&self) -> Result<(), ProtocolError> {
        derived_common(
            &self.channel,
            &self.name,
            self.tint,
            self.intensity,
            "background",
        )?;
        if !(self.radius.is_finite() && self.radius > 0.0) {
            return Err(ProtocolError::Invalid(format!(
                "the background channel {} measures a positive radius",
                self.name
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatioSpec {
    pub channel: String,
    pub denominator: String,
    pub name: String,
    pub tint: [f32; 3],
    pub intensity: [f32; 2],
}

impl RatioSpec {
    fn checked(&self) -> Result<(), ProtocolError> {
        derived_common(
            &self.channel,
            &self.name,
            self.tint,
            self.intensity,
            "ratio",
        )?;
        if self.denominator.trim().is_empty() {
            return Err(ProtocolError::Invalid(format!(
                "the ratio channel {} names a denominator channel",
                self.name
            )));
        }
        if self.denominator == self.channel {
            return Err(ProtocolError::Invalid(format!(
                "the ratio channel {} divides two distinct channels",
                self.name
            )));
        }
        Ok(())
    }
}

fn derived_common(
    channel: &str,
    name: &str,
    tint: [f32; 3],
    intensity: [f32; 2],
    label: &str,
) -> Result<(), ProtocolError> {
    for (field, value) in [("channel", channel), ("name", name)] {
        if value.trim().is_empty() {
            return Err(ProtocolError::Invalid(format!(
                "a {label} channel names a {field}"
            )));
        }
    }
    if tint.iter().any(|value| !(0.0..=1.0).contains(value)) {
        return Err(ProtocolError::Invalid(format!(
            "the tint of the {label} channel {name} is not an srgb triple"
        )));
    }
    if !(intensity[0].is_finite() && intensity[1].is_finite()) || intensity[0] >= intensity[1] {
        return Err(ProtocolError::Invalid(format!(
            "the intensity range of the {label} channel {name} is unordered"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriftSpec {
    pub channel: String,
    #[serde(default, skip_serializing_if = "is_u32_zero")]
    pub reference: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lattice: Option<LatticeSpec>,
    pub warps: Vec<WarpSpec>,
}

impl DriftSpec {
    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn reference(&self) -> u32 {
        self.reference
    }

    pub fn lattice(&self) -> Option<&LatticeSpec> {
        self.lattice.as_ref()
    }

    pub fn warps(&self) -> &[WarpSpec] {
        &self.warps
    }

    pub fn checked(&self) -> Result<(), ProtocolError> {
        if self.warps.is_empty() {
            return Err(ProtocolError::Invalid(
                "the drift correction carries no frame warps".to_string(),
            ));
        }
        if self.reference as usize >= self.warps.len() {
            return Err(ProtocolError::Invalid(format!(
                "the drift reference frame {} is not one of the aligned frames",
                self.reference
            )));
        }
        let limit = grana_scene::Drift::LIMIT;
        let mut deformed = false;
        for (index, warp) in self.warps.iter().enumerate() {
            match warp {
                WarpSpec::Shift { offset } => {
                    if offset
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() > limit)
                    {
                        return Err(ProtocolError::Invalid(format!(
                            "the drift offset of frame {} stays inside the volume",
                            index + 1
                        )));
                    }
                }
                WarpSpec::Field { values } => {
                    deformed = true;
                    let lattice = self.lattice.as_ref().ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the deformation of frame {} names no displacement lattice",
                            index + 1
                        ))
                    })?;
                    let count =
                        lattice.dims[0] as u64 * lattice.dims[1] as u64 * lattice.dims[2] as u64;
                    if count != values.len() as u64 {
                        return Err(ProtocolError::Invalid(format!(
                            "frame {} holds {} displacement nodes and its lattice holds {count}",
                            index + 1,
                            values.len()
                        )));
                    }
                    if values
                        .iter()
                        .flatten()
                        .any(|value| !value.is_finite() || value.abs() > limit)
                    {
                        return Err(ProtocolError::Invalid(format!(
                            "the deformation of frame {} stays inside the volume",
                            index + 1
                        )));
                    }
                }
            }
        }
        if let Some(lattice) = &self.lattice {
            lattice.checked(deformed)?;
        }
        let built = self.built().map_err(ProtocolError::Invalid)?;
        if !built[self.reference as usize].identity() {
            return Err(ProtocolError::Invalid(
                "the drift reference frame holds still".to_string(),
            ));
        }
        Ok(())
    }

    pub fn built(&self) -> Result<Vec<Warp>, String> {
        let mut warps = Vec::with_capacity(self.warps.len());
        for (index, warp) in self.warps.iter().enumerate() {
            warps.push(match warp {
                WarpSpec::Shift { offset } => Warp::shift(*offset),
                WarpSpec::Field { values } => {
                    let lattice = self.lattice.as_ref().ok_or_else(|| {
                        format!(
                            "the deformation of frame {} names no displacement lattice",
                            index + 1
                        )
                    })?;
                    let count =
                        lattice.dims[0] as u64 * lattice.dims[1] as u64 * lattice.dims[2] as u64;
                    if count != values.len() as u64 {
                        return Err(format!(
                            "frame {} holds {} displacement nodes and its lattice holds {count}",
                            index + 1,
                            values.len()
                        ));
                    }
                    Warp::field(Grid::new(
                        lattice.dims,
                        lattice.origin,
                        lattice.step,
                        values.clone(),
                    ))
                }
            });
        }
        Ok(warps)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationSpec {
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lattice: Option<LatticeSpec>,
    pub warps: Vec<WarpSpec>,
}

impl RegistrationSpec {
    pub fn reference(&self) -> &str {
        &self.reference
    }

    pub fn lattice(&self) -> Option<&LatticeSpec> {
        self.lattice.as_ref()
    }

    pub fn warps(&self) -> &[WarpSpec] {
        &self.warps
    }

    pub fn checked(&self) -> Result<(), ProtocolError> {
        if self.warps.is_empty() {
            return Err(ProtocolError::Invalid(
                "the channel registration carries no channel warps".to_string(),
            ));
        }
        let mut deformed = false;
        for (index, warp) in self.warps.iter().enumerate() {
            match warp {
                WarpSpec::Shift { offset: shift } => {
                    if shift.iter().any(|value| {
                        !value.is_finite() || value.abs() > grana_scene::Registration::LIMIT
                    }) {
                        return Err(ProtocolError::Invalid(format!(
                            "the registration offset of channel {} stays inside the volume",
                            index + 1
                        )));
                    }
                }
                WarpSpec::Field { values } => {
                    deformed = true;
                    let lattice = self.lattice.as_ref().ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the deformation of channel {} names no displacement lattice",
                            index + 1
                        ))
                    })?;
                    if lattice.nodes() != values.len() as u64 {
                        return Err(ProtocolError::Invalid(format!(
                            "channel {} holds {} displacement nodes and its lattice holds {}",
                            index + 1,
                            values.len(),
                            lattice.nodes()
                        )));
                    }
                    if values.iter().flatten().any(|value| {
                        !value.is_finite() || value.abs() > grana_scene::Registration::LIMIT
                    }) {
                        return Err(ProtocolError::Invalid(format!(
                            "the deformation of channel {} stays inside the volume",
                            index + 1
                        )));
                    }
                }
            }
        }
        if let Some(lattice) = &self.lattice {
            lattice.checked(deformed)?;
        }
        Ok(())
    }

    pub fn built(&self) -> Result<Vec<Warp>, String> {
        let mut warps = Vec::with_capacity(self.warps.len());
        for (index, warp) in self.warps.iter().enumerate() {
            warps.push(match warp {
                WarpSpec::Shift { offset } => Warp::shift(*offset),
                WarpSpec::Field { values } => {
                    let lattice = self.lattice.as_ref().ok_or_else(|| {
                        format!(
                            "the deformation of channel {} names no displacement lattice",
                            index + 1
                        )
                    })?;
                    if lattice.nodes() != values.len() as u64 {
                        return Err(format!(
                            "channel {} holds {} displacement nodes and its lattice holds {}",
                            index + 1,
                            values.len(),
                            lattice.nodes()
                        ));
                    }
                    Warp::field(Grid::new(
                        lattice.dims,
                        lattice.origin,
                        lattice.step,
                        values.clone(),
                    ))
                }
            });
        }
        Ok(warps)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatticeSpec {
    pub dims: [u32; 3],
    pub origin: [f32; 3],
    pub step: [f32; 3],
}

impl LatticeSpec {
    fn nodes(&self) -> u64 {
        self.dims[0] as u64 * self.dims[1] as u64 * self.dims[2] as u64
    }

    fn checked(&self, deformed: bool) -> Result<(), ProtocolError> {
        if self.dims.contains(&0) {
            return Err(ProtocolError::Invalid(
                "the displacement lattice spans at least one node per axis".to_string(),
            ));
        }
        if self.origin.iter().any(|value| !value.is_finite()) {
            return Err(ProtocolError::Invalid(
                "the displacement lattice starts at a finite world point".to_string(),
            ));
        }
        if self
            .step
            .iter()
            .any(|value| !value.is_finite() || *value == 0.0)
        {
            return Err(ProtocolError::Invalid(
                "the displacement lattice steps by a nonzero world distance".to_string(),
            ));
        }
        if !deformed {
            return Err(ProtocolError::Invalid(
                "the displacement lattice carries no deformation".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum WarpSpec {
    Shift { offset: [f32; 3] },
    Field { values: Vec<[f32; 3]> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BleachSpec {
    pub channel: String,
    pub name: String,
    pub tint: [f32; 3],
    pub intensity: [f32; 2],
}

impl BleachSpec {
    fn checked(&self) -> Result<(), ProtocolError> {
        derived_common(
            &self.channel,
            &self.name,
            self.tint,
            self.intensity,
            "bleach corrected",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statistic: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub child: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub shift: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetSpec {
    pub key: String,
    pub name: String,
    pub color: [f32; 3],
    #[serde(default = "truth", skip_serializing_if = "is_true")]
    pub visible: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub labels: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<MetricSpec>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_by: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_by: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier: Option<ClassifierSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_coloc: Option<PairSpec>,
    pub source: SourceSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curation: Option<CurationSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<MeasureSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meshes: Vec<MeshFormat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoleculesSpec {
    pub frames: [u32; 2],
    pub pixel: f32,
    pub blur: f32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub projected: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub clustered: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub group_colored: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_photons: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_precision: Option<f32>,
    pub radius: f32,
    pub dark: u32,
    pub minimum: u32,
}

impl MoleculesSpec {
    pub fn of(source: &grana_scene::LocalizationSource) -> Self {
        let spec = source.spec();
        let clusters = source.clusters();
        let filter = spec.filter();
        Self {
            frames: spec.frames(),
            pixel: spec.pixel(),
            blur: spec.blur(),
            projected: source.projected(),
            clustered: source.clustered(),
            group_colored: source.group_colored(),
            min_photons: (filter.photons() > 0.0).then_some(filter.photons()),
            max_precision: (filter.precision() > 0.0).then_some(filter.precision()),
            radius: clusters.radius(),
            dark: clusters.dark(),
            minimum: clusters.minimum(),
        }
    }

    pub fn localization(&self) -> grana_object::LocalizationSpec {
        grana_object::LocalizationSpec::new(self.frames, self.pixel, self.blur)
            .at_filter(self.filter())
    }

    pub fn clusters(&self) -> grana_object::ClusterSpec {
        grana_object::ClusterSpec::new(self.radius, self.dark, self.minimum)
    }

    pub fn filter(&self) -> grana_object::LocalizationFilter {
        grana_object::LocalizationFilter::new(
            self.min_photons.unwrap_or(0.0),
            self.max_precision.unwrap_or(0.0),
        )
    }

    fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the set {set} {detail}")));
        if self.frames[0] > self.frames[1] {
            return invalid("renders a frame range that ends before it starts".to_string());
        }
        if !(grana_object::LocalizationSpec::PIXEL_MINIMUM
            ..=grana_object::LocalizationSpec::PIXEL_MAXIMUM)
            .contains(&self.pixel)
        {
            return invalid("renders at an unsupported pixel size".to_string());
        }
        if !(0.0..=grana_object::LocalizationSpec::BLUR_MAXIMUM).contains(&self.blur) {
            return invalid("blurs by an unsupported width".to_string());
        }
        if self.min_photons.is_some_and(|photons| {
            !(0.0..=grana_object::LocalizationFilter::PHOTONS_MAXIMUM).contains(&photons)
        }) {
            return invalid("filters localizations by an unsupported photon count".to_string());
        }
        if self.max_precision.is_some_and(|precision| {
            !(0.0..=grana_object::LocalizationFilter::PRECISION_MAXIMUM).contains(&precision)
        }) {
            return invalid("filters localizations by an unsupported precision".to_string());
        }
        if !(grana_object::ClusterSpec::RADIUS_MINIMUM..=grana_object::ClusterSpec::RADIUS_MAXIMUM)
            .contains(&self.radius)
        {
            return invalid("clusters within an unsupported radius".to_string());
        }
        if self.dark > grana_object::ClusterSpec::DARK_MAXIMUM {
            return invalid("clusters over an unsupported count of dark frames".to_string());
        }
        if self.minimum == 0 || self.minimum > grana_object::ClusterSpec::MINIMUM_MAXIMUM {
            return invalid("clusters with an unsupported minimum of localizations".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum SourceSpec {
    Spots {
        channel: String,
        diameter: f32,
        contrast: f32,
    },
    Surfaces {
        channel: String,
        threshold: f32,
        grain: f32,
        split: f32,
        min_volume: f32,
        opening: f32,
        closing: f32,
        min_sphericity: f32,
        min_mean_intensity: f32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        seeds: Vec<Vec<[f32; 3]>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed_source: Option<String>,
    },
    Filaments {
        channel: String,
        threshold: f32,
        min_length: f32,
    },
    Cells {
        channel: String,
        reach: f32,
        grain: f32,
        seed_source: String,
    },
    Segments {
        channel: String,
        cellprob: f32,
        flow: f32,
        minimum_size: u32,
        tile_overlap: u32,
        slice_overlap: f32,
        #[serde(default = "default_segment_model")]
        model: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        seeds: Vec<Vec<[f32; 3]>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seed_source: Option<String>,
    },
    Labels {
        label: String,
    },
    External {
        file: String,
    },
    Localizations {
        file: String,
        molecules: MoleculesSpec,
    },
    Filtered {
        source: String,
        #[serde(default = "objects", skip_serializing_if = "is_objects")]
        group: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        filters: Vec<FilterSpec>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lasso: Option<LassoSpec>,
    },
    Copy {
        source: String,
    },
    Edited {
        source: String,
        edits: Vec<SurfaceEditSpec>,
    },
    Masked {
        source: String,
        edits: Vec<MaskEditSpec>,
    },
    Merged {
        parts: Vec<PartSpec>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartSpec {
    pub name: String,
    pub source: SourceSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curation: Option<CurationSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassifierSpec {
    pub features: Vec<String>,
    pub mean: Vec<f64>,
    pub scale: Vec<f64>,
    pub weights: Vec<f64>,
    pub bias: f64,
    #[serde(default, skip_serializing_if = "is_u32_zero")]
    pub positives: u32,
    #[serde(default, skip_serializing_if = "is_u32_zero")]
    pub negatives: u32,
    #[serde(default, skip_serializing_if = "is_f32_zero")]
    pub accuracy: f32,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compare: Vec<String>,
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
#[serde(deny_unknown_fields)]
pub struct CurationSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<RemovalSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden: Vec<HiddenSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<EditSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalSpec {
    pub frame: u32,
    pub anchors: Vec<AnchorSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HiddenSpec {
    pub frame: u32,
    pub anchors: Vec<AnchorSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorSpec {
    pub position: [f32; 3],
    pub low: [f32; 3],
    pub high: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    pub frame: u32,
    pub object: usize,
    pub position: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum EditSpec {
    Cut {
        node: NodeSpec,
    },
    Join {
        source: NodeSpec,
        target: NodeSpec,
    },
    Divide {
        parent: NodeSpec,
        first: NodeSpec,
        second: NodeSpec,
    },
    Merge {
        first: NodeSpec,
        second: NodeSpec,
        child: NodeSpec,
    },
    Reparent {
        child: NodeSpec,
        parent: NodeSpec,
    },
    Detach {
        child: NodeSpec,
    },
}

impl CurationSpec {
    pub fn of(curation: &grana_scene::Curation) -> Option<Self> {
        if curation.is_empty() {
            return None;
        }
        let removed = curation
            .removed()
            .iter()
            .enumerate()
            .filter(|(_, anchors)| !anchors.is_empty())
            .map(|(frame, anchors)| RemovalSpec {
                frame: frame as u32,
                anchors: anchors
                    .iter()
                    .map(|anchor| AnchorSpec::of(*anchor))
                    .collect(),
            })
            .collect();
        let hidden = curation
            .hidden()
            .iter()
            .enumerate()
            .filter(|(_, anchors)| !anchors.is_empty())
            .map(|(frame, anchors)| HiddenSpec {
                frame: frame as u32,
                anchors: anchors
                    .iter()
                    .map(|anchor| AnchorSpec::of(*anchor))
                    .collect(),
            })
            .collect();
        let edits = curation.edits().iter().map(EditSpec::of).collect();
        Some(Self {
            removed,
            hidden,
            edits,
        })
    }

    pub fn curation(&self, frames: usize) -> Result<grana_scene::Curation, String> {
        let mut removed = vec![Vec::new(); frames];
        for spec in &self.removed {
            let Some(slot) = removed.get_mut(spec.frame as usize) else {
                return Err(format!(
                    "the protocol removes objects from the frame {} of {frames}",
                    spec.frame
                ));
            };
            *slot = spec.anchors.iter().map(|anchor| anchor.anchor()).collect();
        }
        let mut hidden = vec![Vec::new(); frames];
        for spec in &self.hidden {
            let Some(slot) = hidden.get_mut(spec.frame as usize) else {
                return Err(format!(
                    "the protocol hides objects of the frame {} of {frames}",
                    spec.frame
                ));
            };
            *slot = spec.anchors.iter().map(|anchor| anchor.anchor()).collect();
        }
        let edits = self
            .edits
            .iter()
            .map(|edit| edit.edit())
            .collect::<Vec<_>>();
        grana_scene::Curation::restored(frames, &removed, &hidden, &edits)
    }
}

impl AnchorSpec {
    fn of(anchor: grana_scene::ObjectAnchor) -> Self {
        let bounds = anchor.bounds();
        Self {
            position: anchor.position(),
            low: bounds.min(),
            high: bounds.max(),
        }
    }

    fn anchor(self) -> grana_scene::ObjectAnchor {
        grana_scene::ObjectAnchor::new(
            self.position,
            grana_geometry::Bounds::new(self.low, self.high),
        )
    }
}

impl EditSpec {
    fn of(edit: &grana_scene::TrackEdit) -> Self {
        let node = |node: grana_scene::TrackAnchor| NodeSpec {
            frame: node.time(),
            object: node.object(),
            position: node.position(),
        };
        match edit {
            grana_scene::TrackEdit::Cut(anchor) => Self::Cut {
                node: node(*anchor),
            },
            grana_scene::TrackEdit::Join { source, target } => Self::Join {
                source: node(*source),
                target: node(*target),
            },
            grana_scene::TrackEdit::Divide {
                parent,
                first,
                second,
            } => Self::Divide {
                parent: node(*parent),
                first: node(*first),
                second: node(*second),
            },
            grana_scene::TrackEdit::Merge {
                first,
                second,
                child,
            } => Self::Merge {
                first: node(*first),
                second: node(*second),
                child: node(*child),
            },
            grana_scene::TrackEdit::Reparent { child, parent } => Self::Reparent {
                child: node(*child),
                parent: node(*parent),
            },
            grana_scene::TrackEdit::Detach { child } => Self::Detach {
                child: node(*child),
            },
        }
    }

    pub fn edit(self) -> grana_scene::TrackEdit {
        let node =
            |spec: NodeSpec| grana_scene::TrackAnchor::new(spec.frame, spec.object, spec.position);
        match self {
            Self::Cut { node: spec } => grana_scene::TrackEdit::Cut(node(spec)),
            Self::Join { source, target } => grana_scene::TrackEdit::Join {
                source: node(source),
                target: node(target),
            },
            Self::Divide {
                parent,
                first,
                second,
            } => grana_scene::TrackEdit::Divide {
                parent: node(parent),
                first: node(first),
                second: node(second),
            },
            Self::Merge {
                first,
                second,
                child,
            } => grana_scene::TrackEdit::Merge {
                first: node(first),
                second: node(second),
                child: node(child),
            },
            Self::Reparent { child, parent } => grana_scene::TrackEdit::Reparent {
                child: node(child),
                parent: node(parent),
            },
            Self::Detach { child } => grana_scene::TrackEdit::Detach { child: node(child) },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum SurfaceEditSpec {
    Merge {
        frame: u32,
        anchors: Vec<AnchorSpec>,
    },
    Split {
        frame: u32,
        anchor: AnchorSpec,
    },
}

impl SurfaceEditSpec {
    pub fn frame(&self) -> u32 {
        match self {
            Self::Merge { frame, .. } | Self::Split { frame, .. } => *frame,
        }
    }

    pub(super) fn of(edit: &grana_scene::SurfaceEdit) -> Self {
        let anchors = || {
            edit.anchors()
                .iter()
                .map(|anchor| AnchorSpec::of(*anchor))
                .collect::<Vec<_>>()
        };
        match edit.kind() {
            grana_scene::SurfaceEditKind::Merge => Self::Merge {
                frame: edit.frame(),
                anchors: anchors(),
            },
            grana_scene::SurfaceEditKind::Split => Self::Split {
                frame: edit.frame(),
                anchor: AnchorSpec::of(edit.anchors()[0]),
            },
        }
    }

    pub(super) fn edit(self) -> Result<grana_scene::SurfaceEdit, String> {
        match self {
            Self::Merge { frame, anchors } => grana_scene::SurfaceEdit::restored(
                frame,
                grana_scene::SurfaceEditKind::Merge,
                anchors.iter().map(|anchor| anchor.anchor()).collect(),
            ),
            Self::Split { frame, anchor } => grana_scene::SurfaceEdit::restored(
                frame,
                grana_scene::SurfaceEditKind::Split,
                vec![anchor.anchor()],
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum MaskEditSpec {
    Brush {
        frame: u32,
        anchor: AnchorSpec,
        mode: String,
        radius: f32,
        points: Vec<[f32; 3]>,
    },
    Wand {
        frame: u32,
        anchor: AnchorSpec,
        mode: String,
        tolerance: f32,
        at: [f32; 3],
    },
}

impl MaskEditSpec {
    pub fn frame(&self) -> u32 {
        match self {
            Self::Brush { frame, .. } | Self::Wand { frame, .. } => *frame,
        }
    }

    pub(super) fn of(edit: &grana_object::MaskEdit) -> Self {
        let anchor = AnchorSpec::of(edit.anchor());
        let mode = mask_mode(edit.mode()).to_string();
        match edit.tool() {
            grana_object::MaskTool::Brush => Self::Brush {
                frame: edit.frame(),
                anchor,
                mode,
                radius: edit.radius(),
                points: edit.points().to_vec(),
            },
            grana_object::MaskTool::Wand => Self::Wand {
                frame: edit.frame(),
                anchor,
                mode,
                tolerance: edit.tolerance(),
                at: edit.points()[0],
            },
        }
    }

    pub(super) fn edit(self) -> Result<grana_object::MaskEdit, String> {
        match self {
            Self::Brush {
                frame,
                anchor,
                mode,
                radius,
                points,
            } => grana_object::MaskEdit::brush(
                frame,
                anchor.anchor(),
                mask_mode_of(&mode)?,
                radius,
                points,
            ),
            Self::Wand {
                frame,
                anchor,
                mode,
                tolerance,
                at,
            } => grana_object::MaskEdit::wand(
                frame,
                anchor.anchor(),
                mask_mode_of(&mode)?,
                tolerance,
                at,
            ),
        }
    }
}

fn mask_mode(mode: grana_object::MaskMode) -> &'static str {
    match mode {
        grana_object::MaskMode::Add => "add",
        grana_object::MaskMode::Remove => "remove",
    }
}

fn mask_mode_of(mode: &str) -> Result<grana_object::MaskMode, String> {
    match mode {
        "add" => Ok(grana_object::MaskMode::Add),
        "remove" => Ok(grana_object::MaskMode::Remove),
        _ => Err(format!("the mask edit carries the unknown mode {mode}")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSpec {
    pub max_step: f32,
    pub max_gap: u32,
    pub division: bool,
    pub division_range: f32,
    pub merge: bool,
    pub merge_range: f32,
    #[serde(default, skip_serializing_if = "is_f32_zero")]
    pub merge_overlap: f32,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColocSettingsSpec {
    pub permutations: u32,
    pub block: u32,
    #[serde(default = "default_coloc_precision")]
    pub precision: String,
}

fn default_coloc_precision() -> String {
    grana_measure::ColocPrecision::default().name().to_string()
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
        let precision = grana_measure::ColocPrecision::parse(&self.settings.precision)
            .expect("a checked colocalization names a supported precision");
        grana_measure::ColocSettings::new(self.settings.permutations, self.settings.block)
            .with_precision(precision)
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

    pub fn coloc_metric(&self) -> Option<ColocMetric> {
        let key = self.key.as_deref()?;
        ColocMetric::ALL
            .into_iter()
            .find(|metric| metric.name() == key)
    }

    pub fn child_field(&self, set: &ObjectSet, keys: &[String]) -> Option<ChildField> {
        let key = self.key.as_deref()?;
        ChildField::parse(key, self.aggregate.as_deref(), keys)
            .filter(|field| set.child_fields().contains(field))
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
        if self.child {
            let Some(target) = &self.target else {
                return invalid("names no child set".to_string());
            };
            if target.trim().is_empty() {
                return invalid("names an empty child set".to_string());
            }
            let Some(key) = &self.key else {
                return invalid("names no child measurement".to_string());
            };
            if key.trim().is_empty() {
                return invalid("names an empty child measurement".to_string());
            }
            if self.field.is_some() {
                return invalid("names a relation field of a child".to_string());
            }
            if self.shift != 0 {
                return invalid("shifts the children of an object".to_string());
            }
            if key == "count" {
                if self.aggregate.is_some() {
                    return invalid("reduces a child count".to_string());
                }
            } else if let Some(statistic) = &self.aggregate
                && !Statistic::ALL
                    .into_iter()
                    .any(|candidate| candidate.key() == statistic)
            {
                return invalid("names an unknown child reduction".to_string());
            } else if self.aggregate.is_none() {
                return invalid("names a child measurement without a reduction".to_string());
            }
        } else if let Some(pair) = &self.pair {
            if self.target.is_some() || self.field.is_some() || self.aggregate.is_some() {
                return invalid(
                    "combines an object colocalization with another metric".to_string(),
                );
            }
            if self.shift != 0 {
                return invalid("shifts an object colocalization".to_string());
            }
            if pair.a.trim().is_empty() || pair.b.trim().is_empty() {
                return invalid("names an empty object colocalization channel".to_string());
            }
            if pair.a == pair.b {
                return invalid("colocalizes one channel with itself".to_string());
            }
            if self.coloc_metric().is_none() {
                return invalid("names no object colocalization metric".to_string());
            }
        } else {
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
                    if self.shift != 0 {
                        return invalid("shifts a measurement that names no relation".to_string());
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

impl ClassifierSpec {
    pub fn of(classifier: &grana_measure::Classifier, keys: &[String]) -> Self {
        let state = classifier.state();
        Self {
            features: state
                .features
                .iter()
                .map(|feature| feature.key(keys))
                .collect(),
            mean: state.mean,
            scale: state.scale,
            weights: state.weights,
            bias: state.bias,
            positives: state.positives as u32,
            negatives: state.negatives as u32,
            accuracy: state.accuracy,
        }
    }

    pub fn classifier(
        &self,
        set: &ObjectSet,
        keys: &[String],
    ) -> Result<grana_measure::Classifier, ProtocolError> {
        let features = self
            .features
            .iter()
            .map(|key| {
                set.colorable_measurements()
                    .into_iter()
                    .find(|measurement| measurement.key(keys) == *key)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the classifier of {} trains on {key}, which the set does not measure",
                            set.name()
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        grana_measure::Classifier::restore(grana_measure::ClassifierState {
            features,
            mean: self.mean.clone(),
            scale: self.scale.clone(),
            weights: self.weights.clone(),
            bias: self.bias,
            positives: self.positives as usize,
            negatives: self.negatives as usize,
            accuracy: self.accuracy,
        })
        .map_err(ProtocolError::Invalid)
    }

    pub(super) fn checked(&self, what: &str) -> Result<(), ProtocolError> {
        let invalid = |detail: String| {
            Err(ProtocolError::Invalid(format!(
                "the classifier of {what} {detail}"
            )))
        };
        if self.features.is_empty() {
            return invalid("trains on no measurement".to_string());
        }
        if self.mean.len() != self.features.len()
            || self.scale.len() != self.features.len()
            || self.weights.len() != self.features.len()
        {
            return invalid("carries coefficients of another length".to_string());
        }
        if !self
            .mean
            .iter()
            .chain(&self.scale)
            .chain(&self.weights)
            .chain([&self.bias])
            .all(|value| value.is_finite())
        {
            return invalid("carries a non-finite coefficient".to_string());
        }
        if self.scale.iter().any(|value| *value <= 0.0) {
            return invalid("scales a measurement by a non-positive factor".to_string());
        }
        if !self.accuracy.is_finite() {
            return invalid("records a non-finite accuracy".to_string());
        }
        if self
            .features
            .iter()
            .any(|feature| feature.trim().is_empty())
        {
            return invalid("trains on an empty measurement name".to_string());
        }
        Ok(())
    }
}

impl MetricSpec {
    pub fn of(metric: ObjectColor, scene: &Scene, keys: &[String]) -> Option<Self> {
        Some(match metric {
            ObjectColor::Coloc { .. } => return None,
            ObjectColor::Score => Self {
                key: Some(grana_measure::SCORE_KEY.to_string()),
                target: None,
                field: None,
                statistic: None,
                child: false,
                shift: 0,
            },
            ObjectColor::Measurement(measurement) => Self {
                key: Some(measurement.key(keys)),
                target: None,
                field: None,
                statistic: None,
                child: false,
                shift: 0,
            },
            ObjectColor::Relation {
                target,
                field,
                shift,
            } => Self {
                key: None,
                target: Some(scene.objects.get(target)?.key().as_str().to_string()),
                field: Some(field.key().to_string()),
                statistic: None,
                child: false,
                shift,
            },
            ObjectColor::Child { target, field } => Self {
                key: Some(
                    field
                        .metric()
                        .map_or_else(|| "count".to_string(), |metric| metric.key(keys)),
                ),
                target: Some(scene.objects.get(target)?.key().as_str().to_string()),
                field: None,
                statistic: field
                    .statistic()
                    .map(|statistic| statistic.key().to_string()),
                child: true,
                shift: 0,
            },
        })
    }

    pub fn metric(
        &self,
        set: &ObjectSet,
        scene: &Scene,
        keys: &[String],
    ) -> Result<ObjectColor, ProtocolError> {
        if self.child {
            let target = self
                .target
                .as_deref()
                .expect("a checked label names a child set");
            let key = self
                .key
                .as_deref()
                .expect("a checked label names a child measurement");
            let child = scene
                .objects
                .iter()
                .find(|candidate| candidate.key().as_str() == target)
                .ok_or_else(|| {
                    ProtocolError::Invalid(format!(
                        "the label names the child set {target}, which is not an object set of the scene"
                    ))
                })?;
            let field = ChildField::parse(key, self.statistic.as_deref(), keys)
                .filter(|field| child.child_fields().contains(field))
                .ok_or_else(|| {
                    ProtocolError::Invalid(format!(
                        "the child set {} of {} measures no {key}",
                        child.name(),
                        set.name()
                    ))
                })?;
            if child.relation(set.id(), 0).is_none() {
                return Err(ProtocolError::Invalid(format!(
                    "the child set {} does not relate to {}",
                    child.name(),
                    set.name()
                )));
            }
            return Ok(ObjectColor::Child {
                target: child.id(),
                field,
            });
        }
        match (&self.key, &self.target) {
            (Some(key), None) if key == grana_measure::SCORE_KEY => {
                if set.classifier().is_none() {
                    return Err(ProtocolError::Invalid(format!(
                        "the set {} names a classifier score before it classifies",
                        set.name()
                    )));
                }
                Ok(ObjectColor::Score)
            }
            (Some(key), None) => {
                let measurement = set
                    .colorable_measurements()
                    .into_iter()
                    .find(|measurement| measurement.key(keys) == *key)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the set {} labels an object without a colorable measurement {key}",
                            set.name()
                        ))
                    })?;
                Ok(ObjectColor::Measurement(measurement))
            }
            (None, Some(target)) => {
                let target = scene
                    .objects
                    .iter()
                    .find(|set| set.key().as_str() == target)
                    .map(ObjectSet::id)
                    .ok_or_else(|| {
                        ProtocolError::Invalid(format!(
                            "the label names the relation target {target}, which is not an object set of the scene"
                        ))
                    })?;
                Ok(ObjectColor::Relation {
                    target,
                    field: self
                        .field()
                        .expect("a checked label names a relation field"),
                    shift: self.shift,
                })
            }
            _ => unreachable!("a checked label names one measurement or relation"),
        }
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
                "the label of {what} {detail}"
            )))
        };
        if self.child {
            let Some(target) = &self.target else {
                return invalid("names no child set".to_string());
            };
            if target.trim().is_empty() {
                return invalid("names an empty child set".to_string());
            }
            let Some(key) = &self.key else {
                return invalid("names no child measurement".to_string());
            };
            if key.trim().is_empty() {
                return invalid("names an empty child measurement".to_string());
            }
            if self.field.is_some() || self.shift != 0 {
                return invalid("names a relation of a child".to_string());
            }
            if key == "count" {
                if self.statistic.is_some() {
                    return invalid("reduces a child count".to_string());
                }
            } else if let Some(statistic) = &self.statistic
                && !Statistic::ALL
                    .into_iter()
                    .any(|candidate| candidate.key() == statistic)
            {
                return invalid("names an unknown child reduction".to_string());
            } else if self.statistic.is_none() {
                return invalid("names a child measurement without a reduction".to_string());
            }
            return Ok(());
        }
        match (&self.key, &self.target) {
            (Some(key), None) => {
                if key.trim().is_empty() {
                    return invalid("names an empty measurement".to_string());
                }
                if self.field.is_some() || self.shift != 0 {
                    return invalid("shifts a measurement that names no relation".to_string());
                }
                if self.statistic.is_some() {
                    return invalid("reduces a measurement without a child".to_string());
                }
            }
            (None, Some(target)) => {
                if target.trim().is_empty() {
                    return invalid("names an empty relation target".to_string());
                }
                if self.field().is_none() {
                    return invalid("names an unknown relation field".to_string());
                }
                if self.statistic.is_some() {
                    return invalid("reduces a relation without a child".to_string());
                }
            }
            _ => return invalid("names exactly one measurement or relation".to_string()),
        }
        Ok(())
    }
}

impl ViewSpec {
    pub fn of(view: &ViewState, time: u32) -> Self {
        Self {
            camera: CameraSpec::of(view.camera),
            time,
            timeline: Some(TimelineSpec::of(&view.timeline)),
            export: Some(ExportSpec::of(view.export)),
        }
    }

    pub fn camera(&self) -> CameraState {
        self.camera.camera()
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
        self.camera.checked()?;
        if let Some(timeline) = &self.timeline {
            for key in &timeline.keys {
                key.camera.checked()?;
            }
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

impl NamedViewSpec {
    pub fn of(name: impl Into<String>, view: ViewSpec) -> Self {
        let name = name.into();
        Self {
            name: name.trim().to_string(),
            view,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn view(&self) -> &ViewSpec {
        &self.view
    }

    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        if self.name.trim().is_empty() {
            return Err(ProtocolError::Invalid(
                "a named view carries a name".to_string(),
            ));
        }
        self.view.checked()
    }
}

impl SnapshotSpec {
    pub const WIDTH: u32 = OutputSize::DEFAULT_WIDTH;
    pub const MIN: u32 = OutputSize::MIN;
    pub const MAX: u32 = OutputSize::MAX;

    pub fn of(export: Option<Export>, views: Vec<String>) -> Self {
        let (size, transparent) = match export {
            Some(export) if export.image >= Self::MIN => (
                OutputSize::of_width(export.image).clamped(),
                export.transparent,
            ),
            _ => (OutputSize::of_width(Self::WIDTH).clamped(), false),
        };
        Self {
            width: size.width(),
            height: size.height(),
            transparent,
            views,
            movies: false,
            sequences: false,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub fn views(&self) -> &[String] {
        &self.views
    }

    pub fn movies(&self) -> bool {
        self.movies
    }

    pub fn sequences(&self) -> bool {
        self.sequences
    }

    fn output(&self) -> OutputSize {
        OutputSize::of(self.width, self.height)
    }

    pub fn size_for(&self, export: Option<Export>) -> [u32; 2] {
        match export {
            Some(export) if (Self::MIN..=Self::MAX).contains(&export.image) => {
                self.output().scaled(export.image).clamped().to_array()
            }
            _ => self.size(),
        }
    }

    pub fn film_size_for(&self, export: Option<Export>) -> [u32; 2] {
        match export {
            Some(export) if (Self::MIN..=Self::MAX).contains(&export.movie) => self
                .output()
                .scaled(export.movie)
                .clamped()
                .even()
                .to_array(),
            _ => self.size(),
        }
    }

    pub(super) fn checked(&self) -> Result<(), ProtocolError> {
        if !(Self::MIN..=Self::MAX).contains(&self.width)
            || !(Self::MIN..=Self::MAX).contains(&self.height)
        {
            return Err(ProtocolError::Invalid(format!(
                "a snapshot covers {}-{} pixels on each side",
                Self::MIN,
                Self::MAX
            )));
        }
        for (index, view) in self.views.iter().enumerate() {
            if view.trim().is_empty() {
                return Err(ProtocolError::Invalid(
                    "a snapshot names a non empty view".to_string(),
                ));
            }
            if self.views[..index].iter().any(|other| other == view) {
                return Err(ProtocolError::Invalid(format!(
                    "the snapshot renders the view {view} twice"
                )));
            }
        }
        Ok(())
    }
}

impl ExportSpec {
    pub fn of(export: Export) -> Self {
        Self {
            image: export.image,
            movie: export.movie,
            transparent: export.transparent,
        }
    }

    pub fn export(self) -> Export {
        Export::new(self.image, self.movie, self.transparent)
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
            projection: camera.projection.name().to_string(),
            aperture: camera.aperture,
            focus: camera.focus,
            slice: SliceSpec::of(camera.slice),
        }
    }

    pub fn camera(&self) -> CameraState {
        CameraState {
            pivot: self.pivot,
            yaw: self.yaw,
            pitch: self.pitch,
            distance: self.distance,
            fov_y: self.fov_y,
            projection: self.mode().expect("a checked camera names a projection"),
            aperture: self.aperture,
            focus: self.focus,
            slice: self.slice.and_then(SliceSpec::slice),
        }
    }

    pub fn mode(&self) -> Option<grana_display::CameraProjection> {
        grana_display::CameraProjection::from_name(&self.projection)
    }

    fn checked(&self) -> Result<(), ProtocolError> {
        if !self.pivot.iter().all(|value| value.is_finite())
            || !self.yaw.is_finite()
            || !self.pitch.is_finite()
            || !self.distance.is_finite()
            || self.distance <= 0.0
            || !self.fov_y.is_finite()
            || self.fov_y <= 1.0f32.to_radians()
            || self.fov_y >= 179.0f32.to_radians()
            || !self.aperture.is_finite()
            || !(0.0..=grana_display::APERTURE_LIMIT).contains(&self.aperture)
            || !self.focus.is_finite()
            || self.focus <= 0.0
            || self.focus > grana_display::APERTURE_LIMIT
            || self.mode().is_none()
            || self.slice.is_some_and(|slice| slice.slice().is_none())
            || (self.slice.is_some()
                && self.mode() != Some(grana_display::CameraProjection::Orthographic))
        {
            return Err(ProtocolError::Invalid(
                "a view describes a plausible camera".to_string(),
            ));
        }
        Ok(())
    }
}

impl SliceSpec {
    pub fn of(slice: Option<grana_display::Slice>) -> Option<Self> {
        slice.map(|slice| Self {
            axis: slice.axis() as u8,
            right: slice.right().to_array(),
            up: slice.up().to_array(),
            step: slice.step(),
        })
    }

    pub fn slice(self) -> Option<grana_display::Slice> {
        grana_display::Slice::restore(self.axis as usize, self.right, self.up, self.step)
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
        if let Some(volume) = &self.volume {
            volume.checked(&self.name)?;
        }
        Ok(())
    }
}

impl VolumeSpec {
    pub fn of(style: VolumeStyle) -> Self {
        Self {
            mode: style.mode().name().to_string(),
            isovalue: style.isovalue(),
        }
    }

    pub fn style(&self) -> VolumeStyle {
        let mode = VolumeMode::from_name(&self.mode).expect("the volume mode was checked");
        VolumeStyle::new(mode, self.isovalue)
    }

    fn checked(&self, name: &str) -> Result<(), ProtocolError> {
        if VolumeMode::from_name(&self.mode).is_none() {
            return Err(ProtocolError::Invalid(format!(
                "the channel {name} names an unknown volume mode {}",
                self.mode
            )));
        }
        if !self.isovalue.is_finite() || !(0.0..=1.0).contains(&self.isovalue) {
            return Err(ProtocolError::Invalid(format!(
                "the isovalue of {name} lies in the unit interval"
            )));
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
    pub fn of(shading: Shading, exposure: f32, step_scale: f32, clip: Clip) -> Self {
        Self {
            exposure,
            step_scale,
            shading: ShadingSpec {
                ambient: shading.ambient,
                diffuse: shading.diffuse,
                shadows: shading.shadows,
                azimuth: shading.azimuth,
                elevation: shading.elevation,
            },
            clip: ClipSpec::of(clip),
        }
    }

    pub fn clip(self) -> Clip {
        self.clip
            .and_then(ClipSpec::clip)
            .unwrap_or_else(Clip::none)
    }

    pub fn shading(self) -> Shading {
        Shading {
            ambient: self.shading.ambient,
            diffuse: self.shading.diffuse,
            shadows: self.shading.shadows,
            azimuth: self.shading.azimuth,
            elevation: self.shading.elevation,
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
        for value in [shading.ambient, shading.diffuse] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return invalid("carries unit lighting weights".to_string());
            }
        }
        if !shading.shadows.is_finite() || !(0.0..=1.0).contains(&shading.shadows) {
            return invalid("carries a unit shadow strength".to_string());
        }
        if !shading.azimuth.is_finite() || shading.azimuth.abs() > grana_display::AZIMUTH_LIMIT {
            return invalid("stands its key light at a finite azimuth".to_string());
        }
        if !shading.elevation.is_finite()
            || shading.elevation.abs() > grana_display::ELEVATION_LIMIT
        {
            return invalid("stands its key light between the poles".to_string());
        }
        if let Some(clip) = self.clip
            && clip.clip().is_none()
        {
            return invalid("clips along a unit plane at a finite depth".to_string());
        }
        Ok(())
    }
}

impl ClipSpec {
    pub fn of(clip: Clip) -> Option<Self> {
        let (normal, offset) = clip.parts();
        clip.enabled().then_some(Self { normal, offset })
    }

    pub fn clip(self) -> Option<Clip> {
        Clip::restore(self.normal, self.offset)
    }
}

impl RegionSpec {
    pub fn of(region: grana_volume::Region, world: Vec3) -> Option<Self> {
        if !region.enabled() {
            return None;
        }
        let half = 0.5 * world;
        let center = region.center();
        Some(Self {
            center: [0, 1, 2].map(|index| (center[index] + half[index]) / world[index]),
            axes: [0, 1, 2].map(|index| region.axis(index).to_array()),
            half: region.half().to_array(),
        })
    }

    pub fn region(self, world: Vec3) -> grana_volume::Region {
        let half = 0.5 * world;
        let center = Vec3::new(
            self.center[0] * world.x - half.x,
            self.center[1] * world.y - half.y,
            self.center[2] * world.z - half.z,
        );
        grana_volume::Region::checked(center.to_array(), self.axes, self.half)
            .expect("a checked region spec describes an orthonormal box")
    }

    pub fn size(self) -> Vec3 {
        Vec3::from(self.half) * 2.0
    }

    pub(super) fn checked(self) -> Result<(), ProtocolError> {
        if self.center.iter().any(|value| !value.is_finite())
            || self.center.iter().any(|value| !(0.0..=1.0).contains(value))
        {
            return Err(ProtocolError::Invalid(
                "a region center lies inside the dataset".to_string(),
            ));
        }
        if self.half.iter().any(|value| !value.is_finite()) || self.half.iter().any(|&v| v <= 0.0) {
            return Err(ProtocolError::Invalid(
                "a region box holds a positive extent on every axis".to_string(),
            ));
        }
        if grana_volume::Region::checked(self.center, self.axes, self.half).is_none() {
            return Err(ProtocolError::Invalid(
                "a region box carries orthonormal axes".to_string(),
            ));
        }
        Ok(())
    }
}

impl MaterialSpec {
    pub fn of(material: Material) -> Option<Self> {
        if material == Material::DEFAULT {
            return None;
        }
        Some(Self {
            roughness: material.roughness(),
            metallic: material.metallic(),
        })
    }

    pub fn material(self) -> Material {
        Material::new(self.roughness, self.metallic)
    }

    pub(super) fn checked(self, name: &str) -> Result<(), ProtocolError> {
        for value in [self.roughness, self.metallic] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(ProtocolError::Invalid(format!(
                    "{name} carries unit material weights"
                )));
            }
        }
        Ok(())
    }
}

impl SetSpec {
    pub fn mask(&self) -> Option<&str> {
        self.mask.as_deref()
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn track_params(&self) -> Option<TrackParams> {
        self.track.map(|track| {
            TrackParams::new(
                track.max_step,
                track.max_gap,
                grana_object::TrackEvents::new(
                    track.division,
                    track.division_range,
                    track.merge,
                    track.merge_range,
                    track.merge_overlap,
                ),
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

    pub fn material(&self) -> Material {
        self.material
            .map_or(Material::DEFAULT, MaterialSpec::material)
    }

    pub fn group(&self) -> Group {
        self.measure
            .as_ref()
            .map_or(Group::Object, MeasureSpec::group)
    }

    pub fn aggregate(&self) -> Aggregate {
        self.measure
            .as_ref()
            .map_or(Aggregate::MEAN, MeasureSpec::aggregate)
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

    pub fn compare(&self) -> &[String] {
        &self.compare
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
        if group == Group::Object && aggregate != Aggregate::MEAN {
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
            lasso.checked(set)?;
        }
        for filter in &self.filters {
            filter.checked(set)?;
        }
        for (index, column) in self.columns.iter().enumerate() {
            if column.trim().is_empty() {
                return invalid("shows an unnamed column".to_string());
            }
            if self.columns[..index].contains(column) {
                return invalid(format!("shows the column {column} twice"));
            }
        }
        for (index, column) in self.compare.iter().enumerate() {
            if column.trim().is_empty() {
                return invalid("compares an unnamed column".to_string());
            }
            if self.compare[..index].contains(column) {
                return invalid(format!("compares the column {column} twice"));
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

impl SourceSpec {
    pub fn kind(&self) -> Option<ObjectKind> {
        match self {
            Self::Spots { .. } | Self::External { .. } | Self::Localizations { .. } => {
                Some(ObjectKind::Spots)
            }
            Self::Surfaces { .. } | Self::Segments { .. } | Self::Labels { .. } => {
                Some(ObjectKind::Surfaces)
            }
            Self::Filaments { .. } => Some(ObjectKind::Filaments),
            Self::Cells { .. } => Some(ObjectKind::Cells),
            Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn channel(&self) -> Option<&str> {
        match self {
            Self::Spots { channel, .. }
            | Self::Surfaces { channel, .. }
            | Self::Filaments { channel, .. }
            | Self::Cells { channel, .. }
            | Self::Segments { channel, .. } => Some(channel),
            Self::Labels { .. }
            | Self::External { .. }
            | Self::Localizations { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Labels { label } => Some(label),
            Self::Spots { .. }
            | Self::External { .. }
            | Self::Localizations { .. }
            | Self::Surfaces { .. }
            | Self::Filaments { .. }
            | Self::Cells { .. }
            | Self::Segments { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn file(&self) -> Option<&str> {
        match self {
            Self::External { file } | Self::Localizations { file, .. } => Some(file),
            Self::Spots { .. }
            | Self::Surfaces { .. }
            | Self::Filaments { .. }
            | Self::Cells { .. }
            | Self::Segments { .. }
            | Self::Labels { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn molecules(&self) -> Option<&MoleculesSpec> {
        match self {
            Self::Localizations { molecules, .. } => Some(molecules),
            _ => None,
        }
    }

    pub fn params(&self) -> Option<ObjectParams> {
        Some(match *self {
            Self::Spots {
                diameter, contrast, ..
            } => ObjectParams::Spots(SpotParams::new(diameter, contrast)),
            Self::Surfaces {
                threshold,
                grain,
                split,
                min_volume,
                opening,
                closing,
                min_sphericity,
                min_mean_intensity,
                ..
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
                ..
            } => ObjectParams::Filaments(FilamentParams::new(threshold, min_length)),
            Self::Cells { reach, grain, .. } => ObjectParams::Cells(CellParams::new(reach, grain)),
            Self::Labels { .. }
            | Self::Segments { .. }
            | Self::External { .. }
            | Self::Localizations { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => {
                return None;
            }
        })
    }

    pub fn segment(&self) -> Option<SegmentParams> {
        match self {
            Self::Segments {
                cellprob,
                flow,
                minimum_size,
                tile_overlap,
                slice_overlap,
                model,
                ..
            } => grana_scene::SegmentModel::from_name(model).map(|model| {
                SegmentParams::with_settings(
                    *cellprob,
                    *flow,
                    *minimum_size,
                    *tile_overlap,
                    *slice_overlap,
                )
                .with_model(model)
            }),
            _ => None,
        }
    }

    pub fn seeds(&self) -> &[Vec<[f32; 3]>] {
        match self {
            Self::Surfaces { seeds, .. } | Self::Segments { seeds, .. } => seeds,
            Self::Spots { .. }
            | Self::Filaments { .. }
            | Self::Cells { .. }
            | Self::Labels { .. }
            | Self::External { .. }
            | Self::Localizations { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => &[],
        }
    }

    pub fn seed_source(&self) -> Option<&str> {
        match self {
            Self::Surfaces { seed_source, .. } | Self::Segments { seed_source, .. } => {
                seed_source.as_deref()
            }
            Self::Cells { seed_source, .. } => Some(seed_source),
            Self::Spots { .. }
            | Self::Filaments { .. }
            | Self::Labels { .. }
            | Self::External { .. }
            | Self::Localizations { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Edited { .. }
            | Self::Masked { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub(super) fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        let invalid =
            |detail: String| Err(ProtocolError::Invalid(format!("the set {set} {detail}")));
        match self {
            Self::Spots {
                channel,
                diameter,
                contrast,
            } => {
                validate(channel, &format!("the channel of {set}"))?;
                if !(diameter.is_finite() && *diameter > 0.0) {
                    return invalid("needs a positive spot diameter".to_string());
                }
                if !(0.0..=1.0).contains(contrast) {
                    return invalid("needs a unit contrast threshold".to_string());
                }
            }
            Self::Surfaces {
                channel,
                threshold,
                grain,
                split,
                min_volume,
                opening,
                closing,
                min_sphericity,
                min_mean_intensity,
                seeds,
                seed_source,
            } => {
                validate(channel, &format!("the channel of {set}"))?;
                if !(0.0..=1.0).contains(threshold) {
                    return invalid("needs a unit surface threshold".to_string());
                }
                if !(grain.is_finite() && *grain >= 0.0) {
                    return invalid("needs a non-negative surface grain".to_string());
                }
                if !(0.0..=1.0).contains(split) {
                    return invalid("needs a unit surface split".to_string());
                }
                if !(min_volume.is_finite() && *min_volume >= 0.0) {
                    return invalid("needs a non-negative minimum volume".to_string());
                }
                if !(opening.is_finite() && *opening >= 0.0) {
                    return invalid("needs a non-negative surface opening radius".to_string());
                }
                if !(closing.is_finite() && *closing >= 0.0) {
                    return invalid("needs a non-negative surface closing radius".to_string());
                }
                if !(0.0..=1.0).contains(min_sphericity) {
                    return invalid("needs a unit sphericity filter".to_string());
                }
                if !(0.0..=1.0).contains(min_mean_intensity) {
                    return invalid("needs a unit intensity filter".to_string());
                }
                if seeds
                    .iter()
                    .flatten()
                    .any(|point| point.iter().any(|value| !value.is_finite()))
                {
                    return invalid("seeds finite points".to_string());
                }
                if let Some(source) = seed_source {
                    validate(source, "a seed source")?;
                    if !seeds.is_empty() {
                        return invalid("seeds from one source or from points".to_string());
                    }
                    if source == set {
                        return invalid("seeds from itself".to_string());
                    }
                }
            }
            Self::Filaments {
                channel,
                threshold,
                min_length,
            } => {
                validate(channel, &format!("the channel of {set}"))?;
                if !(0.0..=1.0).contains(threshold) {
                    return invalid("needs a unit filament threshold".to_string());
                }
                if !(min_length.is_finite() && *min_length > 0.0) {
                    return invalid("needs a positive minimum length".to_string());
                }
            }
            Self::Cells {
                channel,
                reach,
                grain,
                seed_source,
            } => {
                validate(channel, &format!("the channel of {set}"))?;
                if !(reach.is_finite() && *reach > 0.0) {
                    return invalid("needs a positive reach".to_string());
                }
                if !(grain.is_finite() && *grain >= 0.0) {
                    return invalid("needs a non-negative cell grain".to_string());
                }
                validate(seed_source, "a cell nucleus source")?;
                if seed_source == set {
                    return invalid("grows from itself".to_string());
                }
            }
            Self::Segments {
                channel,
                cellprob,
                flow,
                minimum_size,
                tile_overlap,
                slice_overlap,
                model,
                seeds,
                seed_source,
            } => {
                validate(channel, &format!("the channel of {set}"))?;
                if !(cellprob.is_finite() && (-10.0..=10.0).contains(cellprob)) {
                    return invalid("needs a bounded cell probability threshold".to_string());
                }
                if !(flow.is_finite() && (0.0..=1.0).contains(flow)) {
                    return invalid("needs a unit flow error threshold".to_string());
                }
                if *minimum_size > 10_000 {
                    return invalid("needs a bounded minimum size".to_string());
                }
                if *tile_overlap > 192 {
                    return invalid("needs a bounded tile overlap".to_string());
                }
                if !(slice_overlap.is_finite() && (0.0..=1.0).contains(slice_overlap)) {
                    return invalid("needs a unit slice overlap".to_string());
                }
                let Some(model) = grana_scene::SegmentModel::from_name(model) else {
                    return invalid("names an unknown segmentation model".to_string());
                };
                if !model.licensed() {
                    return invalid(format!(
                        "names the {} model, which this build is not licensed to run",
                        model.label()
                    ));
                }
                if seeds
                    .iter()
                    .flatten()
                    .any(|point| point.iter().any(|value| !value.is_finite()))
                {
                    return invalid("seeds finite points".to_string());
                }
                if let Some(source) = seed_source {
                    validate(source, "a seed source")?;
                    if !seeds.is_empty() {
                        return invalid("seeds from one source or from points".to_string());
                    }
                    if source == set {
                        return invalid("seeds from itself".to_string());
                    }
                }
            }
            Self::Labels { label } => {
                validate(label, &format!("the label image of {set}"))?;
            }
            Self::External { file } => {
                validate(file, &format!("the object file of {set}"))?;
            }
            Self::Localizations { file, molecules } => {
                validate(file, &format!("the localization table of {set}"))?;
                molecules.checked(set)?;
            }
            Self::Filtered {
                source,
                group,
                filters,
                lasso,
            } => {
                validate(source, &format!("the source of {set}"))?;
                let group = Group::parse(group).ok_or_else(|| {
                    ProtocolError::Invalid(format!("the set {set} filters an unknown grouping"))
                })?;
                if group == Group::Timepoint {
                    return invalid("filters objects or tracks".to_string());
                }
                if filters.is_empty() && lasso.is_none() {
                    return invalid("filters at least one measurement".to_string());
                }
                for filter in filters {
                    filter.checked(set)?;
                }
                if let Some(lasso) = lasso {
                    lasso.checked(set)?;
                }
            }
            Self::Copy { source } => {
                validate(source, &format!("the source of {set}"))?;
            }
            Self::Edited { source, edits } => {
                validate(source, &format!("the source of {set}"))?;
                if edits.is_empty() {
                    return invalid("edits at least one surface".to_string());
                }
                for edit in edits {
                    edit.clone().edit().map_err(|detail| {
                        ProtocolError::Invalid(format!("the set {set} {detail}"))
                    })?;
                }
            }
            Self::Masked { source, edits } => {
                validate(source, &format!("the source of {set}"))?;
                if source == set {
                    return invalid("edits its own mask".to_string());
                }
                if edits.is_empty() {
                    return invalid("paints at least one mask stroke".to_string());
                }
                for edit in edits {
                    edit.clone().edit().map_err(|detail| {
                        ProtocolError::Invalid(format!("the set {set} {detail}"))
                    })?;
                }
            }
            Self::Merged { parts } => {
                if parts.is_empty() {
                    return invalid("merges at least one part".to_string());
                }
                for part in parts {
                    validate(&part.name, &format!("a merged part of {set}"))?;
                    part.source.checked(&part.name)?;
                }
            }
        }
        Ok(())
    }
}

impl FilterSpec {
    pub(super) fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        if self.column.trim().is_empty() {
            return Err(ProtocolError::Invalid(format!(
                "the set {set} filters an unnamed column"
            )));
        }
        if !(self.minimum.is_finite() && self.maximum.is_finite()) {
            return Err(ProtocolError::Invalid(format!(
                "the set {set} filters a range that is not finite"
            )));
        }
        if self.minimum > self.maximum {
            return Err(ProtocolError::Invalid(format!(
                "the set {set} filters a range that is not ordered"
            )));
        }
        Ok(())
    }
}

impl LassoSpec {
    pub(super) fn checked(&self, set: &str) -> Result<(), ProtocolError> {
        if self.x.trim().is_empty() || self.y.trim().is_empty() {
            return Err(ProtocolError::Invalid(format!(
                "the set {set} gates an unnamed column"
            )));
        }
        if self.x == self.y {
            return Err(ProtocolError::Invalid(format!(
                "the set {set} gates one column twice"
            )));
        }
        if self.points.len() < 3 || self.points.iter().flatten().any(|value| !value.is_finite()) {
            return Err(ProtocolError::Invalid(format!(
                "the set {set} gates a polygon"
            )));
        }
        Ok(())
    }
}

impl Protocol {
    const MAX_NESTING: usize = 16;

    pub fn kind_of(&self, key: &str) -> Option<ObjectKind> {
        self.kind_of_key(key, 0)
    }

    pub fn channel_of<'a>(&'a self, key: &str) -> Option<&'a str> {
        self.channel_of_key(key, 0)
    }

    pub fn kind_of_source(&self, source: &SourceSpec) -> Option<ObjectKind> {
        self.kind_of_source_at(source, 0)
    }

    fn kind_of_key(&self, key: &str, depth: usize) -> Option<ObjectKind> {
        if depth >= Self::MAX_NESTING {
            return None;
        }
        let spec = self.sets.iter().find(|set| set.key == key)?;
        self.kind_of_source_at(&spec.source, depth + 1)
    }

    fn kind_of_source_at(&self, source: &SourceSpec, depth: usize) -> Option<ObjectKind> {
        if depth >= Self::MAX_NESTING {
            return None;
        }
        match source {
            SourceSpec::Spots { .. }
            | SourceSpec::Surfaces { .. }
            | SourceSpec::Filaments { .. }
            | SourceSpec::Cells { .. }
            | SourceSpec::Segments { .. }
            | SourceSpec::Labels { .. }
            | SourceSpec::External { .. }
            | SourceSpec::Localizations { .. } => source.kind(),
            SourceSpec::Filtered { source, .. }
            | SourceSpec::Copy { source }
            | SourceSpec::Edited { source, .. }
            | SourceSpec::Masked { source, .. } => self.kind_of_key(source, depth + 1),
            SourceSpec::Merged { parts } => {
                self.kind_of_source_at(&parts.first()?.source, depth + 1)
            }
        }
    }

    fn channel_of_key<'a>(&'a self, key: &str, depth: usize) -> Option<&'a str> {
        if depth >= Self::MAX_NESTING {
            return None;
        }
        let spec = self.sets.iter().find(|set| set.key == key)?;
        self.channel_of_source_at(&spec.source, depth + 1)
    }

    fn channel_of_source_at<'a>(&'a self, source: &'a SourceSpec, depth: usize) -> Option<&'a str> {
        if depth >= Self::MAX_NESTING {
            return None;
        }
        match source {
            SourceSpec::Spots { .. }
            | SourceSpec::Surfaces { .. }
            | SourceSpec::Filaments { .. }
            | SourceSpec::Cells { .. }
            | SourceSpec::Segments { .. } => source.channel(),
            SourceSpec::Labels { .. }
            | SourceSpec::External { .. }
            | SourceSpec::Localizations { .. } => None,
            SourceSpec::Filtered { source, .. }
            | SourceSpec::Copy { source }
            | SourceSpec::Edited { source, .. }
            | SourceSpec::Masked { source, .. } => self.channel_of_key(source, depth + 1),
            SourceSpec::Merged { parts } => {
                self.channel_of_source_at(&parts.first()?.source, depth + 1)
            }
        }
    }

    pub(super) fn source_kind_consistent(&self, key: &str) -> bool {
        let Some(kind) = self.kind_of(key) else {
            return false;
        };
        self.sets
            .iter()
            .find(|spec| spec.key == key)
            .is_some_and(|spec| match &spec.source {
                SourceSpec::Merged { parts } => parts
                    .iter()
                    .all(|part| self.kind_of_source(&part.source) == Some(kind)),
                _ => true,
            })
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
        if !(self.merge_range.is_finite() && self.merge_range > 0.0) {
            return invalid("needs a positive merge range".to_string());
        }
        if !(0.0..=1.0).contains(&self.merge_overlap) {
            return invalid("needs a merge overlap within [0, 1]".to_string());
        }
        if !(self.thickness.is_finite() && self.thickness > 0.0) {
            return invalid("needs a positive tube thickness".to_string());
        }
        Ok(())
    }
}

impl CurationSpec {
    pub(super) fn checked(&self, set: &str, tracked: bool) -> Result<(), ProtocolError> {
        let invalid = |detail: String| {
            Err(ProtocolError::Invalid(format!(
                "the manual edits of {set} {detail}"
            )))
        };
        if self.removed.is_empty() && self.hidden.is_empty() && self.edits.is_empty() {
            return invalid("record no edit".to_string());
        }
        if !self.edits.is_empty() && !tracked {
            return invalid("edit tracks the set does not measure".to_string());
        }
        let removed = self
            .removed
            .iter()
            .map(|removal| (removal.frame, removal.anchors.as_slice()))
            .collect::<Vec<_>>();
        let hidden = self
            .hidden
            .iter()
            .map(|hidden| (hidden.frame, hidden.anchors.as_slice()))
            .collect::<Vec<_>>();
        for (entries, verb) in [(&removed, "remove"), (&hidden, "hide")] {
            if let Err(detail) = check_anchors(entries, verb) {
                return invalid(detail);
            }
        }
        for edit in &self.edits {
            let nodes = match edit {
                EditSpec::Cut { node } => vec![*node],
                EditSpec::Join { source, target } => {
                    if target.frame <= source.frame {
                        return invalid("join a later track that starts a lineage".to_string());
                    }
                    vec![*source, *target]
                }
                EditSpec::Divide {
                    parent,
                    first,
                    second,
                } => {
                    if first.frame <= parent.frame || second.frame <= parent.frame {
                        return invalid("divide a track into later daughter tracks".to_string());
                    }
                    if first.frame == second.frame && first.object == second.object {
                        return invalid("divide a track into two daughter tracks".to_string());
                    }
                    vec![*parent, *first, *second]
                }
                EditSpec::Merge {
                    first,
                    second,
                    child,
                } => {
                    if first.frame >= child.frame || second.frame >= child.frame {
                        return invalid("merge two earlier tracks into a later track".to_string());
                    }
                    if first.frame == second.frame && first.object == second.object {
                        return invalid("merge two parent tracks".to_string());
                    }
                    vec![*first, *second, *child]
                }
                EditSpec::Reparent { child, parent } => {
                    if child.frame <= parent.frame {
                        return invalid("parent a track on an earlier track".to_string());
                    }
                    vec![*child, *parent]
                }
                EditSpec::Detach { child } => vec![*child],
            };
            for node in nodes {
                if node.position.iter().any(|value| !value.is_finite()) {
                    return invalid(format!(
                        "name an object of the frame {} at a degenerate position",
                        node.frame
                    ));
                }
            }
        }
        Ok(())
    }
}

fn truth() -> bool {
    true
}

fn check_anchors(entries: &[(u32, &[AnchorSpec])], verb: &str) -> Result<(), String> {
    let mut previous = None;
    for (frame, anchors) in entries {
        if previous.is_some_and(|previous: u32| previous >= *frame) {
            return Err("are not ordered by frame".to_string());
        }
        previous = Some(*frame);
        if anchors.is_empty() {
            return Err(format!("{verb} no object from the frame {frame}"));
        }
        if anchors.iter().any(|anchor| {
            anchor.position.iter().any(|value| !value.is_finite())
                || (0..3).any(|axis| {
                    !(anchor.low[axis].is_finite() && anchor.high[axis].is_finite())
                        || anchor.low[axis] > anchor.high[axis]
                })
        }) {
            return Err(format!(
                "{verb} an object of the frame {frame} at a degenerate anchor"
            ));
        }
    }
    Ok(())
}

fn objects() -> String {
    "objects".to_string()
}

fn is_objects(value: &str) -> bool {
    value == "objects"
}

pub(super) fn validate(value: &str, what: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() {
        return Err(ProtocolError::Invalid(format!("{what} is empty")));
    }
    Ok(())
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

pub(crate) fn is_f32_zero(value: &f32) -> bool {
    *value == 0.0
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

fn is_u32_zero(value: &u32) -> bool {
    *value == 0
}

fn one() -> f32 {
    1.0
}

fn is_one(value: &f32) -> bool {
    *value == 1.0
}
