use crate::marker::Marker;
use crate::transfer::TransferFunction;
use grana_object::Mesh;
use grana_volume::Intensity;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shading {
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,
    pub shininess: f32,
    pub shadows: f32,
}

impl Default for Shading {
    fn default() -> Self {
        Self {
            ambient: 0.35,
            diffuse: 0.75,
            specular: 0.30,
            shininess: 28.0,
            shadows: 0.7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerKey {
    owner: u64,
    part: u32,
    frame: Option<u32>,
    revision: u64,
}

impl LayerKey {
    pub fn at_frame(owner: u64, part: u32, frame: u32, revision: u64) -> Self {
        Self {
            owner,
            part,
            frame: Some(frame),
            revision,
        }
    }

    pub fn stable(owner: u64, part: u32, revision: u64) -> Self {
        Self {
            owner,
            part,
            frame: None,
            revision,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    color: [f32; 3],
    visible: bool,
    selected: bool,
}

impl Style {
    pub fn new(color: [f32; 3], visible: bool, selected: bool) -> Self {
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "a style color is an srgb triple"
        );
        Self {
            color,
            visible,
            selected,
        }
    }

    pub fn color(self) -> [f32; 3] {
        self.color
    }

    pub fn visible(self) -> bool {
        self.visible
    }

    pub fn selected(self) -> bool {
        self.selected
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelRow<'a> {
    pub visible: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub transfer: &'a TransferFunction,
}

#[derive(Clone, Copy)]
pub struct Layer<'a> {
    pub key: LayerKey,
    pub mesh: Option<&'a Mesh>,
    pub style_start: usize,
    pub style_count: usize,
}

pub struct SceneFrame<'a> {
    pub channels: &'a [ChannelRow<'a>],
    pub shading: Shading,
    pub step_scale: f32,
    pub styles: &'a [Style],
    pub layers: &'a [Layer<'a>],
    pub markers: &'a [Marker],
}
