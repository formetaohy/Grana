use std::path::PathBuf;
use std::sync::Arc;

use grana_animation::{Keyframe, Timeline};
use grana_display::{CameraState, Palette, SectionState, Shading, Stop, TransferFunction};
use grana_geometry::{Bounds, GeometryGroup, PackedMesh, PackedVertex};
use grana_measure::{Aggregate, Filter, Group, Measurement, Statistic, TrackMeasurement};
use grana_measure::{
    COLOC_BINS, COLOC_SCATTER, ColocFrame, ColocMoments, ColocPair, ColocSeries, ColocThresholds,
};
use grana_object::{
    FilamentSet, FilamentStatistics, FilamentTopology, IntensityTable, SampleStatistics, Spot,
    SpotSet, SurfaceSet, SurfaceStatistics, Track, TrackNode, TrackSet,
};
use grana_scene::{
    ChannelState, ColocState, ColorState, ItemState, MeasureState, ObjectFrame, ObjectKind,
    ObjectParams, SceneState, SelectionState, SetState, TrackStyle, Visibility,
};
use grana_volume::{Axis, Intensity, Region};

use crate::binary::{Decode, Reader};
use crate::encode::{TAG_FILAMENTS, TAG_SPOTS, TAG_SURFACES};
use crate::project::{Project, Shape, Source, ViewState};

const FAULT_VALUE: &str = "the project data carries a value outside its domain";
const FAULT_SHAPE: &str = "the project data carries an inconsistent shape";

impl Decode for Project {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Self {
            shape: Shape::decode(reader)?,
            source: Source::decode(reader)?,
            scene: SceneState::decode(reader)?,
            measure: MeasureState::decode(reader)?,
            view: ViewState::decode(reader)?,
        })
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
            1 => Ok(Self::Zarr {
                name: reader.string()?,
                path: PathBuf::from(reader.string()?),
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
        };
        if !(0.0..=1.0).contains(&view.split) {
            return Err(FAULT_VALUE);
        }
        Ok(view)
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
        Ok(Self {
            axis: axis(reader.u8()?)?,
            span: positive(reader.f32()?)?,
        })
    }
}

impl Decode for SceneState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let state = Self {
            channels: Vec::<ChannelState>::decode(reader)?,
            selected: reader.count(1)?,
            shading: Shading::decode(reader)?,
            step_scale: positive(reader.f32()?)?,
            exposure: positive(reader.f32()?)?,
            region: Region::decode(reader)?,
            time: reader.u32()?,
            selection: Option::<SelectionState>::decode(reader)?,
            sets: Vec::<SetState>::decode(reader)?,
            coloc: ColocState::decode(reader)?,
        };
        if state.sets.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(FAULT_SHAPE);
        }
        let channels = state.channels.len();
        if state.sets.iter().any(|set| {
            set.channel >= channels
                || (!set.kind.trackable() && set.track_color.metric.is_some())
                || set
                    .object_color
                    .metric
                    .is_some_and(|metric| !color_applies(set.kind, channels, metric))
                || set
                    .track_color
                    .metric
                    .is_some_and(|metric| !metric.colorable())
        }) {
            return Err(FAULT_SHAPE);
        }
        Ok(state)
    }
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
        let mut minimum = [0.0; 3];
        for value in &mut minimum {
            *value = finite(reader.f32()?)?;
        }
        let mut maximum = [0.0; 3];
        for value in &mut maximum {
            *value = finite(reader.f32()?)?;
        }
        if !(0..3).all(|axis| minimum[axis] < maximum[axis]) {
            return Err(FAULT_VALUE);
        }
        let mut region = Self::new(minimum.into(), maximum.into());
        region.set_enabled(reader.bool()?);
        Ok(region)
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
        let params = ObjectParams::decode(reader)?;
        if params.kind() != kind {
            return Err(FAULT_SHAPE);
        }
        let state = Self {
            id,
            kind,
            name,
            channel,
            color,
            visible,
            params,
            track_style: Option::<TrackStyle>::decode(reader)?,
            tracks: Option::<TrackSet>::decode(reader)?,
            track_thickness: Option::<f32>::decode(reader)?.map(positive).transpose()?,
            track_visible: Option::<bool>::decode(reader)?,
            object_color: ColorState::<Measurement>::decode(reader)?,
            track_color: ColorState::<TrackMeasurement>::decode(reader)?,
            visibility: Visibility::decode(reader)?,
            frames: Vec::<Option<Arc<ObjectFrame>>>::decode(reader)?,
        };
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
        Ok(state)
    }
}

impl Decode for ObjectParams {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let tag = reader.u8()?;
        let first = finite(reader.f32()?)?;
        let second = finite(reader.f32()?)?;
        match tag {
            0 if first > 0.0 && (0.0..=1.0).contains(&second) => {
                Ok(Self::Spots(grana_analysis::SpotParams::new(first, second)))
            }
            1 if (0.0..=1.0).contains(&first) && second >= 0.0 => Ok(Self::Surfaces(
                grana_analysis::SurfaceParams::new(first, second),
            )),
            2 if (0.0..=1.0).contains(&first) && second > 0.0 => Ok(Self::Filaments(
                grana_analysis::FilamentParams::new(first, second),
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
            grana_analysis::TrackParams::new(max_step, max_gap, division, division_range),
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
            1 => Ok(Self::Object(Vec::<Vec<bool>>::decode(reader)?)),
            2 => Ok(Self::Track(Vec::<bool>::decode(reader)?)),
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
        Ok(measurement)
    }
}

impl Decode for TrackMeasurement {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        TrackMeasurement::from_index(reader.u8()?).ok_or(FAULT_SHAPE)
    }
}

impl Decode for ObjectFrame {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
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
        let geometry = PackedMesh::decode(reader)?;
        let intensities = IntensityTable::decode(reader)?;
        Self::from_parts(level, geometry, objects, intensities).ok_or(FAULT_SHAPE)
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
        Ok(Self::new(
            voxel_count,
            volume,
            area,
            centroid,
            Bounds::decode(reader)?,
        ))
    }
}

impl Decode for FilamentSet {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let level = reader.count(1)?;
        let objects = Vec::<FilamentStatistics>::decode(reader)?;
        let geometry = PackedMesh::decode(reader)?;
        let intensities = IntensityTable::decode(reader)?;
        Self::from_parts(level, geometry, objects, intensities).ok_or(FAULT_SHAPE)
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
        let groups = Vec::<GeometryGroup>::decode(reader)?;
        let vertices = Vec::<PackedVertex>::decode(reader)?;
        let narrow = Vec::<u16>::decode(reader)?;
        let wide = Vec::<u32>::decode(reader)?;
        Self::from_parts(groups, vertices, narrow, wide).ok_or(FAULT_SHAPE)
    }
}

impl Decode for GeometryGroup {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut min = [0.0; 3];
        for value in &mut min {
            *value = finite(reader.f32()?)?;
        }
        let mut scale = [0.0; 3];
        for value in &mut scale {
            *value = finite(reader.f32()?)?;
        }
        Ok(Self::new(
            min,
            scale,
            reader.u32()?,
            reader.u32()?,
            reader.u32()?,
            reader.u32()?,
            reader.bool()?,
        ))
    }
}

impl Decode for PackedVertex {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let mut position = [0; 3];
        for value in &mut position {
            *value = reader.u16()?;
        }
        let mut normal = [0; 2];
        for value in &mut normal {
            *value = reader.u16()?;
        }
        Ok(Self::new(position, normal))
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
        let filters = Vec::<Filter>::decode(reader)?;
        let sort = match reader.u8()? {
            0 => None,
            1 => Some((reader.count(1)?, reader.bool()?)),
            _ => return Err(FAULT_SHAPE),
        };
        Ok(Self {
            set,
            group,
            aggregate,
            filters,
            sort,
            columns: Vec::<bool>::decode(reader)?,
            visibility: reader.bool()?,
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

impl Decode for ColocState {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let state = Self {
            pair: Option::<ColocPair>::decode(reader)?,
            thresholds: ColocThresholds::decode(reader)?,
            region: Option::<Region>::decode(reader)?,
            series: Option::<ColocSeries>::decode(reader)?,
        };
        if let Some(series) = &state.series
            && (state.pair != Some(series.pair()) || state.region.is_none())
        {
            return Err(FAULT_SHAPE);
        }
        Ok(state)
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

impl Decode for ColocSeries {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let pair = ColocPair::decode(reader)?;
        let frames = Vec::<ColocFrame>::decode(reader)?;
        let counts_a = Vec::<u64>::decode(reader)?;
        let counts_b = Vec::<u64>::decode(reader)?;
        let scatter = Vec::<u64>::decode(reader)?;
        let total = ColocFrame::decode(reader)?;
        if counts_a.len() != COLOC_BINS
            || counts_b.len() != COLOC_BINS
            || scatter.len() != COLOC_SCATTER * COLOC_SCATTER
        {
            return Err(FAULT_SHAPE);
        }
        let voxels = total.moments().voxels();
        let counted: u64 = counts_a.iter().copied().sum();
        let scattered: u64 = scatter.iter().copied().sum();
        let framed: u64 = frames.iter().map(|frame| frame.moments().voxels()).sum();
        if counted != voxels || scattered != voxels || framed != voxels {
            return Err(FAULT_SHAPE);
        }
        Ok(Self::from_parts(
            pair, frames, counts_a, counts_b, scatter, total,
        ))
    }
}

impl Decode for ColocFrame {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        let moments = ColocMoments::decode(reader)?;
        let a_by_b = Vec::<f64>::decode(reader)?;
        let b_by_a = Vec::<f64>::decode(reader)?;
        if a_by_b.len() != COLOC_BINS || b_by_a.len() != COLOC_BINS {
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

fn color_applies(kind: ObjectKind, channels: usize, measurement: Measurement) -> bool {
    measurement.colorable() && kind.measurements(channels).contains(&measurement)
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

fn axis(tag: u8) -> Result<Axis, &'static str> {
    Axis::ALL.get(tag as usize).copied().ok_or(FAULT_SHAPE)
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

fn unit(value: f32) -> Result<f32, &'static str> {
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(FAULT_VALUE)
    }
}
