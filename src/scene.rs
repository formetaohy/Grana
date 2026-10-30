use glam::Vec3;
use grana_analysis::{SpotParams, SurfaceParams};
use grana_object::{MeshVertex, Spot, SpotSet, SurfaceSet, SurfaceStatistics};
use grana_render::{ChannelParams, Marker, Ray, RenderParams, Shading, SurfaceStyle};
use grana_volume::{Axis, ChannelInfo, Intensity};

use crate::dataset::Dataset;
use crate::transfer_editor::TransferEditor;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];

const OBJECT_PALETTE: [[f32; 3]; 8] = [
    [0.35, 0.70, 1.00],
    [0.35, 0.90, 0.55],
    [1.00, 0.55, 0.35],
    [0.80, 0.55, 1.00],
    [1.00, 0.80, 0.35],
    [0.35, 0.90, 0.90],
    [1.00, 0.45, 0.70],
    [0.75, 0.85, 0.45],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Spot(usize),
    Surface(usize),
}

pub struct ChannelView {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub default_window: Intensity,
    transfer: TransferEditor,
}

struct ObjectLayer<T> {
    visible: bool,
    frames: Vec<Option<T>>,
}

impl<T> ObjectLayer<T> {
    fn new(frames: u32) -> Self {
        Self {
            visible: true,
            frames: (0..frames).map(|_| None).collect(),
        }
    }

    fn get(&self, time: u32) -> Option<&T> {
        self.frames.get(time as usize).and_then(Option::as_ref)
    }

    fn set(&mut self, time: u32, value: T) {
        assert!(
            (time as usize) < self.frames.len(),
            "the object frame lies inside the time series"
        );
        self.frames[time as usize] = Some(value);
    }

    fn detected(&self) -> usize {
        self.frames.iter().filter(|frame| frame.is_some()).count()
    }

    fn iter(&self) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.frames
            .iter()
            .enumerate()
            .filter_map(|(time, frame)| frame.as_ref().map(|set| (time as u32, set)))
    }
}

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<ChannelView>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub spot_params: SpotParams,
    pub surface_params: SurfaceParams,
    time: u32,
    spots: Option<ObjectLayer<SpotSet>>,
    surfaces: Option<ObjectLayer<SurfaceSet>>,
    selection: Option<Selection>,
}

impl Scene {
    pub fn new(dataset: Dataset) -> Self {
        let channels = dataset
            .channels()
            .iter()
            .map(|channel| ChannelView {
                enabled: true,
                opacity: 1.0,
                window: channel.window(),
                default_window: channel.window(),
                transfer: TransferEditor::new(channel.tint()),
            })
            .collect();
        let spot_params = SpotParams::new(4.0 * dataset.voxel_size().min_element(), 0.2);
        let surface_params = SurfaceParams::new(dataset.channels()[0].intensity().value(0.5), 0.0);
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            spot_params,
            surface_params,
            time: 0,
            spots: None,
            surfaces: None,
            selection: None,
        }
    }

    pub fn channel(&self, index: usize) -> &ChannelInfo {
        &self.dataset.channels()[index]
    }

    pub fn time(&self) -> u32 {
        self.time
    }

    pub fn frame_count(&self) -> u32 {
        self.dataset.frame_count()
    }

    pub fn set_time(&mut self, time: u32) {
        assert!(time < self.frame_count(), "the frame lies inside the scene");
        if time != self.time {
            self.time = time;
            self.selection = None;
        }
    }

    pub fn params(&self) -> RenderParams {
        RenderParams {
            shading: self.shading,
            step_scale: self.step_scale,
            channels: self
                .channels
                .iter()
                .map(|channel| ChannelParams {
                    visible: channel.enabled,
                    opacity: channel.opacity,
                    window: channel.window,
                    transfer: channel.transfer.transfer(),
                })
                .collect(),
        }
    }

    pub fn selected_transfer(&mut self) -> &mut TransferEditor {
        &mut self.channels[self.selected].transfer
    }

    pub fn select_channel(&mut self, index: usize) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.selected = index;
    }

    pub fn set_window(&mut self, index: usize, start: f32, end: f32) {
        let data = self.dataset.channels()[index].intensity();
        let low = start.min(end).clamp(data.min(), data.max());
        let high = start.max(end).clamp(data.min(), data.max());
        let minimum = (data.span() * 1.0e-3).max(f32::EPSILON);
        let high = if high - low < minimum {
            (low + minimum).min(data.max())
        } else {
            high
        };
        if high - low <= 0.0 {
            return;
        }
        self.channels[index].window = Intensity::new(low, high);
    }

    pub fn reset_window(&mut self, index: usize) {
        self.channels[index].window = self.channels[index].default_window;
    }

    pub fn spots(&self) -> Option<&SpotSet> {
        self.spots.as_ref().and_then(|layer| layer.get(self.time))
    }

    pub fn spot_frames(&self) -> impl Iterator<Item = (u32, &SpotSet)> + '_ {
        self.spots.iter().flat_map(|layer| layer.iter())
    }

    pub fn spot_frame_count(&self) -> usize {
        self.spots.as_ref().map_or(0, ObjectLayer::detected)
    }

    pub fn spot(&self, index: usize) -> Option<Spot> {
        self.spots().and_then(|set| set.spots().get(index).copied())
    }

    pub fn spot_visible(&self) -> bool {
        self.spots.as_ref().is_some_and(|layer| layer.visible)
    }

    pub fn set_spot_visible(&mut self, visible: bool) {
        if let Some(layer) = &mut self.spots {
            layer.visible = visible;
        }
    }

    pub fn install_spots(&mut self, time: u32, set: SpotSet) {
        assert!(
            set.channel() < self.channels.len(),
            "the spot set belongs to an existing channel"
        );
        let frames = self.frame_count();
        let layer = self.spots.get_or_insert_with(|| ObjectLayer::new(frames));
        layer.set(time, set);
        if time == self.time && matches!(self.selection, Some(Selection::Spot(_))) {
            self.selection = None;
        }
    }

    pub fn clear_spots(&mut self) {
        self.spots = None;
        if matches!(self.selection, Some(Selection::Spot(_))) {
            self.selection = None;
        }
    }

    pub fn surfaces(&self) -> Option<&SurfaceSet> {
        self.surfaces
            .as_ref()
            .and_then(|layer| layer.get(self.time))
    }

    pub fn surface_frames(&self) -> impl Iterator<Item = (u32, &SurfaceSet)> + '_ {
        self.surfaces.iter().flat_map(|layer| layer.iter())
    }

    pub fn surface_frame_count(&self) -> usize {
        self.surfaces.as_ref().map_or(0, ObjectLayer::detected)
    }

    pub fn surface(&self, index: usize) -> Option<SurfaceStatistics> {
        self.surfaces().and_then(|set| set.object(index))
    }

    pub fn surface_visible(&self) -> bool {
        self.surfaces.as_ref().is_some_and(|layer| layer.visible)
    }

    pub fn set_surface_visible(&mut self, visible: bool) {
        if let Some(layer) = &mut self.surfaces {
            layer.visible = visible;
        }
    }

    pub fn install_surfaces(&mut self, time: u32, set: SurfaceSet) {
        assert!(
            set.channel() < self.channels.len(),
            "the surface set belongs to an existing channel"
        );
        let frames = self.frame_count();
        let layer = self
            .surfaces
            .get_or_insert_with(|| ObjectLayer::new(frames));
        layer.set(time, set);
        if time == self.time && matches!(self.selection, Some(Selection::Surface(_))) {
            self.selection = None;
        }
    }

    pub fn clear_surfaces(&mut self) {
        self.surfaces = None;
        if matches!(self.selection, Some(Selection::Surface(_))) {
            self.selection = None;
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn select(&mut self, selection: Option<Selection>) {
        self.selection = selection;
    }

    pub fn markers(&self, out: &mut Vec<Marker>) {
        out.clear();
        let Some(layer) = &self.spots else {
            return;
        };
        if !layer.visible {
            return;
        }
        let Some(set) = layer.get(self.time) else {
            return;
        };
        let channel = set.channel();
        if !self.channels[channel].enabled {
            return;
        }
        let color = self.dataset.channels()[channel].tint();
        for (index, spot) in set.spots().iter().enumerate() {
            if self.selection == Some(Selection::Spot(index)) {
                out.push(Marker::new(
                    spot.center(),
                    1.3 * spot.radius(),
                    SELECTED_COLOR,
                    1.0,
                ));
            } else {
                out.push(Marker::new(spot.center(), spot.radius(), color, 1.0));
            }
        }
    }

    pub fn surface_styles(&self, out: &mut Vec<SurfaceStyle>) {
        out.clear();
        let Some(layer) = &self.surfaces else {
            return;
        };
        let Some(set) = layer.get(self.time) else {
            return;
        };
        let enabled = self.channels[set.channel()].enabled;
        for index in 0..set.object_count() {
            let selected = self.selection == Some(Selection::Surface(index));
            out.push(SurfaceStyle::new(
                Self::object_color(index),
                layer.visible && enabled,
                selected,
            ));
        }
    }

    pub fn object_color(index: usize) -> [f32; 3] {
        OBJECT_PALETTE[index % OBJECT_PALETTE.len()]
    }

    pub fn pick_ray(&self, ray: &Ray) -> Option<Selection> {
        let mut best: Option<(f32, Selection)> = None;
        if let Some(spots) = self.pickable_spots() {
            for (index, spot) in spots.spots().iter().enumerate() {
                let Some(distance) = ray_sphere(ray, *spot) else {
                    continue;
                };
                if best.is_none_or(|(nearest, _)| distance < nearest) {
                    best = Some((distance, Selection::Spot(index)));
                }
            }
        }
        if let Some(surfaces) = self.pickable_surfaces() {
            for index in 0..surfaces.mesh().triangle_count() {
                let triangle = surfaces.mesh().triangle(index);
                let Some(distance) = ray_triangle(ray, triangle) else {
                    continue;
                };
                if best.is_none_or(|(nearest, _)| distance < nearest) {
                    best = Some((distance, Selection::Surface(triangle[0].object() as usize)));
                }
            }
        }
        best.map(|(_, selection)| selection)
    }

    pub fn pick_slice(&self, axis: Axis, point: Vec3) -> Option<Selection> {
        let mut best: Option<(f32, Selection)> = None;
        if let Some(spots) = self.pickable_spots() {
            let plane = axis.index();
            let (u_axis, v_axis) = axis.in_plane();
            let (u, v) = (u_axis.index(), v_axis.index());
            for (index, spot) in spots.spots().iter().enumerate() {
                let offset = Vec3::from(spot.center()) - point;
                let depth = offset[plane].abs();
                let radius = spot.radius();
                if depth > radius {
                    continue;
                }
                let planar = Vec3::new(offset[u], offset[v], 0.0).length();
                if planar > (radius * radius - depth * depth).sqrt() {
                    continue;
                }
                if best.is_none_or(|(nearest, _)| depth < nearest) {
                    best = Some((depth, Selection::Spot(index)));
                }
            }
        }
        if let Some(surfaces) = self.pickable_surfaces() {
            let plane = axis.index();
            let (u_axis, v_axis) = axis.in_plane();
            let (u, v) = (u_axis.index(), v_axis.index());
            let mut nearest: Option<(f32, bool, usize)> = None;
            for index in 0..surfaces.mesh().triangle_count() {
                let triangle = surfaces.mesh().triangle(index);
                let Some((offset, exit)) = slice_triangle(triangle, plane, u, v, point, axis)
                else {
                    continue;
                };
                if offset < 0.0 {
                    continue;
                }
                if nearest.is_none_or(|(nearest_offset, _, _)| offset < nearest_offset) {
                    nearest = Some((offset, exit, triangle[0].object() as usize));
                }
            }
            if let Some((offset, true, object)) = nearest
                && best.is_none_or(|(nearest_offset, _)| offset < nearest_offset)
            {
                best = Some((offset, Selection::Surface(object)));
            }
        }
        best.map(|(_, selection)| selection)
    }

    fn pickable_spots(&self) -> Option<&SpotSet> {
        let layer = self.spots.as_ref()?;
        if !layer.visible {
            return None;
        }
        let set = layer.get(self.time)?;
        if !self.channels[set.channel()].enabled {
            return None;
        }
        Some(set)
    }

    fn pickable_surfaces(&self) -> Option<&SurfaceSet> {
        let layer = self.surfaces.as_ref()?;
        if !layer.visible {
            return None;
        }
        let set = layer.get(self.time)?;
        if !self.channels[set.channel()].enabled {
            return None;
        }
        Some(set)
    }
}

fn ray_sphere(ray: &Ray, spot: Spot) -> Option<f32> {
    let offset = Vec3::from(spot.center()) - ray.origin;
    let projection = offset.dot(ray.direction);
    let radius = spot.radius();
    let squared = offset.length_squared() - projection * projection;
    let radius_squared = radius * radius;
    if squared > radius_squared {
        return None;
    }
    let half = (radius_squared - squared).sqrt();
    if projection + half < 0.0 {
        return None;
    }
    Some((projection - half).max(0.0))
}

fn ray_triangle(ray: &Ray, triangle: [MeshVertex; 3]) -> Option<f32> {
    let [first, second, third] = triangle.map(|vertex| Vec3::from(vertex.position()));
    let edge = second - first;
    let across = third - first;
    let h = ray.direction.cross(across);
    let determinant = edge.dot(h);
    if determinant.abs() < 1.0e-12 {
        return None;
    }
    let inverse = 1.0 / determinant;
    let offset = ray.origin - first;
    let u = inverse * offset.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = offset.cross(edge);
    let v = inverse * ray.direction.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = inverse * across.dot(q);
    (distance > 1.0e-6).then_some(distance)
}

fn slice_triangle(
    triangle: [MeshVertex; 3],
    plane: usize,
    u: usize,
    v: usize,
    point: Vec3,
    axis: Axis,
) -> Option<(f32, bool)> {
    let positions = triangle.map(|vertex| Vec3::from(vertex.position()));
    let (a, b, c) = (positions[0], positions[1], positions[2]);
    let denominator = (b[u] - a[u]) * (c[v] - a[v]) - (c[u] - a[u]) * (b[v] - a[v]);
    if denominator.abs() < 1.0e-12 {
        return None;
    }
    let du = point[u] - a[u];
    let dv = point[v] - a[v];
    let first = (du * (c[v] - a[v]) - dv * (c[u] - a[u])) / denominator;
    let second = (dv * (b[u] - a[u]) - du * (b[v] - a[v])) / denominator;
    let third = 1.0 - first - second;
    if first < -1.0e-6 || second < -1.0e-6 || third < -1.0e-6 {
        return None;
    }
    let offset = first * (a[plane] - point[plane])
        + second * (b[plane] - point[plane])
        + third * (c[plane] - point[plane]);
    let mut normal = (b - a).cross(c - a);
    if normal.length_squared() < 1.0e-20 {
        return None;
    }
    normal = normal.normalize();
    let mut direction = Vec3::ZERO;
    direction[axis.index()] = 1.0;
    Some((offset, normal.dot(direction) > 0.0))
}
