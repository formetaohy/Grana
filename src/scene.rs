use glam::Vec3;
use grana_analysis::{Spot, SpotParams, SpotSet};
use grana_render::{ChannelDisplay, ChannelParams, Marker, Ray, RenderParams, Shading};
use grana_volume::{Axis, ChannelInfo, Intensity};

use crate::dataset::Dataset;
use crate::transfer_editor::TransferEditor;

const SELECTED_COLOR: [f32; 3] = [1.00, 0.93, 0.62];

pub struct ChannelView {
    pub enabled: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub default_window: Intensity,
    pub display: ChannelDisplay,
    transfer: TransferEditor,
}

struct SpotLayer {
    visible: bool,
    set: SpotSet,
}

pub struct Scene {
    pub dataset: Dataset,
    pub channels: Vec<ChannelView>,
    pub selected: usize,
    pub shading: Shading,
    pub step_scale: f32,
    pub spot_params: SpotParams,
    spots: Option<SpotLayer>,
    selection: Option<usize>,
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
                display: ChannelDisplay::Volume,
                transfer: TransferEditor::new(channel.tint()),
            })
            .collect();
        let spot_params = SpotParams::new(4.0 * dataset.voxel_size().min_element(), 0.2);
        Self {
            dataset,
            channels,
            selected: 0,
            shading: Shading::default(),
            step_scale: 1.0,
            spot_params,
            spots: None,
            selection: None,
        }
    }

    pub fn channel(&self, index: usize) -> &ChannelInfo {
        &self.dataset.channels()[index]
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
                    display: channel.display,
                })
                .collect(),
        }
    }

    pub fn selected_transfer(&mut self) -> &mut TransferEditor {
        &mut self.channels[self.selected].transfer
    }

    pub fn select(&mut self, index: usize) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.selected = index;
    }

    pub fn set_display(&mut self, index: usize, display: ChannelDisplay) {
        assert!(index < self.channels.len(), "channel index out of bounds");
        self.channels[index].display = display;
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
        self.spots.as_ref().map(|layer| &layer.set)
    }

    pub fn spot(&self, index: usize) -> Option<Spot> {
        self.spots
            .as_ref()
            .and_then(|layer| layer.set.spots().get(index).copied())
    }

    pub fn selection(&self) -> Option<usize> {
        self.selection
    }

    pub fn select_spot(&mut self, index: Option<usize>) {
        self.selection = index;
    }

    pub fn spot_visible(&self) -> bool {
        self.spots.as_ref().is_some_and(|layer| layer.visible)
    }

    pub fn set_spot_visible(&mut self, visible: bool) {
        if let Some(layer) = &mut self.spots {
            layer.visible = visible;
        }
    }

    pub fn install_spots(&mut self, set: SpotSet) {
        assert!(
            set.channel() < self.channels.len(),
            "the spot set belongs to an existing channel"
        );
        self.spots = Some(SpotLayer { visible: true, set });
        self.selection = None;
    }

    pub fn clear_spots(&mut self) {
        self.spots = None;
        self.selection = None;
    }

    pub fn markers(&self, out: &mut Vec<Marker>) {
        out.clear();
        let Some(layer) = &self.spots else {
            return;
        };
        if !layer.visible {
            return;
        }
        let channel = layer.set.channel();
        if !self.channels[channel].enabled {
            return;
        }
        let color = self.dataset.channels()[channel].tint();
        for (index, spot) in layer.set.spots().iter().enumerate() {
            if Some(index) == self.selection {
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

    pub fn pick_ray(&self, ray: &Ray) -> Option<usize> {
        let layer = self.pickable()?;
        let mut best: Option<(f32, usize)> = None;
        for (index, spot) in layer.spots().iter().enumerate() {
            let Some(distance) = ray_sphere(ray, *spot) else {
                continue;
            };
            if best.is_none_or(|(nearest, _)| distance < nearest) {
                best = Some((distance, index));
            }
        }
        best.map(|(_, index)| index)
    }

    pub fn pick_slice(&self, axis: Axis, point: Vec3) -> Option<usize> {
        let layer = self.pickable()?;
        let plane = axis.index();
        let (u_axis, v_axis) = axis.in_plane();
        let (u, v) = (u_axis.index(), v_axis.index());
        let mut best: Option<(f32, f32, usize)> = None;
        for (index, spot) in layer.spots().iter().enumerate() {
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
            let better = best.is_none_or(|(best_depth, best_planar, _)| {
                depth < best_depth || (depth == best_depth && planar < best_planar)
            });
            if better {
                best = Some((depth, planar, index));
            }
        }
        best.map(|(_, _, index)| index)
    }

    fn pickable(&self) -> Option<&SpotSet> {
        let layer = self.spots.as_ref()?;
        if !layer.visible || !self.channels[layer.set.channel()].enabled {
            return None;
        }
        Some(&layer.set)
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
