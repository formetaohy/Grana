use glam::Vec3;
use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, Shading, Stop, TransferFunction};
use grana_measure::{
    Aggregate, ChildField, Filter, Group, MeasureView, RelationField, Scope, Statistic, Table,
};
use grana_object::{FilamentParams, SpotParams, SurfaceFilter, SurfaceParams, TrackParams};
use grana_project::{Export, ViewState};
use grana_scene::{Annotation, ObjectColor, ObjectKind, ObjectParams, ObjectSet, Scene};
use grana_volume::{Axis, Clip};
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
pub struct ShadingSpec {
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
    pub shadows: f32,
    #[serde(default = "default_azimuth")]
    pub azimuth: f32,
    #[serde(default = "default_elevation")]
    pub elevation: f32,
}

fn default_azimuth() -> f32 {
    grana_display::DEFAULT_AZIMUTH
}

fn default_elevation() -> f32 {
    grana_display::DEFAULT_ELEVATION
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotSpec {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
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
        at: [f32; 3],
    },
    Distance {
        name: String,
        #[serde(default = "truth", skip_serializing_if = "is_true")]
        visible: bool,
        from: [f32; 3],
        to: [f32; 3],
    },
    Roi {
        name: String,
        #[serde(default = "truth", skip_serializing_if = "is_true")]
        visible: bool,
        center: [f32; 3],
        radius: f32,
    },
}

impl AnnotationSpec {
    pub fn of(annotation: &Annotation) -> Self {
        let name = annotation.name().to_string();
        let visible = annotation.visible();
        match annotation.kind().as_point() {
            Some(at) => Self::Point { name, visible, at },
            None => match (annotation.kind().end(), annotation.kind().radius()) {
                (Some(to), _) => Self::Distance {
                    name,
                    visible,
                    from: annotation.kind().anchor(),
                    to,
                },
                (None, Some(radius)) => Self::Roi {
                    name,
                    visible,
                    center: annotation.kind().anchor(),
                    radius,
                },
                (None, None) => unreachable!("an annotation names a point, a distance or a region"),
            },
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Point { name, .. } | Self::Distance { name, .. } | Self::Roi { name, .. } => name,
        }
    }

    pub fn visible(&self) -> bool {
        match self {
            Self::Point { visible, .. }
            | Self::Distance { visible, .. }
            | Self::Roi { visible, .. } => *visible,
        }
    }

    pub fn kind(&self) -> grana_scene::AnnotationKind {
        match *self {
            Self::Point { at, .. } => grana_scene::AnnotationKind::point(at),
            Self::Distance { from, to, .. } => grana_scene::AnnotationKind::distance(from, to),
            Self::Roi { center, radius, .. } => grana_scene::AnnotationKind::roi(center, radius),
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
            Self::Roi { center, radius, .. } => {
                if !finite(center) || !radius.is_finite() || radius <= 0.0 {
                    return Err(ProtocolError::Invalid(
                        "an annotation region grows by a positive radius".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSpec {
    pub low: [f32; 3],
    pub high: [f32; 3],
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
#[serde(deny_unknown_fields)]
pub struct DriftSpec {
    pub channel: String,
    #[serde(default, skip_serializing_if = "is_u32_zero")]
    pub reference: u32,
    pub offsets: Vec<[f32; 3]>,
}

impl DriftSpec {
    pub fn channel(&self) -> &str {
        &self.channel
    }

    pub fn reference(&self) -> u32 {
        self.reference
    }

    pub fn offsets(&self) -> &[[f32; 3]] {
        &self.offsets
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
    pub color_by: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_by: Option<ColorSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier: Option<ClassifierSpec>,
    pub source: SourceSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<TrackSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curation: Option<CurationSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measure: Option<MeasureSpec>,
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
    Labels {
        label: String,
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
    pub expected: Vec<Option<usize>>,
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
    Cut { node: NodeSpec },
    Join { source: NodeSpec, target: NodeSpec },
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
            expected: curation.expected().to_vec(),
            removed,
            hidden,
            edits,
        })
    }

    pub fn curation(&self) -> Result<grana_scene::Curation, String> {
        let mut removed = vec![Vec::new(); self.expected.len()];
        for spec in &self.removed {
            let Some(slot) = removed.get_mut(spec.frame as usize) else {
                return Err(format!(
                    "the protocol removes objects from the frame {} of {}",
                    spec.frame,
                    self.expected.len()
                ));
            };
            *slot = spec.anchors.iter().map(|anchor| anchor.anchor()).collect();
        }
        let mut hidden = vec![Vec::new(); self.expected.len()];
        for spec in &self.hidden {
            let Some(slot) = hidden.get_mut(spec.frame as usize) else {
                return Err(format!(
                    "the protocol hides objects of the frame {} of {}",
                    spec.frame,
                    self.expected.len()
                ));
            };
            *slot = spec.anchors.iter().map(|anchor| anchor.anchor()).collect();
        }
        let edits = self
            .edits
            .iter()
            .map(|edit| edit.edit())
            .collect::<Vec<_>>();
        grana_scene::Curation::restored(&self.expected, &removed, &hidden, &edits)
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
        }
    }
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

impl SnapshotSpec {
    pub const WIDTH: u32 = 1600;
    pub const MIN: u32 = 64;
    pub const MAX: u32 = 16384;

    pub fn of(export: Option<Export>) -> Self {
        let (width, transparent) = match export {
            Some(export) if export.width >= Self::MIN => (export.width, export.transparent),
            _ => (Self::WIDTH, false),
        };
        Self {
            width,
            height: (width * 5 / 8).max(Self::MIN),
            transparent,
        }
    }

    pub fn size(self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub(super) fn checked(self) -> Result<(), ProtocolError> {
        if !(Self::MIN..=Self::MAX).contains(&self.width)
            || !(Self::MIN..=Self::MAX).contains(&self.height)
        {
            return Err(ProtocolError::Invalid(format!(
                "a snapshot covers {}-{} pixels on each side",
                Self::MIN,
                Self::MAX
            )));
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
    pub fn of(shading: Shading, exposure: f32, step_scale: f32, clip: Clip) -> Self {
        Self {
            exposure,
            step_scale,
            shading: ShadingSpec {
                ambient: shading.ambient,
                diffuse: shading.diffuse,
                specular: shading.specular,
                shininess: shading.shininess,
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
            specular: self.shading.specular,
            shininess: self.shading.shininess,
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
            Self::Spots { .. } => Some(ObjectKind::Spots),
            Self::Surfaces { .. } | Self::Labels { .. } => Some(ObjectKind::Surfaces),
            Self::Filaments { .. } => Some(ObjectKind::Filaments),
            Self::Filtered { .. } | Self::Copy { .. } | Self::Merged { .. } => None,
        }
    }

    pub fn channel(&self) -> Option<&str> {
        match self {
            Self::Spots { channel, .. }
            | Self::Surfaces { channel, .. }
            | Self::Filaments { channel, .. } => Some(channel),
            Self::Labels { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Merged { .. } => None,
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Labels { label } => Some(label),
            Self::Spots { .. }
            | Self::Surfaces { .. }
            | Self::Filaments { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Merged { .. } => None,
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
            Self::Labels { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Merged { .. } => {
                return None;
            }
        })
    }

    pub fn seeds(&self) -> &[Vec<[f32; 3]>] {
        match self {
            Self::Surfaces { seeds, .. } => seeds,
            Self::Spots { .. }
            | Self::Filaments { .. }
            | Self::Labels { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
            | Self::Merged { .. } => &[],
        }
    }

    pub fn seed_source(&self) -> Option<&str> {
        match self {
            Self::Surfaces { seed_source, .. } => seed_source.as_deref(),
            Self::Spots { .. }
            | Self::Filaments { .. }
            | Self::Labels { .. }
            | Self::Filtered { .. }
            | Self::Copy { .. }
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
            Self::Labels { label } => {
                validate(label, &format!("the label image of {set}"))?;
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
            | SourceSpec::Labels { .. } => source.kind(),
            SourceSpec::Filtered { source, .. } | SourceSpec::Copy { source } => {
                self.kind_of_key(source, depth + 1)
            }
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
            | SourceSpec::Filaments { .. } => source.channel(),
            SourceSpec::Labels { .. } => None,
            SourceSpec::Filtered { source, .. } | SourceSpec::Copy { source } => {
                self.channel_of_key(source, depth + 1)
            }
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
        if self.expected.is_empty() {
            return invalid("cover no frame".to_string());
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
            if let Err(detail) = check_anchors(&self.expected, entries, verb) {
                return invalid(detail);
            }
        }
        for edit in &self.edits {
            let nodes = match edit {
                EditSpec::Cut { node } => [*node, *node],
                EditSpec::Join { source, target } => {
                    if source.frame != target.frame || source.object == target.object {
                        return invalid("join two objects of another frame".to_string());
                    }
                    [*source, *target]
                }
            };
            for node in nodes {
                if node.position.iter().any(|value| !value.is_finite()) {
                    return invalid(format!(
                        "name an object of the frame {} at a degenerate position",
                        node.frame
                    ));
                }
                match self.expected.get(node.frame as usize).copied().flatten() {
                    Some(count) if node.object >= count => {
                        return invalid(format!(
                            "name an object outside the frame {} of {count}",
                            node.frame
                        ));
                    }
                    Some(_) => {}
                    None => {
                        return invalid(format!(
                            "name an object of the frame {} without an object count",
                            node.frame
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn truth() -> bool {
    true
}

fn check_anchors(
    expected: &[Option<usize>],
    entries: &[(u32, &[AnchorSpec])],
    verb: &str,
) -> Result<(), String> {
    let mut previous = None;
    for (frame, anchors) in entries {
        if previous.is_some_and(|previous: u32| previous >= *frame) {
            return Err("are not ordered by frame".to_string());
        }
        previous = Some(*frame);
        if expected.get(*frame as usize).copied().flatten().is_none() {
            return Err(format!(
                "{verb} objects from the frame {frame} without an object count"
            ));
        }
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
