use std::sync::Arc;

use crate::splat::Splats;
use grana_display::{Material, Shading, TransferFunction, VolumeStyle};
use grana_geometry::PackedMesh;
use grana_volume::Intensity;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerKey {
    owner: u64,
    part: u32,
    frame: Option<u32>,
    revision: u64,
    detail: u16,
}

impl LayerKey {
    pub fn at_frame(owner: u64, part: u32, frame: u32, revision: u64, detail: u16) -> Self {
        Self {
            owner,
            part,
            frame: Some(frame),
            revision,
            detail,
        }
    }

    pub fn stable(owner: u64, part: u32, revision: u64) -> Self {
        Self {
            owner,
            part,
            frame: None,
            revision,
            detail: 0,
        }
    }

    pub fn digest(self) -> u64 {
        let mut key = Digest::new();
        key.mix(self.owner);
        key.mix(u64::from(self.part));
        key.mix(self.frame.map_or(u64::MAX, u64::from));
        key.mix(self.revision);
        key.mix(u64::from(self.detail));
        key.finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Versions {
    data: u64,
    objects: u64,
    style: u64,
}

impl Versions {
    pub fn new(data: u64, objects: u64, style: u64) -> Self {
        Self {
            data,
            objects,
            style,
        }
    }

    pub fn data(self) -> u64 {
        self.data
    }

    pub fn objects(self) -> u64 {
        self.objects
    }

    pub fn style(self) -> u64 {
        self.style
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Digest(u64);

impl Default for Digest {
    fn default() -> Self {
        Self::new()
    }
}

impl Digest {
    const OFFSET: u64 = 0x6a09_e667_f3bc_c909;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub fn mix(&mut self, value: u64) {
        self.0 = (self.0 ^ value).wrapping_mul(Self::PRIME).rotate_left(23);
    }

    pub fn real(&mut self, value: f32) {
        self.mix(u64::from(value.to_bits()));
    }

    pub fn flag(&mut self, value: bool) {
        self.mix(u64::from(value));
    }

    pub fn finish(self) -> u64 {
        let value = self.0 ^ (self.0 >> 31);
        value.wrapping_mul(0xff51_afd7_ed55_8ccd) ^ (value >> 29)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    color: [f32; 3],
    visible: bool,
    selected: bool,
    opacity: f32,
    roughness: f32,
    metallic: f32,
}

impl Style {
    pub fn new(color: [f32; 3], visible: bool, selected: bool, opacity: f32) -> Self {
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "a style color is an srgb triple"
        );
        assert!(
            (0.0..=1.0).contains(&opacity),
            "a style opacity lies in the unit interval"
        );
        Self {
            color,
            visible,
            selected,
            opacity,
            roughness: Material::DEFAULT.roughness(),
            metallic: Material::DEFAULT.metallic(),
        }
    }

    pub fn color(self) -> [f32; 3] {
        self.color
    }

    pub fn visible(self) -> bool {
        self.visible
    }

    pub fn opacity(self) -> f32 {
        self.opacity
    }

    pub fn transparent(self) -> bool {
        self.opacity < 1.0
    }

    pub fn selected(self) -> bool {
        self.selected
    }

    pub fn roughness(self) -> f32 {
        self.roughness
    }

    pub fn metallic(self) -> f32 {
        self.metallic
    }

    pub fn with_material(self, material: Material) -> Self {
        Self {
            roughness: material.roughness(),
            metallic: material.metallic(),
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelRow {
    pub visible: bool,
    pub opacity: f32,
    pub window: Intensity,
    pub transfer: TransferFunction,
    pub volume: VolumeStyle,
}

#[derive(Clone)]
pub struct Layer {
    pub key: LayerKey,
    pub mesh: Arc<PackedMesh>,
    pub style_start: usize,
    pub style_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlay<'a> {
    key: u64,
    size: [u32; 2],
    pixels: &'a [u8],
    corners: [[f32; 3]; 4],
    tint: [f32; 3],
    opacity: f32,
}

impl<'a> Overlay<'a> {
    pub fn new(
        key: u64,
        size: [u32; 2],
        pixels: &'a [u8],
        corners: [[f32; 3]; 4],
        tint: [f32; 3],
        opacity: f32,
    ) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "an overlay image covers at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            size[0] as usize * size[1] as usize * 4,
            "an overlay image holds four bytes per pixel"
        );
        assert!(
            corners.iter().flatten().all(|value| value.is_finite()),
            "an overlay image lies at finite corners"
        );
        assert!(
            tint.iter().all(|value| (0.0..=1.0).contains(value)),
            "an overlay tint is a linear rgb triple"
        );
        assert!(
            opacity.is_finite() && (0.0..=1.0).contains(&opacity),
            "an overlay fades by a bounded opacity"
        );
        Self {
            key,
            size,
            pixels,
            corners,
            tint,
            opacity,
        }
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn pixels(&self) -> &'a [u8] {
        self.pixels
    }

    pub fn corners(&self) -> [[f32; 3]; 4] {
        self.corners
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn digest(&self) -> u64 {
        let mut digest = Digest::new();
        digest.mix(self.key);
        digest.mix(u64::from(self.size[0]) | (u64::from(self.size[1]) << 32));
        for corner in self.corners {
            for value in corner {
                digest.real(value);
            }
        }
        for value in self.tint {
            digest.real(value);
        }
        digest.real(self.opacity);
        digest.finish()
    }
}

pub struct SceneFrame<'a> {
    pub versions: Versions,
    pub channels: &'a [ChannelRow],
    pub shading: Shading,
    pub step_scale: f32,
    pub styles: &'a [Style],
    pub layers: &'a [Layer],
    pub splats: Splats<'a>,
    pub overlays: &'a [Overlay<'a>],
}
