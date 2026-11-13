use grana_object::LocalizationSpec;

use super::{
    FLOOR, Groups, LocalizationDepth, LocalizationFrame, LocalizationGeometry,
    LocalizationRenderer, Survey, localization_weight, margin_pixels, preview_geometry,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompositionSpec {
    pixel: f32,
    blur: f32,
    depth: f32,
}

impl CompositionSpec {
    pub const DEPTH_MAXIMUM: f32 = grana_object::LocalizationSpec::DEPTH_MAXIMUM;

    pub fn new(pixel: f32, blur: f32, depth: f32) -> Self {
        assert!(
            pixel.is_finite() && pixel > 0.0,
            "a composed image renders at a positive pixel size"
        );
        assert!(
            blur.is_finite() && blur >= 0.0,
            "a composed image blurs by a non-negative width"
        );
        assert!(
            depth.is_finite() && (0.0..=Self::DEPTH_MAXIMUM).contains(&depth),
            "a composed image slices a bounded depth"
        );
        Self { pixel, blur, depth }
    }

    pub fn covering(specs: &[LocalizationSpec]) -> Option<Self> {
        specs.first()?;
        let pixel = specs
            .iter()
            .map(LocalizationSpec::pixel)
            .fold(f32::MIN, f32::max);
        let blur = specs.iter().map(LocalizationSpec::blur).fold(0.0, f32::max);
        let depth = specs
            .iter()
            .map(LocalizationSpec::depth)
            .filter(|depth| *depth > 0.0)
            .fold(f32::INFINITY, f32::min);
        let depth = if depth.is_finite() {
            depth * 0.001
        } else {
            0.0
        };
        Some(Self::new(pixel * 0.001, blur * 0.001, depth))
    }

    pub fn pixel(&self) -> f32 {
        self.pixel
    }

    pub fn blur(&self) -> f32 {
        self.blur
    }

    pub fn depth(&self) -> f32 {
        self.depth
    }
}

#[derive(Clone, Copy)]
pub struct CompositionEntry<'a> {
    frames: &'a [LocalizationFrame<'a>],
    tint: [f32; 3],
}

impl<'a> CompositionEntry<'a> {
    pub fn new(frames: &'a [LocalizationFrame<'a>], tint: [f32; 3]) -> Self {
        assert!(
            !frames.is_empty(),
            "a composed channel holds at least one frame"
        );
        assert!(
            tint.iter().all(|value| (0.0..=1.0).contains(value)),
            "a composed channel tints within the unit interval"
        );
        Self { frames, tint }
    }

    pub fn frames(&self) -> &'a [LocalizationFrame<'a>] {
        self.frames
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }
}

pub struct LocalizationComposition<'a> {
    geometry: LocalizationGeometry,
    depth: LocalizationDepth,
    channels: Vec<Channel<'a>>,
}

struct Channel<'a> {
    renderer: LocalizationRenderer<'a>,
    tint: [f32; 3],
    row: Vec<u8>,
}

impl<'a> LocalizationComposition<'a> {
    pub fn new(
        entries: &[CompositionEntry<'a>],
        spec: CompositionSpec,
        progress: &'a dyn Fn(usize, usize) -> bool,
    ) -> Result<Self, String> {
        if entries.is_empty() {
            return Err("a composed image holds at least one channel".to_string());
        }
        let mut low = [f32::INFINITY; 2];
        let mut high = [f32::NEG_INFINITY; 2];
        let mut bottom = f32::INFINITY;
        let mut top = f32::NEG_INFINITY;
        for entry in entries {
            let survey = Survey::of(entry.frames());
            let (frame_low, frame_high) = survey
                .bounds
                .ok_or_else(|| "a composed channel lies at an infinite position".to_string())?;
            for axis in 0..2 {
                low[axis] = low[axis].min(frame_low[axis]);
                high[axis] = high[axis].max(frame_high[axis]);
            }
            let [below, above] = survey
                .heights
                .ok_or_else(|| "a composed channel lies at an infinite height".to_string())?;
            bottom = bottom.min(below);
            top = top.max(above);
        }
        let span = [0, 1].map(|axis| (high[axis] - low[axis]).max(FLOOR));
        let depth = if spec.depth() > 0.0 {
            LocalizationDepth::sliced(bottom, top, spec.depth())?
        } else {
            LocalizationDepth::projection(bottom)
        };
        let preview = preview_geometry(low, span, spec.pixel(), spec.blur(), depth)?;
        let geometry = LocalizationGeometry::new(
            preview.origin(),
            preview.pixel(),
            preview.size(),
            spec.blur(),
            margin_pixels(spec.blur(), preview.pixel()),
        );
        let width = geometry.size()[0] as usize;
        let mut channels = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            if !progress(index, entries.len()) {
                return Err("the composed image was cancelled".to_string());
            }
            let mut renderer = LocalizationRenderer::new(
                entry.frames(),
                geometry,
                depth,
                Groups::default(),
                localization_weight(entry.frames()),
                progress,
            )?;
            let density = renderer.survey()?;
            renderer.at_density(density);
            channels.push(Channel {
                renderer,
                tint: entry.tint(),
                row: vec![0u8; width * 3],
            });
        }
        Ok(Self {
            geometry,
            depth,
            channels,
        })
    }

    pub fn geometry(&self) -> LocalizationGeometry {
        self.geometry
    }

    pub fn depth(&self) -> LocalizationDepth {
        self.depth
    }

    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    pub fn begin(&mut self, layer: u32) -> Result<(), String> {
        for channel in &mut self.channels {
            channel.renderer.begin(layer)?;
        }
        Ok(())
    }

    pub fn row_rgb(&mut self, row: &mut [u8]) -> Result<(), String> {
        assert_eq!(
            row.len(),
            self.geometry.size()[0] as usize * 3,
            "a composed row holds three bytes per pixel"
        );
        row.fill(0);
        for channel in &mut self.channels {
            channel.renderer.row_rgb(&mut channel.row)?;
            let tint = channel.tint;
            for (target, texel) in row
                .as_chunks_mut::<3>()
                .0
                .iter_mut()
                .zip(channel.row.as_chunks::<3>().0)
            {
                let light = f32::from(texel[0].max(texel[1]).max(texel[2])) * (1.0 / 255.0);
                for axis in 0..3 {
                    let value = f32::from(target[axis]) + tint[axis] * light * 255.0;
                    target[axis] = value.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        Ok(())
    }
}
