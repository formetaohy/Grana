use glam::Vec3;
use grana_object::{Mesh, MeshVertex, Spot, TrackSet};
use grana_render::{ChannelParams, Marker, MeshKey, MeshStyle, Ray, RenderParams, Shading};
use grana_volume::{Axis, ChannelInfo, Intensity};

use crate::dataset::Dataset;
use crate::objects::{ObjectFrame, ObjectKind, ObjectSet, ObjectSetId, Objects};
use crate::tracks::TrackLayer;
use crate::transfer_editor::TransferEditor;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];
const SELECTED_SCALE: f32 = 1.3;

pub struct ChannelView {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub default_window: Intensity,
    transfer: TransferEditor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub set: ObjectSetId,
    pub item: Item,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Object(usize),
    Track(usize),
}

pub struct SceneLayer<'a> {
    pub key: MeshKey,
    pub mesh: Option<&'a Mesh>,
    pub styles: Vec<MeshStyle>,
}

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<ChannelView>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub objects: Objects,
    time: u32,
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
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            objects: Objects::new(),
            time: 0,
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
        if time == self.time {
            return;
        }
        self.time = time;
        if self
            .selection
            .is_some_and(|selection| matches!(selection.item, Item::Object(_)))
        {
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

    pub fn add_object(&mut self, kind: ObjectKind, channel: usize) -> ObjectSetId {
        self.objects.add(kind, channel, &self.dataset)
    }

    pub fn remove_object(&mut self, id: ObjectSetId) {
        self.objects.remove(id);
        if self.selection.is_some_and(|selection| selection.set == id) {
            self.selection = None;
        }
    }

    pub fn install(&mut self, id: ObjectSetId, time: u32, frame: ObjectFrame) {
        let Some(set) = self.objects.get_mut(id) else {
            return;
        };
        set.install(time, frame);
        if time == self.time {
            let stale = self.selection.is_some_and(|selection| {
                selection.set == id && matches!(selection.item, Item::Object(_))
            });
            if stale {
                self.selection = None;
            }
        }
    }

    pub fn clear_object(&mut self, id: ObjectSetId) {
        if let Some(set) = self.objects.get_mut(id) {
            set.clear();
        }
        if self.selection.is_some_and(|selection| selection.set == id) {
            self.selection = None;
        }
    }

    pub fn install_tracks(&mut self, id: ObjectSetId, set: TrackSet) {
        let Some(object) = self.objects.get_mut(id) else {
            return;
        };
        object.install_tracks(set);
        let stale = self.selection.is_some_and(|selection| {
            selection.set == id && matches!(selection.item, Item::Track(_))
        });
        if stale {
            self.selection = None;
        }
    }

    pub fn clear_tracks(&mut self, id: ObjectSetId) {
        if let Some(set) = self.objects.get_mut(id) {
            set.clear_tracks();
        }
        let stale = self.selection.is_some_and(|selection| {
            selection.set == id && matches!(selection.item, Item::Track(_))
        });
        if stale {
            self.selection = None;
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn select(&mut self, selection: Option<Selection>) {
        self.selection = selection;
    }

    pub fn select_track(&mut self, id: ObjectSetId, index: usize) {
        assert!(
            self.objects
                .get(id)
                .and_then(ObjectSet::tracks)
                .is_some_and(|layer| index < layer.set().len()),
            "the selected track exists"
        );
        self.selection = Some(Selection {
            set: id,
            item: Item::Track(index),
        });
    }

    pub fn selected_track(&self, id: ObjectSetId) -> Option<usize> {
        match self.selection {
            Some(Selection {
                set,
                item: Item::Track(index),
            }) if set == id => Some(index),
            _ => None,
        }
    }

    pub fn markers(&self, out: &mut Vec<Marker>) {
        out.clear();
        for set in self.objects.iter() {
            if !set.visible() || !self.channels[set.channel()].enabled {
                continue;
            }
            let Some(frame) = set.frame(self.time) else {
                continue;
            };
            let Some(spots) = frame.spots() else {
                continue;
            };
            for (index, spot) in spots.spots().iter().enumerate() {
                let selected = self.selection
                    == Some(Selection {
                        set: set.id(),
                        item: Item::Object(index),
                    });
                if selected {
                    out.push(Marker::new(
                        spot.center(),
                        SELECTED_SCALE * spot.radius(),
                        SELECTED_COLOR,
                        1.0,
                    ));
                } else {
                    out.push(Marker::new(spot.center(), spot.radius(), set.color(), 1.0));
                }
            }
        }
    }

    pub fn layers(&self) -> Vec<SceneLayer<'_>> {
        let mut layers = Vec::new();
        for set in self.objects.iter() {
            let id = set.id();
            let enabled = self.channels[set.channel()].enabled;
            let visible = set.visible() && enabled;
            if let (Some(frame), Some(stamp)) = (set.frame(self.time), set.frame_stamp(self.time))
                && let Some(mesh) = frame.mesh()
                && !frame.is_empty()
            {
                let styles = (0..frame.object_count())
                    .map(|index| {
                        let selected = self.selection
                            == Some(Selection {
                                set: id,
                                item: Item::Object(index),
                            });
                        MeshStyle::new(set.color(), visible, selected)
                    })
                    .collect();
                layers.push(SceneLayer {
                    key: MeshKey::of_frame(id.raw(), 0, self.time, stamp),
                    mesh: Some(mesh),
                    styles,
                });
            }
            if let Some(tracks) = set.tracks()
                && !tracks.set().is_empty()
            {
                let styles = (0..tracks.set().len())
                    .map(|index| {
                        let selected = self.selection
                            == Some(Selection {
                                set: id,
                                item: Item::Track(index),
                            });
                        MeshStyle::new(set.color(), visible && tracks.visible(), selected)
                    })
                    .collect();
                layers.push(SceneLayer {
                    key: MeshKey::timeless(id.raw(), 1, tracks.revision()),
                    mesh: Some(tracks.mesh()),
                    styles,
                });
            }
        }
        layers
    }

    pub fn pick_ray(&self, ray: &Ray) -> Option<Selection> {
        let mut best: Option<(f32, Selection)> = None;
        for set in self.pickable() {
            if let Some(frame) = set.frame(self.time) {
                if let Some(spots) = frame.spots() {
                    for (index, spot) in spots.spots().iter().enumerate() {
                        let Some(distance) = ray_sphere(ray, *spot) else {
                            continue;
                        };
                        let selection = Selection {
                            set: set.id(),
                            item: Item::Object(index),
                        };
                        if best.is_none_or(|(nearest, _)| distance < nearest) {
                            best = Some((distance, selection));
                        }
                    }
                }
                if let Some(mesh) = frame.mesh()
                    && !frame.is_empty()
                {
                    for index in 0..mesh.triangle_count() {
                        let triangle = mesh.triangle(index);
                        let Some(distance) = ray_triangle(ray, triangle) else {
                            continue;
                        };
                        let selection = Selection {
                            set: set.id(),
                            item: Item::Object(triangle[0].object() as usize),
                        };
                        if best.is_none_or(|(nearest, _)| distance < nearest) {
                            best = Some((distance, selection));
                        }
                    }
                }
            }
            if let Some(tracks) = self.pickable_tracks(set) {
                let mesh = tracks.mesh();
                for index in 0..mesh.triangle_count() {
                    let triangle = mesh.triangle(index);
                    let Some(distance) = ray_triangle(ray, triangle) else {
                        continue;
                    };
                    let selection = Selection {
                        set: set.id(),
                        item: Item::Track(triangle[0].object() as usize),
                    };
                    if best.is_none_or(|(nearest, _)| distance < nearest) {
                        best = Some((distance, selection));
                    }
                }
            }
        }
        best.map(|(_, selection)| selection)
    }

    pub fn pick_slice(&self, axis: Axis, point: Vec3) -> Option<Selection> {
        let mut best: Option<(f32, Selection)> = None;
        for set in self.pickable() {
            if let Some(frame) = set.frame(self.time) {
                if let Some(spots) = frame.spots() {
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
                        let selection = Selection {
                            set: set.id(),
                            item: Item::Object(index),
                        };
                        if best.is_none_or(|(nearest, _)| depth < nearest) {
                            best = Some((depth, selection));
                        }
                    }
                }
                if let Some(mesh) = frame.mesh()
                    && !frame.is_empty()
                    && let Some((offset, object)) = slice_mesh(mesh, axis, point)
                    && best.is_none_or(|(nearest, _)| offset < nearest)
                {
                    best = Some((
                        offset,
                        Selection {
                            set: set.id(),
                            item: Item::Object(object),
                        },
                    ));
                }
            }
            if let Some(tracks) = self.pickable_tracks(set)
                && let Some((offset, object)) = slice_mesh(tracks.mesh(), axis, point)
                && best.is_none_or(|(nearest, _)| offset < nearest)
            {
                best = Some((
                    offset,
                    Selection {
                        set: set.id(),
                        item: Item::Track(object),
                    },
                ));
            }
        }
        best.map(|(_, selection)| selection)
    }

    fn pickable(&self) -> impl Iterator<Item = &ObjectSet> + '_ {
        self.objects
            .iter()
            .filter(|set| set.visible())
            .filter(|set| self.channels[set.channel()].enabled)
    }

    fn pickable_tracks<'a>(&self, set: &'a ObjectSet) -> Option<&'a TrackLayer> {
        set.tracks().filter(|tracks| tracks.visible())
    }
}

fn slice_mesh(mesh: &Mesh, axis: Axis, point: Vec3) -> Option<(f32, usize)> {
    let plane = axis.index();
    let (u_axis, v_axis) = axis.in_plane();
    let (u, v) = (u_axis.index(), v_axis.index());
    let mut nearest: Option<(f32, bool, usize)> = None;
    for index in 0..mesh.triangle_count() {
        let triangle = mesh.triangle(index);
        let Some((offset, exit)) = slice_triangle(triangle, plane, u, v, point, axis) else {
            continue;
        };
        if offset < 0.0 {
            continue;
        }
        if nearest.is_none_or(|(nearest_offset, _, _)| offset < nearest_offset) {
            nearest = Some((offset, exit, triangle[0].object() as usize));
        }
    }
    match nearest {
        Some((offset, true, object)) => Some((offset, object)),
        _ => None,
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
