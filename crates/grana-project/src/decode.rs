use std::path::PathBuf;
use std::sync::Arc;

use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, Shading, Stop, TransferFunction};
use grana_geometry::{Bounds, Geometry, PackedMesh, SurfaceShape};
use grana_measure::{
    Aggregate, ChildField, ChildMetric, ClassifierState, Filter, Group, Lasso, MeasureView,
    Measurement, RelationField, Scope, Statistic, TrackMeasurement,
};
use grana_measure::{
    COLOC_BINS_MAX, COLOC_BINS_MIN, COLOC_BLOCK_MAX, COLOC_PERMUTATIONS_MAX, ColocCost, ColocFrame,
    ColocMetric, ColocMoments, ColocPair, ColocSeries, ColocSeriesInfo, ColocSettings,
    ColocSignificance, ColocThresholds, ObjectColoc, ObjectColocValue,
};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, MaskEdit, MaskMode,
    MaskTool, RelationFrame, RelationPair, SampleStatistics, Spot, SpotSet, SurfaceSet,
    SurfaceStatistics, SurfaceTopology, Track, TrackNode, TrackSet,
};
use grana_optics::{Kernel, MeasuredPsf, Psf, PsfParams};
use grana_scene::{
    Annotation, AnnotationKind, AnnotationState, BackgroundSpec, ChannelState, ColocResult,
    ColocState, ColorState, Criteria, Curation, DerivedSpec, Drift, FilterRange, ItemState,
    LabelExport, LassoPath, MaskFilter, MeasureState, ObjectAnchor, ObjectColor, ObjectFrame,
    ObjectKind, ObjectParams, ObjectSetId, ObjectSetKey, Operation, Provenance, RatioSpec, Recipe,
    RelationState, RestoreSpec, SceneState, Seeds, SelectionState, SetState, SmoothSpec,
    SurfaceEdit, SurfaceEditKind, TrackAnchor, TrackColor, TrackEdit, TrackStyle, Visibility,
};
use grana_volume::{Clip, Fingerprint, Intensity, Region, Spacing};

use crate::codec::{Decode, Reader};
use crate::encode::{TAG_FILAMENTS, TAG_SPOTS, TAG_SURFACES};
use crate::project::{Export, Project, Shape, Source, ViewState};

const FAULT_VALUE: &str = "the project data carries a value outside its domain";
const FAULT_SHAPE: &str = "the project data carries an inconsistent shape";
const FAULT_TAG: &str = "the project data carries an unknown variant tag";
const FAULT_SPILL: &str = "the project cannot store the geometry of a frame";
const MAX_PROVENANCE_DEPTH: usize = 32;

impl Decode for Project {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self {
            shape: Shape::decode(reader)?,
            source: Source::decode(reader)?,
            fingerprint: Fingerprint::decode(reader)?,
            scene: SceneState::decode(reader)?,
            measure: MeasureState::decode(reader)?,
            view: ViewState::decode(reader)?,
        })
    }
}

impl Decode for DerivedSpec {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let source = reader.u32()? as usize;
        let name = reader.string()?;
        if name.trim().is_empty() {
            return Err(FAULT_VALUE);
        }
        let mut tint = [0.0; 3];
        for value in &mut tint {
            *value = reader.f32()?;
            if !(0.0..=1.0).contains(value) {
                return Err(FAULT_VALUE);
            }
        }
        let min = reader.f32()?;
        let max = reader.f32()?;
        if !(min.is_finite() && max.is_finite() && min < max) {
            return Err(FAULT_VALUE);
        }
        let operation = match reader.u8()? {
            0 => Operation::Mask {
                set: ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?,
            },
            1 => Operation::Restore(RestoreSpec::new(psf(reader)?, iterations(reader)?)),
            2 => {
                let sigma = reader.f32()?;
                if !(sigma.is_finite() && sigma > 0.0) {
                    return Err(FAULT_VALUE);
                }
                Operation::Smooth(SmoothSpec::new(sigma))
            }
            3 => {
                let radius = reader.f32()?;
                if !(radius.is_finite() && radius > 0.0) {
                    return Err(FAULT_VALUE);
                }
                Operation::Background(BackgroundSpec::new(radius))
            }
            4 => Operation::Ratio(RatioSpec::new(reader.u32()? as usize)),
            _ => return Err(FAULT_TAG),
        };
        Ok(Self::new(
            source,
            name,
            tint,
            Intensity::new(min, max),
            operation,
        ))
    }
}

fn psf(reader: &mut Reader<'_>) -> Result<Psf, &'static str> {
    match reader.u8()? {
        0 => {
            let mut params = [0.0; 5];
            for value in &mut params {
                *value = reader.f32()?;
                if !value.is_finite() {
                    return Err(FAULT_VALUE);
                }
            }
            Ok(Psf::theoretical(PsfParams::new(
                params[0], params[1], params[2], params[3], params[4],
            )))
        }
        1 => {
            let mut measured = [0.0; 3];
            for value in &mut measured {
                *value = reader.f32()?;
                if !(value.is_finite() && *value > 0.0) {
                    return Err(FAULT_VALUE);
                }
            }
            let mut dims = [0u32; 3];
            for value in &mut dims {
                *value = reader.u32()?;
                if *value % 2 == 0 || !(1..=63).contains(value) {
                    return Err(FAULT_VALUE);
                }
            }
            let layers = reader.u32()?;
            if !(1..=16).contains(&layers) {
                return Err(FAULT_VALUE);
            }
            let count = (dims[0] as u64 * dims[1] as u64 * dims[2] as u64) as usize;
            if count == 0 || count > 1 << 22 {
                return Err(FAULT_VALUE);
            }
            let mut kernels = Vec::with_capacity(layers as usize);
            for _ in 0..layers {
                let mut values = vec![0.0f32; count];
                let mut total = 0.0f32;
                for value in &mut values {
                    *value = reader.f32()?;
                    if !(value.is_finite() && *value >= 0.0) {
                        return Err(FAULT_VALUE);
                    }
                    total += *value;
                }
                if !(total.is_finite() && total > 0.0) {
                    return Err(FAULT_VALUE);
                }
                kernels.push(Kernel::from_values(dims, values));
            }
            Ok(Psf::measured(MeasuredPsf::new(
                Spacing::new(measured[0], measured[1], measured[2]),
                kernels,
            )))
        }
        _ => Err(FAULT_TAG),
    }
}

fn iterations(reader: &mut Reader<'_>) -> Result<u32, &'static str> {
    let iterations = reader.u32()?;
    if !(1..=1000).contains(&iterations) {
        return Err(FAULT_VALUE);
    }
    Ok(iterations)
}

impl Decode for Fingerprint {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut bytes = [0u8; 32];
        for byte in &mut bytes {
            *byte = reader.u8()?;
        }
        Ok(Fingerprint::new(bytes))
    }
}

impl Decode for Shape {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut extent = [0; 3];
        for value in &mut extent {
            *value = reader.u32()?;
            if *value == 0 {
                return Err(FAULT_VALUE);
            }
        }
        let mut spacing = [0.0; 3];
        for value in &mut spacing {
            *value = positive(reader.f32()?)?;
        }
        Ok(Self { extent, spacing })
    }
}

impl Decode for Source {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(Self::Synthetic {
                name: reader.string()?,
            }),
            1 => Ok(Self::File {
                name: reader.string()?,
                path: PathBuf::from(reader.string()?),
                series: usize::try_from(reader.u32()?)
                    .map_err(|_| "the project names an impossible series")?,
            }),
            2 => Ok(Self::Remote {
                name: reader.string()?,
                url: reader.string()?,
                series: usize::try_from(reader.u32()?)
                    .map_err(|_| "the project names an impossible series")?,
            }),
            _ => Err("the project names an unknown dataset source"),
        }
    }
}

impl Decode for ViewState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let view = Self {
            camera: CameraState::decode(reader)?,
            section: SectionState::decode(reader)?,
            split: finite(reader.f32()?)?,
            timeline: Timeline::decode(reader)?,
            export: Export::decode(reader)?,
        };
        if !(grana_display::SPLIT_MIN..=grana_display::SPLIT_MAX).contains(&view.split)
            || !view.export.matches()
        {
            return Err(FAULT_VALUE);
        }
        Ok(view)
    }
}

impl Decode for Export {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self {
            width: reader.u32()?,
            transparent: reader.bool()?,
        })
    }
}

impl Decode for Timeline {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let fps = reader.u32()?;
        let duration = reader.f32()?;
        let keys = Vec::<Keyframe>::decode(reader)?;
        Timeline::from_parts(fps, duration, keys).map_err(|_| FAULT_VALUE)
    }
}

impl Decode for Keyframe {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self {
            at: finite(reader.f32()?)?,
            time: reader.u32()?,
            camera: CameraState::decode(reader)?,
            section: SectionState::decode(reader)?,
        })
    }
}

impl Decode for CameraState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut pivot = [0.0; 3];
        for value in &mut pivot {
            *value = finite(reader.f32()?)?;
        }
        let state = Self {
            pivot,
            yaw: finite(reader.f32()?)?,
            pitch: finite(reader.f32()?)?,
            distance: positive(reader.f32()?)?,
            fov_y: finite(reader.f32()?)?,
        };
        if state.fov_y <= 1.0f32.to_radians() || state.fov_y >= 179.0f32.to_radians() {
            return Err(FAULT_VALUE);
        }
        Ok(state)
    }
}

impl Decode for SectionState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut center = [0.0f32; 3];
        for value in &mut center {
            *value = finite(reader.f32()?)?;
        }
        let azimuth = finite(reader.f32()?)?;
        let elevation = finite(reader.f32()?)?;
        let span = positive(reader.f32()?)?;
        let projection =
            grana_display::Projection::from_index(reader.u8()? as usize).ok_or(FAULT_VALUE)?;
        let thickness = non_negative(reader.f32()?)?;
        if thickness > grana_display::THICKNESS_LIMIT {
            return Err(FAULT_VALUE);
        }
        let state = Self {
            center,
            azimuth,
            elevation,
            span,
            projection,
            thickness,
        };
        if state.elevation.abs() > std::f32::consts::FRAC_PI_2 {
            return Err(FAULT_VALUE);
        }
        Ok(state)
    }
}

impl Decode for SceneState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let state = Self {
            channels: Vec::<ChannelState>::decode(reader)?,
            derived: Vec::<Arc<DerivedSpec>>::decode(reader)?,
            selected: reader.count(1)?,
            shading: Shading::decode(reader)?,
            step_scale: positive(reader.f32()?)?,
            exposure: positive(reader.f32()?)?,
            region: Region::decode(reader)?,
            clip: Clip::decode(reader)?,
            mask: match reader.u64()? {
                0 => None,
                set => Some(set),
            },
            mask_radius: non_negative(reader.f32()?)?,
            mask_filter: decode_mask_filter(reader)?,
            time: reader.u32()?,
            selections: Vec::<SelectionState>::decode(reader)?,
            sets: Vec::<SetState>::decode(reader)?,
            coloc: ColocState::decode(reader)?,
            drift: Drift::decode(reader)?,
            annotations: AnnotationState::decode(reader)?,
        };
        if state.selections.len() > 1 && state.selections.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err(FAULT_SHAPE);
        }
        let channels = state.channels.len();
        let Some(base) = channels.checked_sub(state.derived.len()) else {
            return Err(FAULT_SHAPE);
        };
        let mut names = state
            .derived
            .iter()
            .map(|spec| spec.name())
            .collect::<Vec<_>>();
        names.sort_unstable();
        if names.windows(2).any(|pair| pair[0] == pair[1])
            || state.derived.iter().enumerate().any(|(index, spec)| {
                let channel = base + index;
                spec.source() >= channel
                    || spec.ratio_denominator().is_some_and(|denominator| {
                        denominator >= channel || denominator == spec.source()
                    })
                    || spec.mask().is_some_and(|set| {
                        !state.sets.iter().any(|candidate| candidate.id == set.raw())
                    })
            })
        {
            return Err(FAULT_SHAPE);
        }
        if state.selected >= channels {
            return Err(FAULT_VALUE);
        }
        let mut ids = state.sets.iter().map(|set| set.id).collect::<Vec<_>>();
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(FAULT_SHAPE);
        }
        let mut keys = state
            .sets
            .iter()
            .map(|set| set.key.as_str())
            .collect::<Vec<_>>();
        keys.sort_unstable();
        if keys.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(FAULT_SHAPE);
        }
        if state.sets.iter().any(|set| {
            set.channel >= channels
                || (!set.kind.trackable() && set.track_color.metric.is_some())
                || set
                    .object_color
                    .metric
                    .is_some_and(|metric| !color_applies(&state.sets, set, channels, metric))
                || set
                    .label
                    .is_some_and(|metric| !color_applies(&state.sets, set, channels, metric))
                || set
                    .track_color
                    .metric
                    .is_some_and(|metric| !track_color_applies(set, metric))
                || set.object_coloc.as_ref().is_some_and(|coloc| {
                    coloc.pair().a() >= channels
                        || coloc.pair().b() >= channels
                        || coloc.frames().len() != set.frames.len()
                        || coloc
                            .frames()
                            .iter()
                            .zip(&set.frames)
                            .any(|(values, frame)| {
                                values.len()
                                    != frame.as_deref().map_or(0, ObjectFrame::object_count)
                            })
                })
        }) {
            return Err(FAULT_SHAPE);
        }
        if !provenance_acyclic(&state.sets)
            || state.sets.iter().any(|set| {
                set.provenance.sources().iter().any(|source| {
                    !state
                        .sets
                        .iter()
                        .any(|candidate| candidate.id == source.raw())
                }) || set
                    .provenance
                    .parts()
                    .iter()
                    .any(|part| part.channel() >= channels)
                    || match &set.seeds {
                        Seeds::Points(_) => false,
                        Seeds::Source(source) => !state
                            .sets
                            .iter()
                            .any(|candidate| candidate.id == source.raw()),
                    }
            })
        {
            return Err(FAULT_SHAPE);
        }
        if state
            .coloc
            .pair
            .is_some_and(|pair| pair.a() >= channels || pair.b() >= channels)
            || state.coloc.results.iter().any(|result| {
                let pair = result.pair();
                pair.a() >= channels
                    || pair.b() >= channels
                    || result
                        .mask()
                        .is_some_and(|mask| !state.sets.iter().any(|set| set.id == mask))
            })
        {
            return Err(FAULT_SHAPE);
        }
        if let Some(filter) = state.mask_filter {
            let Some(mask) = state.mask else {
                return Err(FAULT_SHAPE);
            };
            let Some(set) = state.sets.iter().find(|set| set.id == mask) else {
                return Err(FAULT_SHAPE);
            };
            let related = set.relations.iter().any(|relation| {
                relation.target == filter.target().raw() && relation.shift == filter.shift()
            });
            if !related
                || !state
                    .sets
                    .iter()
                    .any(|candidate| candidate.id == filter.target().raw())
            {
                return Err(FAULT_SHAPE);
            }
        }
        if state.selections.iter().any(|selection| {
            let Some(set) = state.sets.iter().find(|set| set.id == selection.set) else {
                return true;
            };
            match selection.item {
                ItemState::Object(index) => !set.frames.iter().any(|frame| {
                    frame
                        .as_deref()
                        .is_some_and(|frame| index < frame.object_count())
                }),
                ItemState::Track(index) => {
                    !set.tracks.as_ref().is_some_and(|set| index < set.len())
                }
            }
        }) {
            return Err(FAULT_SHAPE);
        }
        Ok(state)
    }
}

impl Decode for AnnotationState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let entries = Vec::<Annotation>::decode(reader)?;
        let selected = decode_index(reader)?;
        if selected.is_some_and(|index| index >= entries.len()) {
            return Err(FAULT_SHAPE);
        }
        let mut counts = [0u32; 2];
        for count in &mut counts {
            *count = reader.u32()?;
        }
        Ok(Self {
            entries,
            selected,
            counts,
        })
    }
}

fn decode_index(reader: &mut Reader<'_>) -> Result<Option<usize>, &'static str> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.count(0)?)),
        _ => Err(FAULT_TAG),
    }
}

impl Decode for Annotation {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let name = reader.string()?;
        if name.trim().is_empty() {
            return Err(FAULT_VALUE);
        }
        let visible = reader.bool()?;
        let frame = decode_frame(reader)?;
        Ok(Self::new(name, frame, AnnotationKind::decode(reader)?).with_visible(visible))
    }
}

fn decode_frame(reader: &mut Reader<'_>) -> Result<Option<u32>, &'static str> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.u32()?)),
        _ => Err(FAULT_TAG),
    }
}

impl Decode for AnnotationKind {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(AnnotationKind::point(decode_point(reader)?)),
            1 => {
                let from = decode_point(reader)?;
                let to = decode_point(reader)?;
                if from == to {
                    return Err(FAULT_VALUE);
                }
                Ok(AnnotationKind::distance(from, to))
            }
            _ => Err(FAULT_TAG),
        }
    }
}

fn decode_point(reader: &mut Reader<'_>) -> Result<[f32; 3], &'static str> {
    let mut at = [0.0; 3];
    for value in &mut at {
        *value = finite(reader.f32()?)?;
    }
    Ok(at)
}

impl Decode for ChannelState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self {
            enabled: reader.bool()?,
            opacity: unit(reader.f32()?)?,
            window: Intensity::decode(reader)?,
            transfer: TransferFunction::decode(reader)?,
        })
    }
}

impl Decode for TransferFunction {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let stops = Vec::<Stop>::decode(reader)?;
        if stops.is_empty() {
            return Err(FAULT_SHAPE);
        }
        Ok(Self::new(stops))
    }
}

impl Decode for Stop {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let position = unit(reader.f32()?)?;
        let mut color = [0.0; 3];
        for value in &mut color {
            *value = unit(reader.f32()?)?;
        }
        Ok(Self::new(position, color, unit(reader.f32()?)?))
    }
}

impl Decode for Shading {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self {
            ambient: unit(reader.f32()?)?,
            diffuse: unit(reader.f32()?)?,
            specular: unit(reader.f32()?)?,
            shininess: positive(reader.f32()?)?,
            shadows: unit(reader.f32()?)?,
            azimuth: degrees(reader.f32()?, grana_display::AZIMUTH_LIMIT)?,
            elevation: degrees(reader.f32()?, grana_display::ELEVATION_LIMIT)?,
        })
    }
}

impl Decode for Intensity {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let minimum = finite(reader.f32()?)?;
        let maximum = finite(reader.f32()?)?;
        if minimum > maximum {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(minimum, maximum))
    }
}

impl Decode for Region {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut center = [0.0; 3];
        for value in &mut center {
            *value = finite(reader.f32()?)?;
        }
        let mut axes = [[0.0; 3]; 3];
        for axis in &mut axes {
            for value in axis {
                *value = finite(reader.f32()?)?;
            }
        }
        let mut half = [0.0; 3];
        for value in &mut half {
            *value = finite(reader.f32()?)?;
        }
        let mut region = Self::checked(center, axes, half).ok_or(FAULT_VALUE)?;
        region.set_enabled(reader.bool()?);
        Ok(region)
    }
}

impl Decode for Clip {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut normal = [0.0; 3];
        for value in &mut normal {
            *value = finite(reader.f32()?)?;
        }
        let offset = finite(reader.f32()?)?;
        Self::restore(normal, offset).ok_or(FAULT_VALUE)
    }
}

impl Decode for SelectionState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let set = reader.u64()?;
        if set == 0 {
            return Err(FAULT_VALUE);
        }
        Ok(Self {
            set,
            item: ItemState::decode(reader)?,
        })
    }
}

impl Decode for ItemState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(Self::Object(reader.count(1)?)),
            1 => Ok(Self::Track(reader.count(1)?)),
            _ => Err(FAULT_SHAPE),
        }
    }
}

impl Decode for SetState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let id = reader.u64()?;
        if id == 0 {
            return Err(FAULT_VALUE);
        }
        let kind = match reader.u8()? {
            0 => ObjectKind::Spots,
            1 => ObjectKind::Surfaces,
            2 => ObjectKind::Filaments,
            _ => return Err(FAULT_SHAPE),
        };
        let key = ObjectSetKey::parse(&reader.string()?).ok_or(FAULT_VALUE)?;
        let name = reader.string()?;
        if name.is_empty() {
            return Err(FAULT_VALUE);
        }
        let channel = reader.count(1)?;
        let mut color = [0.0; 3];
        for value in &mut color {
            *value = unit(reader.f32()?)?;
        }
        let visible = reader.bool()?;
        let labels = reader.bool()?;
        let label = Option::<ObjectColor>::decode(reader)?;
        let hidden = {
            let frames = reader.count(1)?;
            let mut hidden = Vec::with_capacity(frames.min(4096));
            for _ in 0..frames {
                let count = reader.count(1)?;
                let mut mask = Vec::with_capacity(count.min(1 << 20));
                for _ in 0..count {
                    mask.push(reader.bool()?);
                }
                hidden.push(mask);
            }
            hidden
        };
        let opacity = unit(reader.f32()?)?;
        let params = ObjectParams::decode(reader)?;
        if params.kind() != kind {
            return Err(FAULT_SHAPE);
        }
        let track_style = Option::<TrackStyle>::decode(reader)?;
        let tracks = Option::<Arc<TrackSet>>::decode(reader)?;
        let track_thickness = Option::<f32>::decode(reader)?.map(positive).transpose()?;
        let track_visible = Option::<bool>::decode(reader)?;
        let object_color = ColorState::<ObjectColor>::decode(reader)?;
        let track_color = ColorState::<TrackColor>::decode(reader)?;
        let classifier = Option::<ClassifierState>::decode(reader)?;
        let object_coloc = Option::<ObjectColoc>::decode(reader)?;
        let label_export = Option::<LabelExport>::decode(reader)?;
        let visibility = Visibility::decode(reader)?;
        let seeds = decode_seeds(reader)?;
        let frames = Vec::<Option<Arc<ObjectFrame>>>::decode(reader)?;
        match &seeds {
            Seeds::Points(seeded) => {
                if seeded.len() != frames.len()
                    || (kind != ObjectKind::Surfaces
                        && seeded.iter().any(|frame| !frame.is_empty()))
                {
                    return Err(FAULT_SHAPE);
                }
            }
            Seeds::Source(source) => {
                if kind != ObjectKind::Surfaces || source.raw() == id {
                    return Err(FAULT_SHAPE);
                }
            }
        }
        let state = Self {
            id,
            key,
            kind,
            name,
            channel,
            color,
            visible,
            labels,
            label,
            hidden,
            opacity,
            params,
            track_style,
            tracks,
            track_thickness,
            track_visible,
            object_color,
            track_color,
            classifier,
            object_coloc,
            label_export,
            visibility,
            seeds,
            frames,
            relations: Vec::<RelationState>::decode(reader)?,
            provenance: Provenance::decode(reader)?,
            curation: Curation::decode(reader)?,
            document: reader.u64()?,
        };
        if state.document == 0 {
            return Err(FAULT_VALUE);
        }
        if !state.curation.is_empty() && state.curation.expected().len() != state.frames.len() {
            return Err(FAULT_SHAPE);
        }
        if !state.hidden.is_empty()
            && (state.hidden.len() != state.frames.len()
                || state.hidden.iter().zip(&state.frames).any(|(mask, frame)| {
                    mask.len() != frame.as_deref().map_or(0, ObjectFrame::object_count)
                }))
        {
            return Err(FAULT_SHAPE);
        }
        if kind.trackable() != state.track_style.is_some()
            || state.tracks.is_some() != state.track_thickness.is_some()
            || state.tracks.is_some() != state.track_visible.is_some()
            || state
                .frames
                .iter()
                .flatten()
                .any(|frame| frame.kind() != kind)
        {
            return Err(FAULT_SHAPE);
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut keys = std::collections::BTreeSet::new();
        for relation in &state.relations {
            if !seen.insert((relation.target, relation.shift))
                || !keys.insert(relation.key.as_str())
                || relation.frames.len() != state.frames.len()
                || (relation.shift.unsigned_abs() as usize) >= state.frames.len()
            {
                return Err(FAULT_SHAPE);
            }
            for (frame, relation) in state.frames.iter().zip(&relation.frames) {
                let objects = frame.as_deref().map_or(0, ObjectFrame::object_count);
                if relation
                    .as_deref()
                    .is_some_and(|relation| relation.objects() != objects)
                {
                    return Err(FAULT_SHAPE);
                }
            }
        }
        if state
            .provenance
            .sources()
            .iter()
            .any(|source| source.raw() == id)
            || state.provenance.parts().iter().any(|part| {
                part.kind() != kind
                    || match part.seeds() {
                        Seeds::Points(frames) => {
                            (part.kind() == ObjectKind::Surfaces
                                && !frames.is_empty()
                                && frames.len() != state.frames.len())
                                || (part.kind() != ObjectKind::Surfaces
                                    && frames.iter().any(|frame| !frame.is_empty()))
                        }
                        Seeds::Source(_) => part.kind() != ObjectKind::Surfaces,
                    }
            })
        {
            return Err(FAULT_SHAPE);
        }
        Ok(state)
    }
}

impl Decode for Provenance {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        decode_provenance(reader, 0)
    }
}

impl Decode for Curation {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let frames = reader.count(2)?;
        let mut expected = Vec::with_capacity(frames);
        for _ in 0..frames {
            expected.push(match reader.u8()? {
                0 => None,
                1 => Some(reader.count(1)?),
                _ => return Err(FAULT_TAG),
            });
        }
        let removed_frames = reader.count(8)?;
        let mut removed = Vec::with_capacity(removed_frames);
        for _ in 0..removed_frames {
            removed.push(decode_anchors(reader)?);
        }
        let hidden_frames = reader.count(8)?;
        let mut hidden = Vec::with_capacity(hidden_frames);
        for _ in 0..hidden_frames {
            hidden.push(decode_anchors(reader)?);
        }
        let edits = reader.count(9)?;
        let mut edits_vec = Vec::with_capacity(edits);
        for _ in 0..edits {
            edits_vec.push(TrackEdit::decode(reader)?);
        }
        Curation::restored(&expected, &removed, &hidden, &edits_vec).map_err(|_| FAULT_VALUE)
    }
}

fn decode_anchors(reader: &mut Reader<'_>) -> Result<Vec<ObjectAnchor>, &'static str> {
    let objects = reader.count(8)?;
    let mut frame = Vec::with_capacity(objects);
    for _ in 0..objects {
        let position = [
            finite(reader.f32()?)?,
            finite(reader.f32()?)?,
            finite(reader.f32()?)?,
        ];
        let low = [
            finite(reader.f32()?)?,
            finite(reader.f32()?)?,
            finite(reader.f32()?)?,
        ];
        let high = [
            finite(reader.f32()?)?,
            finite(reader.f32()?)?,
            finite(reader.f32()?)?,
        ];
        if (0..3).any(|axis| low[axis] > high[axis]) {
            return Err(FAULT_SHAPE);
        }
        frame.push(ObjectAnchor::new(
            position,
            grana_geometry::Bounds::new(low, high),
        ));
    }
    Ok(frame)
}

impl Decode for TrackEdit {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let node = |reader: &mut Reader<'_>| -> Result<TrackAnchor, &'static str> {
            let time = reader.u32()?;
            let object = reader.count(1)?;
            let position = [
                finite(reader.f32()?)?,
                finite(reader.f32()?)?,
                finite(reader.f32()?)?,
            ];
            Ok(TrackAnchor::new(time, object, position))
        };
        Ok(match reader.u8()? {
            0 => Self::Cut(node(reader)?),
            1 => Self::Join {
                source: node(reader)?,
                target: node(reader)?,
            },
            _ => return Err(FAULT_TAG),
        })
    }
}

fn decode_provenance(reader: &mut Reader<'_>, depth: usize) -> Result<Provenance, &'static str> {
    if depth >= MAX_PROVENANCE_DEPTH {
        return Err(FAULT_SHAPE);
    }
    match reader.u8()? {
        0 => Ok(Provenance::Detection),
        1 => Ok(Provenance::Filtered {
            source: ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?,
            criteria: Criteria::decode(reader)?,
        }),
        2 => Ok(Provenance::Copied {
            source: ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?,
        }),
        7 => {
            let source = ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?;
            let count = reader.count(9)?;
            let mut edits = Vec::with_capacity(count);
            for _ in 0..count {
                let frame = reader.u32()?;
                let kind = SurfaceEditKind::parse(reader.u8()? as usize).ok_or(FAULT_TAG)?;
                let anchors = decode_anchors(reader)?;
                edits.push(SurfaceEdit::restored(frame, kind, anchors).map_err(|_| FAULT_VALUE)?);
            }
            if edits.is_empty() {
                return Err(FAULT_VALUE);
            }
            Ok(Provenance::Edited { source, edits })
        }
        8 => {
            let source = ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?;
            let count = reader.count(22)?;
            let mut strokes = Vec::with_capacity(count);
            for _ in 0..count {
                let frame = reader.u32()?;
                let anchors = decode_anchors(reader)?;
                if anchors.len() != 1 {
                    return Err(FAULT_VALUE);
                }
                let tool = MaskTool::parse(reader.u8()? as usize).ok_or(FAULT_TAG)?;
                let mode = MaskMode::parse(reader.u8()? as usize).ok_or(FAULT_TAG)?;
                let radius = finite(reader.f32()?)?;
                let tolerance = finite(reader.f32()?)?;
                let points = reader.count(12)?;
                let mut path = Vec::with_capacity(points);
                for _ in 0..points {
                    path.push([
                        finite(reader.f32()?)?,
                        finite(reader.f32()?)?,
                        finite(reader.f32()?)?,
                    ]);
                }
                let edit = match tool {
                    MaskTool::Brush => MaskEdit::brush(frame, anchors[0], mode, radius, path)
                        .map_err(|_| FAULT_VALUE)?,
                    MaskTool::Wand => {
                        let at = *path.first().ok_or(FAULT_VALUE)?;
                        MaskEdit::wand(frame, anchors[0], mode, tolerance, at)
                            .map_err(|_| FAULT_VALUE)?
                    }
                };
                strokes.push(edit);
            }
            if strokes.is_empty() {
                return Err(FAULT_VALUE);
            }
            Ok(Provenance::Masked { source, strokes })
        }
        3 => {
            let count = reader.count(1)?;
            let mut parts = Vec::new();
            for _ in 0..count {
                parts.push(decode_recipe(reader, depth + 1)?);
            }
            Ok(Provenance::Merged { parts })
        }
        4 => {
            let label = reader.string()?;
            if label.trim().is_empty() {
                return Err(FAULT_VALUE);
            }
            Ok(Provenance::Imported { label })
        }
        5 => {
            let file = reader.string()?;
            if file.trim().is_empty() {
                return Err(FAULT_VALUE);
            }
            Ok(Provenance::Interop { file })
        }
        6 => {
            let cellprob = reader.f32()?;
            let flow = reader.f32()?;
            let minimum = reader.count(1)?;
            let tile = reader.count(0)?;
            let slice = reader.f32()?;
            let model = reader.u8()?;
            if !(cellprob.is_finite() && (-10.0..=10.0).contains(&cellprob))
                || !(flow.is_finite() && (0.0..=1.0).contains(&flow))
                || minimum > 10_000
                || tile > 192
                || !(slice.is_finite() && (0.0..=1.0).contains(&slice))
            {
                return Err(FAULT_VALUE);
            }
            let model = grana_scene::SegmentModel::from_index(model).ok_or(FAULT_VALUE)?;
            Ok(Provenance::Segmented {
                params: grana_scene::SegmentParams::with_settings(
                    cellprob,
                    flow,
                    minimum as u32,
                    tile as u32,
                    slice,
                )
                .with_model(model),
            })
        }
        _ => Err(FAULT_TAG),
    }
}

fn decode_recipe(reader: &mut Reader<'_>, depth: usize) -> Result<Recipe, &'static str> {
    let name = reader.string()?;
    if name.trim().is_empty() {
        return Err(FAULT_VALUE);
    }
    let channel = reader.count(1)?;
    let params = ObjectParams::decode(reader)?;
    let seeds = decode_seeds(reader)?;
    match &seeds {
        Seeds::Points(frames) => {
            if params.kind() != ObjectKind::Surfaces && frames.iter().any(|frame| !frame.is_empty())
            {
                return Err(FAULT_SHAPE);
            }
        }
        Seeds::Source(_) => {
            if params.kind() != ObjectKind::Surfaces {
                return Err(FAULT_SHAPE);
            }
        }
    }
    Ok(Recipe::new(
        name,
        channel,
        params,
        seeds,
        Curation::decode(reader)?,
        decode_provenance(reader, depth)?,
    ))
}

impl Decode for Criteria {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let group = group(reader.u8()?)?;
        if group == Group::Timepoint {
            return Err(FAULT_VALUE);
        }
        let filters = Vec::<FilterRange>::decode(reader)?;
        let lasso = Option::<LassoPath>::decode(reader)?;
        if filters.is_empty() && lasso.is_none() {
            return Err(FAULT_VALUE);
        }
        Ok(Criteria::new(group, filters, lasso))
    }
}

impl Decode for FilterRange {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let key = reader.string()?;
        if key.trim().is_empty() {
            return Err(FAULT_VALUE);
        }
        let minimum = reader.f32()?;
        let maximum = reader.f32()?;
        if !(minimum.is_finite() && maximum.is_finite()) || minimum > maximum {
            return Err(FAULT_VALUE);
        }
        Ok(FilterRange::new(&key, minimum, maximum))
    }
}

impl Decode for LassoPath {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let x = reader.string()?;
        let y = reader.string()?;
        if x.trim().is_empty() || y.trim().is_empty() || x == y {
            return Err(FAULT_VALUE);
        }
        let count = reader.count(3)?;
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            let mut point = [0.0f32; 2];
            for value in &mut point {
                *value = finite(reader.f32()?)?;
            }
            points.push(point);
        }
        LassoPath::new(&x, &y, points).ok_or(FAULT_VALUE)
    }
}

impl Decode for LabelExport {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let path = reader.string()?;
        let frames = reader.u32()?;
        let labels = reader.u32()?;
        if path.trim().is_empty() || frames == 0 || labels == 0 {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(path, frames, labels))
    }
}

impl Decode for RelationState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let target = reader.u64()?;
        if target == 0 {
            return Err(FAULT_VALUE);
        }
        let shift = reader.i32()?;
        let key = reader.string()?;
        if key.trim().is_empty() {
            return Err(FAULT_VALUE);
        }
        Ok(Self {
            target,
            shift,
            key,
            frames: Vec::<Option<Arc<RelationFrame>>>::decode(reader)?,
        })
    }
}

impl Decode for RelationFrame {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let count = reader.count(16)?;
        let mut distance = Vec::with_capacity(count.min(1 << 16));
        let mut center = Vec::with_capacity(count.min(1 << 16));
        let mut inside = Vec::with_capacity(count.min(1 << 16));
        let mut contact = Vec::with_capacity(count.min(1 << 16));
        let mut overlapping = Vec::with_capacity(count.min(1 << 16));
        let mut partner = Vec::with_capacity(count.min(1 << 16));
        for _ in 0..count {
            let reach = reader.f32()?;
            if reach.is_nan() || reach < 0.0 {
                return Err(FAULT_VALUE);
            }
            distance.push(reach);
            let span = reader.f32()?;
            if span.is_nan() || span < 0.0 {
                return Err(FAULT_VALUE);
            }
            center.push(span);
            let within = reader.f32()?;
            if !(0.0..=1.0).contains(&within) {
                return Err(FAULT_VALUE);
            }
            inside.push(within);
            let area = reader.f32()?;
            if !area.is_finite() || area < 0.0 {
                return Err(FAULT_VALUE);
            }
            contact.push(area);
            overlapping.push(reader.u32()?);
            partner.push(reader.u32()?);
        }
        let pairs = reader.count(24)?;
        let mut rows: Vec<RelationPair> = Vec::with_capacity(pairs.min(1 << 16));
        for _ in 0..pairs {
            let source = reader.u32()?;
            let target = reader.u32()?;
            let contact = reader.f32()?;
            let distance = reader.f32()?;
            let inside = reader.f32()?;
            let center = reader.f32()?;
            if source as usize >= count
                || rows.last().is_some_and(|previous| {
                    (previous.source(), previous.target()) >= (source, target)
                })
                || !contact.is_finite()
                || contact < 0.0
                || !distance.is_finite()
                || distance < 0.0
                || !(0.0..=1.0).contains(&inside)
                || !center.is_finite()
                || center < 0.0
            {
                return Err(FAULT_VALUE);
            }
            rows.push(RelationPair::new(
                source, target, contact, distance, inside, center,
            ));
        }
        Ok(Self::new(
            distance,
            center,
            inside,
            contact,
            overlapping,
            partner,
            rows,
        ))
    }
}

impl Decode for ObjectParams {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let kind = match reader.u8()? {
            0 => ObjectKind::Spots,
            1 => ObjectKind::Surfaces,
            2 => ObjectKind::Filaments,
            _ => return Err(FAULT_SHAPE),
        };
        const FIELDS: usize = 8;
        let count = kind.fields().len();
        if count > FIELDS {
            return Err(FAULT_SHAPE);
        }
        let mut values = [0.0f32; FIELDS];
        for slot in values.iter_mut().take(count) {
            *slot = finite(reader.f32()?)?;
        }
        let unit = |value: f32| (0.0..=1.0).contains(&value);
        let length = |value: f32| value >= 0.0;
        match kind {
            ObjectKind::Spots if values[0] > 0.0 && values[1] >= 0.0 => Ok(Self::Spots(
                grana_object::SpotParams::new(values[0], values[1]),
            )),
            ObjectKind::Surfaces
                if unit(values[0])
                    && length(values[1])
                    && length(values[2])
                    && length(values[3])
                    && length(values[4])
                    && length(values[5])
                    && unit(values[6])
                    && unit(values[7]) =>
            {
                Ok(Self::Surfaces(grana_object::SurfaceParams::new(
                    values[0],
                    values[1],
                    values[2],
                    values[3],
                    grana_object::SurfaceFilter::new(values[4], values[5], values[6], values[7]),
                )))
            }
            ObjectKind::Filaments if unit(values[0]) && values[1] > 0.0 => Ok(Self::Filaments(
                grana_object::FilamentParams::new(values[0], values[1]),
            )),
            _ => Err(FAULT_VALUE),
        }
    }
}

impl Decode for TrackStyle {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let max_step = positive(reader.f32()?)?;
        let max_gap = reader.u32()?;
        let division = reader.bool()?;
        let division_range = positive(reader.f32()?)?;
        let thickness = positive(reader.f32()?)?;
        if division && division_range < max_step {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(
            grana_object::TrackParams::new(max_step, max_gap, division, division_range),
            thickness,
        ))
    }
}

impl Decode for TrackSet {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let frame_interval = positive(reader.f32()?)?;
        let tracks = Vec::<Track>::decode(reader)?;
        let mut children = vec![0usize; tracks.len()];
        for (index, track) in tracks.iter().enumerate() {
            if let Some(parent) = track.parent() {
                let parent = parent as usize;
                if parent >= index || tracks[parent].end() >= track.start() {
                    return Err(FAULT_SHAPE);
                }
                children[parent] += 1;
            }
        }
        if children.iter().any(|count| *count != 0 && *count != 2) {
            return Err(FAULT_SHAPE);
        }
        Ok(Self::new(frame_interval, tracks))
    }
}

impl Decode for Track {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let parent = Option::<u32>::decode(reader)?;
        let nodes = Vec::<TrackNode>::decode(reader)?;
        if nodes.is_empty() || !nodes.windows(2).all(|pair| pair[1].time() > pair[0].time()) {
            return Err(FAULT_SHAPE);
        }
        Ok(Self::new(parent, nodes))
    }
}

impl Decode for TrackNode {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let time = reader.u32()?;
        let object = reader.count(1)?;
        let mut centroid = [0.0; 3];
        for value in &mut centroid {
            *value = finite(reader.f32()?)?;
        }
        Ok(Self::new(time, object, centroid))
    }
}

impl Decode for Visibility {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(Self::All),
            1 => Ok(Self::Object(Arc::<Vec<Vec<bool>>>::decode(reader)?)),
            2 => Ok(Self::Track(Arc::<Vec<bool>>::decode(reader)?)),
            _ => Err(FAULT_SHAPE),
        }
    }
}

impl Decode for Palette {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Palette::from_index(reader.u8()?).ok_or(FAULT_SHAPE)
    }
}

impl<M: Decode> Decode for ColorState<M> {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let metric = Option::<M>::decode(reader)?;
        let palette = Palette::decode(reader)?;
        let reversed = reader.bool()?;
        let manual = match reader.bool()? {
            true => {
                let minimum = finite(reader.f32()?)?;
                let maximum = finite(reader.f32()?)?;
                if minimum > maximum {
                    return Err(FAULT_VALUE);
                }
                Some((minimum, maximum))
            }
            false => None,
        };
        Ok(Self {
            metric,
            palette,
            reversed,
            manual,
        })
    }
}

impl Decode for Measurement {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let measurement = match reader.u8()? {
            0 => Self::Position(reader.u8()? as usize),
            1 => Self::Diameter,
            2 => Self::Contrast,
            3 => Self::Volume,
            4 => Self::Area,
            5 => Self::Sphericity,
            6 => Self::Voxels,
            13 => Self::MaxDiameter,
            14 => Self::HullVolume,
            15 => Self::Ellipsoid(reader.u8()? as usize),
            7 => Self::Length,
            8 => Self::Branches,
            9 => Self::Terminals,
            10 => Self::Junctions,
            11 => Self::Nodes,
            12 => Self::Intensity {
                channel: reader.u32()? as usize,
                statistic: Statistic::from_index(reader.u8()?).ok_or(FAULT_SHAPE)?,
            },
            _ => return Err(FAULT_SHAPE),
        };
        if let Self::Position(axis) = measurement
            && axis >= 3
        {
            return Err(FAULT_VALUE);
        }
        if let Self::Ellipsoid(axis) = measurement
            && axis >= 3
        {
            return Err(FAULT_VALUE);
        }
        Ok(measurement)
    }
}

impl Decode for ObjectColor {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(Self::Measurement(Measurement::decode(reader)?)),
            1 => {
                let target = ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?;
                let field = RelationField::from_index(reader.u8()?).ok_or(FAULT_SHAPE)?;
                if !RelationField::CONTINUOUS.contains(&field) {
                    return Err(FAULT_VALUE);
                }
                let shift = reader.i32()?;
                Ok(Self::Relation {
                    target,
                    field,
                    shift,
                })
            }
            2 => {
                let target = ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?;
                Ok(Self::Child {
                    target,
                    field: ChildField::decode(reader)?,
                })
            }
            3 => Ok(Self::Score),
            4 => {
                let pair = ColocPair::decode(reader)?;
                let metric = ColocMetric::from_index(reader.u8()?).ok_or(FAULT_SHAPE)?;
                Ok(Self::Coloc { pair, metric })
            }
            _ => Err(FAULT_SHAPE),
        }
    }
}

impl Decode for ObjectColocValue {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let voxels = reader.u64()?;
        let mut sums = [0.0; 5];
        for sum in &mut sums {
            *sum = reader.f64()?;
        }
        if !sums.iter().all(|sum| sum.is_finite() && *sum >= 0.0) {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(voxels, sums))
    }
}

impl Decode for ObjectColoc {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let pair = ColocPair::decode(reader)?;
        let level = reader.count(0)?;
        let frames = reader.count(4)?;
        if frames > 0 {
            let mut values = Vec::with_capacity(frames.min(4096));
            for _ in 0..frames {
                let objects = reader.count(48)?;
                if objects > usize::from(u16::MAX) {
                    return Err(FAULT_SHAPE);
                }
                let mut frame = Vec::with_capacity(objects);
                for _ in 0..objects {
                    frame.push(ObjectColocValue::decode(reader)?);
                }
                values.push(frame);
            }
            return Ok(Self::new(pair, level, values));
        }
        Err(FAULT_SHAPE)
    }
}

impl Decode for ClassifierState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let state = Self {
            features: Vec::<Measurement>::decode(reader)?,
            mean: Vec::<f64>::decode(reader)?,
            scale: Vec::<f64>::decode(reader)?,
            weights: Vec::<f64>::decode(reader)?,
            bias: reader.f64()?,
            positives: reader.u32()? as usize,
            negatives: reader.u32()? as usize,
            accuracy: reader.f32()?,
        };
        grana_measure::Classifier::restore(state.clone()).map_err(|_| FAULT_VALUE)?;
        Ok(state)
    }
}

impl Decode for ChildField {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(Self::Count),
            1 => Ok(Self::Stat {
                metric: ChildMetric::decode(reader)?,
                statistic: Statistic::from_index(reader.u8()?).ok_or(FAULT_SHAPE)?,
            }),
            _ => Err(FAULT_SHAPE),
        }
    }
}

impl Decode for ChildMetric {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(match reader.u8()? {
            0 => Self::Diameter,
            1 => Self::Contrast,
            2 => Self::Volume,
            3 => Self::Area,
            4 => Self::Sphericity,
            5 => Self::Voxels,
            6 => Self::Length,
            7 => Self::Branches,
            8 => Self::Terminals,
            9 => Self::Junctions,
            10 => Self::Nodes,
            11 => Self::Intensity(reader.u32()? as usize),
            _ => return Err(FAULT_SHAPE),
        })
    }
}

impl Decode for TrackColor {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.u8()? {
            0 => Ok(Self::Measurement(TrackMeasurement::decode(reader)?)),
            1 => {
                let target = ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?;
                let field = RelationField::from_index(reader.u8()?).ok_or(FAULT_SHAPE)?;
                if !RelationField::CONTINUOUS.contains(&field) {
                    return Err(FAULT_VALUE);
                }
                let aggregate = Aggregate::from_index(reader.u8()?).ok_or(FAULT_SHAPE)?;
                let shift = reader.i32()?;
                Ok(Self::Relation {
                    target,
                    field,
                    aggregate,
                    shift,
                })
            }
            _ => Err(FAULT_SHAPE),
        }
    }
}

impl Decode for TrackMeasurement {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        TrackMeasurement::from_index(reader.u8()?).ok_or(FAULT_SHAPE)
    }
}

impl Decode for ObjectFrame {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.step();
        match reader.u8()? {
            TAG_SPOTS => Ok(Self::Spots(SpotSet::decode(reader)?)),
            TAG_SURFACES => Ok(Self::Surfaces(SurfaceSet::decode(reader)?)),
            TAG_FILAMENTS => Ok(Self::Filaments(FilamentSet::decode(reader)?)),
            _ => Err(FAULT_SHAPE),
        }
    }
}

impl Decode for SpotSet {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let level = reader.count(1)?;
        let spots = Vec::<Spot>::decode(reader)?;
        let intensities = IntensityTable::decode(reader)?;
        if intensities.objects() != spots.len() {
            return Err(FAULT_SHAPE);
        }
        Ok(Self::new(level, spots, intensities))
    }
}

impl Decode for Spot {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut center = [0.0; 3];
        for value in &mut center {
            *value = finite(reader.f32()?)?;
        }
        Ok(Self::new(
            center,
            positive(reader.f32()?)?,
            finite(reader.f32()?)?,
        ))
    }
}

impl Decode for SurfaceSet {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let level = reader.count(1)?;
        let objects = Vec::<SurfaceStatistics>::decode(reader)?;
        let store = Arc::clone(reader.store());
        let geometry =
            Geometry::paged(&store, PackedMesh::decode(reader)?).map_err(|_| FAULT_SPILL)?;
        let intensities = IntensityTable::decode(reader)?;
        let Some(set) =
            Self::from_geometry(level, geometry, objects, intensities).map_err(|_| FAULT_SPILL)?
        else {
            return Err(FAULT_SHAPE);
        };
        match reader.u8()? {
            0 => Ok(set),
            1 => {
                reader.step();
                let bytes = reader.blob()?;
                let topology = SurfaceTopology::from_bytes(&bytes).ok_or(FAULT_SHAPE)?;
                let basins = topology.basins().len() as u32;
                let groups = reader.count(size_of::<u32>())?;
                if groups != set.objects().len() {
                    return Err(FAULT_SHAPE);
                }
                let mut leaves = Vec::with_capacity(groups);
                for _ in 0..groups {
                    let count = reader.count(size_of::<u32>())?;
                    let mut held = Vec::with_capacity(count);
                    for _ in 0..count {
                        let basin = reader.u32()?;
                        if basin >= basins {
                            return Err(FAULT_VALUE);
                        }
                        held.push(basin);
                    }
                    leaves.push(held);
                }
                let removed = reader.count(size_of::<u32>())?;
                let mut pruned = Vec::with_capacity(removed);
                for _ in 0..removed {
                    let basin = reader.u32()?;
                    if basin >= basins || pruned.last().is_some_and(|last| *last >= basin) {
                        return Err(FAULT_VALUE);
                    }
                    pruned.push(basin);
                }
                let set = set.with_cut(leaves, pruned);
                let topology = Geometry::paged(&store, topology).map_err(|_| FAULT_SPILL)?;
                set.with_topology(topology).map_err(|_| FAULT_SPILL)
            }
            _ => Err(FAULT_TAG),
        }
    }
}

impl Decode for SurfaceStatistics {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let voxel_count = reader.u64()?;
        let volume = reader.f64()?;
        let area = reader.f64()?;
        let mut centroid = [0.0; 3];
        for value in &mut centroid {
            *value = finite(reader.f32()?)?;
        }
        if voxel_count == 0
            || !(volume.is_finite() && volume > 0.0)
            || !(area.is_finite() && area > 0.0)
        {
            return Err(FAULT_VALUE);
        }
        let bounds = Bounds::decode(reader)?;
        let shape = match reader.u8()? {
            0 => None,
            1 => {
                let mut axes = [0.0; 3];
                for axis in &mut axes {
                    *axis = reader.f64()?;
                }
                let diameter = reader.f64()?;
                let hull_volume = reader.f64()?;
                if axes.iter().any(|axis| !axis.is_finite() || *axis < 0.0)
                    || !(diameter.is_finite() && diameter >= 0.0)
                    || !(hull_volume.is_finite() && hull_volume >= 0.0)
                    || axes[0] < axes[1]
                    || axes[1] < axes[2]
                {
                    return Err(FAULT_VALUE);
                }
                Some(SurfaceShape::new(axes, diameter, hull_volume))
            }
            _ => return Err(FAULT_TAG),
        };
        Ok(Self::new(
            voxel_count,
            volume,
            area,
            centroid,
            bounds,
            shape,
        ))
    }
}

impl Decode for FilamentSet {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let level = reader.count(1)?;
        let objects = Vec::<FilamentStatistics>::decode(reader)?;
        let store = Arc::clone(reader.store());
        let geometry =
            Geometry::paged(&store, PackedMesh::decode(reader)?).map_err(|_| FAULT_SPILL)?;
        let intensities = IntensityTable::decode(reader)?;
        match Self::from_geometry(level, geometry, objects, intensities) {
            Ok(Some(set)) => Ok(set),
            Ok(None) => Err(FAULT_SHAPE),
            Err(_) => Err(FAULT_SPILL),
        }
    }
}

impl Decode for FilamentStatistics {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let length = reader.f64()?;
        let diameter = reader.f64()?;
        let volume = reader.f64()?;
        let topology = FilamentTopology::decode(reader)?;
        let mut centroid = [0.0; 3];
        for value in &mut centroid {
            *value = finite(reader.f32()?)?;
        }
        let bounds = Bounds::decode(reader)?;
        if !(length.is_finite() && length > 0.0)
            || !(diameter.is_finite() && diameter > 0.0)
            || !(volume.is_finite() && volume > 0.0)
        {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(
            length, diameter, volume, topology, centroid, bounds,
        ))
    }
}

impl Decode for FilamentTopology {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let branches = reader.u32()?;
        let terminals = reader.u32()?;
        let junctions = reader.u32()?;
        let nodes = reader.u32()?;
        if branches == 0 || nodes == 0 || terminals > 2 * branches || junctions >= branches {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(branches, terminals, junctions, nodes))
    }
}

impl Decode for Bounds {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut minimum = [0.0; 3];
        for value in &mut minimum {
            *value = finite(reader.f32()?)?;
        }
        let mut maximum = [0.0; 3];
        for value in &mut maximum {
            *value = finite(reader.f32()?)?;
        }
        if !(0..3).all(|axis| minimum[axis] <= maximum[axis]) {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(minimum, maximum))
    }
}

impl Decode for PackedMesh {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.step();
        let bytes = reader.blob()?;
        Self::from_bytes(&bytes).ok_or(FAULT_SHAPE)
    }
}

impl Decode for IntensityTable {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let channels = reader.u32()? as usize;
        let objects = reader.u32()? as usize;
        if channels == 0 {
            return Err(FAULT_VALUE);
        }
        let total = match channels.checked_mul(objects) {
            Some(total) if total.saturating_mul(24) <= reader.remaining() => total,
            _ => return Err(FAULT_SHAPE),
        };
        let mut values = Vec::with_capacity(total.min(1 << 16));
        for _ in 0..total {
            values.push(SampleStatistics::decode(reader)?);
        }
        Ok(Self::new(channels, values))
    }
}

impl Decode for SampleStatistics {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mean = finite(reader.f32()?)?;
        let minimum = finite(reader.f32()?)?;
        let maximum = finite(reader.f32()?)?;
        let deviation = finite(reader.f32()?)?;
        let sum = reader.f64()?;
        if minimum > mean || mean > maximum || deviation < 0.0 || !(sum.is_finite() && sum >= 0.0) {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(mean, minimum, maximum, deviation, sum))
    }
}

impl Decode for MeasureState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let set = Option::<u64>::decode(reader)?;
        if set == Some(0) {
            return Err(FAULT_VALUE);
        }
        let group = group(reader.u8()?)?;
        let aggregate = aggregate(reader.u8()?)?;
        let scope = Scope::from_index(reader.u8()?).ok_or(FAULT_TAG)?;
        let filters = Vec::<Filter>::decode(reader)?;
        let lasso = match reader.u8()? {
            0 => None,
            1 => {
                let x = reader.count(1)?;
                let y = reader.count(1)?;
                let count = reader.count(8)?;
                let mut points = Vec::with_capacity(count.min(1 << 16));
                for _ in 0..count {
                    let first = finite(reader.f32()?)?;
                    let second = finite(reader.f32()?)?;
                    points.push([first, second]);
                }
                Some(Lasso::new(x, y, points).ok_or(FAULT_VALUE)?)
            }
            _ => return Err(FAULT_SHAPE),
        };
        let sort = match reader.u8()? {
            0 => None,
            1 => Some((reader.count(1)?, reader.bool()?)),
            _ => return Err(FAULT_SHAPE),
        };
        let columns = Vec::<bool>::decode(reader)?;
        let visibility = reader.bool()?;
        let view = MeasureView::from_index(reader.u8()?).ok_or(FAULT_TAG)?;
        let x = reader.count(0)?;
        let y = reader.count(0)?;
        Ok(Self {
            set,
            group,
            aggregate,
            scope,
            filters,
            lasso,
            sort,
            columns,
            visibility,
            view,
            axes: [x, y],
        })
    }
}

impl Decode for Filter {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let column = reader.count(1)?;
        let minimum = finite(reader.f32()?)?;
        let maximum = finite(reader.f32()?)?;
        if minimum > maximum {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(column, minimum, maximum))
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let count = reader.count(1)?;
        let mut values = Vec::with_capacity(count.min(1 << 16));
        for _ in 0..count {
            values.push(T::decode(reader)?);
        }
        Ok(values)
    }
}

impl Decode for Drift {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let channel = reader.u32()? as usize;
        let reference = reader.u32()?;
        let frames = reader.count(1)?;
        let mut offsets = Vec::with_capacity(frames.min(1 << 16));
        for _ in 0..frames {
            let mut offset = [0f32; 3];
            for value in &mut offset {
                *value = finite(reader.f32()?)?;
                if value.abs() > Drift::LIMIT {
                    return Err(FAULT_VALUE);
                }
            }
            offsets.push(offset);
        }
        if offsets.is_empty() {
            if reference != 0 {
                return Err(FAULT_VALUE);
            }
            return Ok(Drift::default());
        }
        if reference as usize >= offsets.len() || offsets[reference as usize] != [0.0; 3] {
            return Err(FAULT_VALUE);
        }
        Ok(Drift::new(channel, reference, offsets))
    }
}

impl Decode for ColocState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let state = Self {
            pair: Option::<ColocPair>::decode(reader)?,
            results: Vec::<ColocResult>::decode(reader)?,
            settings: ColocSettings::decode(reader)?,
            document: reader.u64()?,
        };
        if state.document == 0 {
            return Err(FAULT_VALUE);
        }
        for (index, result) in state.results.iter().enumerate() {
            if state.results[index + 1..]
                .iter()
                .any(|other| other.pair() == result.pair())
            {
                return Err(FAULT_SHAPE);
            }
        }
        Ok(state)
    }
}

impl Decode for ColocResult {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self::from_parts(
            ColocThresholds::decode(reader)?,
            ColocSeries::decode(reader)?,
        ))
    }
}

impl Decode for ColocPair {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let a = reader.count(1)?;
        let b = reader.count(1)?;
        if a == b {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(a, b))
    }
}

impl Decode for ColocThresholds {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self::new(unit(reader.f32()?)?, unit(reader.f32()?)?))
    }
}

impl Decode for ColocSettings {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let permutations = reader.u32()?;
        let block = reader.u32()?;
        if !(1..=COLOC_PERMUTATIONS_MAX).contains(&permutations)
            || !(1..=COLOC_BLOCK_MAX).contains(&block)
        {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(permutations, block))
    }
}

impl Decode for ColocSeries {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let pair = ColocPair::decode(reader)?;
        let info = ColocSeriesInfo::decode(reader)?;
        let bins = reader.count(1)?;
        if !(COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&bins) {
            return Err(FAULT_VALUE);
        }
        let frames = Vec::<ColocFrame>::decode(reader)?;
        let joint = Vec::<u64>::decode(reader)?;
        let significance = Option::<ColocSignificance>::decode(reader)?;
        let cost = ColocCost::decode(reader)?;
        if frames.is_empty() || joint.len() != bins * bins {
            return Err(FAULT_SHAPE);
        }
        if frames.iter().any(|frame| frame.bins() != bins) {
            return Err(FAULT_SHAPE);
        }
        let voxels = frames
            .iter()
            .map(|frame| frame.moments().voxels())
            .sum::<u64>();
        if joint.iter().sum::<u64>() != voxels || cost.scan_voxels() != voxels {
            return Err(FAULT_SHAPE);
        }
        if let Some(significance) = significance
            && significance.randomizations() != cost.randomizations()
        {
            return Err(FAULT_SHAPE);
        }
        Ok(Self::from_parts(
            pair,
            info,
            bins,
            frames,
            joint,
            significance,
            cost,
        ))
    }
}

impl Decode for ColocSeriesInfo {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let level = reader.count(1)?;
        let permutation_level = reader.count(1)?;
        let permutation_stride = reader.u32()?;
        let mask = Option::<u64>::decode(reader)?;
        if permutation_level < level || permutation_stride == 0 {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(
            level,
            permutation_level,
            permutation_stride,
            mask,
        ))
    }
}

impl Decode for ColocSignificance {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let p = reader.f64()?;
        let randomizations = reader.u32()?;
        let permutation_pearson = reader.f64()?;
        if !p.is_finite()
            || !(0.0..=1.0).contains(&p)
            || !(1..=COLOC_PERMUTATIONS_MAX).contains(&randomizations)
            || !permutation_pearson.is_finite()
            || !(-1.0..=1.0).contains(&permutation_pearson)
        {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(p, randomizations, permutation_pearson))
    }
}

impl Decode for ColocCost {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let scan_voxels = reader.u64()?;
        let permutation_voxels = reader.u64()?;
        let permutation_frames = reader.u32()?;
        let randomizations = reader.u32()?;
        let blocks = reader.u64()?;
        if scan_voxels == 0
            || permutation_voxels == 0
            || permutation_frames == 0
            || blocks == 0
            || !(1..=COLOC_PERMUTATIONS_MAX).contains(&randomizations)
        {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(
            scan_voxels,
            permutation_voxels,
            permutation_frames,
            randomizations,
            blocks,
        ))
    }
}

impl Decode for ColocFrame {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let moments = ColocMoments::decode(reader)?;
        let a_by_b = Vec::<f64>::decode(reader)?;
        let b_by_a = Vec::<f64>::decode(reader)?;
        if a_by_b.is_empty() || a_by_b.len() != b_by_a.len() || a_by_b.len() > COLOC_BINS_MAX {
            return Err(FAULT_SHAPE);
        }
        if a_by_b
            .iter()
            .chain(&b_by_a)
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(FAULT_VALUE);
        }
        Ok(Self::from_parts(moments, a_by_b, b_by_a))
    }
}

impl Decode for ColocMoments {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let voxels = reader.u64()?;
        let mut sums = [0.0; 5];
        for value in &mut sums {
            *value = reader.f64()?;
        }
        if sums.iter().any(|value| !value.is_finite() || *value < 0.0) {
            return Err(FAULT_VALUE);
        }
        Ok(Self::new(voxels, sums))
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        match reader.bool()? {
            true => Ok(Some(T::decode(reader)?)),
            false => Ok(None),
        }
    }
}

fn provenance_acyclic(sets: &[SetState]) -> bool {
    let mut remaining = sets
        .iter()
        .map(|set| {
            (
                set.id,
                set.provenance
                    .sources()
                    .into_iter()
                    .map(ObjectSetId::raw)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    while !remaining.is_empty() {
        let removable = remaining
            .iter()
            .enumerate()
            .filter(|(_, (_, sources))| {
                sources
                    .iter()
                    .all(|source| !remaining.iter().any(|(id, _)| id == source))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if removable.is_empty() {
            return false;
        }
        for index in removable.into_iter().rev() {
            remaining.remove(index);
        }
    }
    true
}

fn color_applies(sets: &[SetState], set: &SetState, channels: usize, metric: ObjectColor) -> bool {
    match metric {
        ObjectColor::Measurement(measurement) => {
            measurement.colorable() && set.kind.measurements(channels).contains(&measurement)
        }
        ObjectColor::Relation {
            target,
            field,
            shift,
        } => {
            RelationField::CONTINUOUS.contains(&field)
                && set
                    .relations
                    .iter()
                    .any(|relation| relation.target == target.raw() && relation.shift == shift)
        }
        ObjectColor::Score => set.classifier.is_some(),
        ObjectColor::Coloc { pair, .. } => pair.a() < channels && pair.b() < channels,
        ObjectColor::Child { target, field } => {
            let Some(child) = sets.iter().find(|candidate| candidate.id == target.raw()) else {
                return false;
            };
            let metrics = child
                .kind
                .measurements(channels)
                .into_iter()
                .filter_map(ChildMetric::of)
                .collect::<Vec<_>>();
            child.id != set.id
                && child
                    .relations
                    .iter()
                    .any(|relation| relation.target == set.id && relation.shift == 0)
                && ChildField::fields(&metrics).contains(&field)
        }
    }
}

fn track_color_applies(set: &SetState, metric: TrackColor) -> bool {
    match metric {
        TrackColor::Measurement(measurement) => measurement.colorable(),
        TrackColor::Relation {
            target,
            field,
            shift,
            ..
        } => {
            RelationField::CONTINUOUS.contains(&field)
                && set
                    .relations
                    .iter()
                    .any(|relation| relation.target == target.raw() && relation.shift == shift)
        }
    }
}

fn decode_seeds(reader: &mut Reader<'_>) -> Result<Seeds, &'static str> {
    match reader.u8()? {
        0 => {
            let frames = reader.count(4)?;
            let mut seeds = Vec::new();
            for _ in 0..frames {
                let count = reader.count(12)?;
                let mut frame = Vec::new();
                for _ in 0..count {
                    let mut position = [0.0f32; 3];
                    for value in &mut position {
                        *value = finite(reader.f32()?)?;
                    }
                    frame.push(position);
                }
                seeds.push(frame);
            }
            Ok(Seeds::Points(seeds))
        }
        1 => {
            let source = ObjectSetId::from_raw(reader.u64()?).ok_or(FAULT_VALUE)?;
            Ok(Seeds::Source(source))
        }
        _ => Err(FAULT_TAG),
    }
}

fn group(tag: u8) -> Result<Group, &'static str> {
    Group::ALL
        .into_iter()
        .find(|group| group.index() == tag)
        .ok_or(FAULT_SHAPE)
}

fn aggregate(tag: u8) -> Result<Aggregate, &'static str> {
    Aggregate::ALL
        .into_iter()
        .find(|aggregate| aggregate.index() == tag)
        .ok_or(FAULT_SHAPE)
}

fn decode_mask_filter(reader: &mut Reader<'_>) -> Result<Option<MaskFilter>, &'static str> {
    if !reader.bool()? {
        return Ok(None);
    }
    let target = reader.u64()?;
    let shift = reader.i32()?;
    let field = RelationField::from_index(reader.u8()?).ok_or(FAULT_VALUE)?;
    let minimum = reader.f32()?;
    let maximum = reader.f32()?;
    if !RelationField::CONTINUOUS.contains(&field)
        || !minimum.is_finite()
        || !maximum.is_finite()
        || minimum > maximum
    {
        return Err(FAULT_VALUE);
    }
    let Some(target) = ObjectSetId::from_raw(target) else {
        return Err(FAULT_VALUE);
    };
    Ok(Some(MaskFilter::new(
        target, shift, field, minimum, maximum,
    )))
}

fn finite(value: f32) -> Result<f32, &'static str> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(FAULT_VALUE)
    }
}

fn positive(value: f32) -> Result<f32, &'static str> {
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(FAULT_VALUE)
    }
}

fn non_negative(value: f32) -> Result<f32, &'static str> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(FAULT_VALUE)
    }
}

fn unit(value: f32) -> Result<f32, &'static str> {
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(FAULT_VALUE)
    }
}

fn degrees(value: f32, limit: f32) -> Result<f32, &'static str> {
    if value.is_finite() && value.abs() <= limit {
        Ok(value)
    } else {
        Err(FAULT_VALUE)
    }
}
