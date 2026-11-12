use std::sync::{Arc, Mutex};

use grana_object::{Mask, Roi};
use grana_optics::Psf;
use grana_volume::{ChannelInfo, Intensity, Level, VoxelFormat};

use crate::object::ObjectFrame;
use crate::object_set::ObjectSetId;

const CACHE_LIMIT: usize = 4;
const INTENSITY_FLOOR: f32 = 0.0;

pub type MaskFrames = Arc<Vec<Option<Arc<ObjectFrame>>>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmoothSpec {
    sigma: f32,
}

impl SmoothSpec {
    pub fn new(sigma: f32) -> Self {
        assert!(
            sigma.is_finite() && sigma > 0.0,
            "a smoothing operator blurs by a positive distance"
        );
        Self { sigma }
    }

    pub fn sigma(self) -> f32 {
        self.sigma
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackgroundSpec {
    radius: f32,
}

impl BackgroundSpec {
    pub fn new(radius: f32) -> Self {
        assert!(
            radius.is_finite() && radius > 0.0,
            "a background operator measures a positive radius"
        );
        Self { radius }
    }

    pub fn radius(self) -> f32 {
        self.radius
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RatioSpec {
    denominator: usize,
}

impl RatioSpec {
    pub fn new(denominator: usize) -> Self {
        Self { denominator }
    }

    pub fn denominator(self) -> usize {
        self.denominator
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RestoreSpec {
    psf: Psf,
    iterations: u32,
}

impl RestoreSpec {
    pub fn new(psf: Psf, iterations: u32) -> Self {
        assert!(
            (1..=1000).contains(&iterations),
            "richardson-lucy runs a bounded number of iterations"
        );
        Self { psf, iterations }
    }

    pub fn psf(&self) -> &Psf {
        &self.psf
    }

    pub fn iterations(&self) -> u32 {
        self.iterations
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operation {
    Mask { set: ObjectSetId },
    Restore(RestoreSpec),
    Smooth(SmoothSpec),
    Background(BackgroundSpec),
    Ratio(RatioSpec),
    Bleach,
}

impl Operation {
    pub fn mask(&self) -> Option<ObjectSetId> {
        match self {
            Self::Mask { set } => Some(*set),
            Self::Restore(_)
            | Self::Smooth(_)
            | Self::Background(_)
            | Self::Ratio(_)
            | Self::Bleach => None,
        }
    }

    pub fn restore(&self) -> Option<&RestoreSpec> {
        match self {
            Self::Restore(spec) => Some(spec),
            Self::Mask { .. }
            | Self::Smooth(_)
            | Self::Background(_)
            | Self::Ratio(_)
            | Self::Bleach => None,
        }
    }

    pub fn smooth(&self) -> Option<f32> {
        match self {
            Self::Smooth(spec) => Some(spec.sigma()),
            _ => None,
        }
    }

    pub fn background(&self) -> Option<f32> {
        match self {
            Self::Background(spec) => Some(spec.radius()),
            _ => None,
        }
    }

    pub fn ratio(&self) -> Option<usize> {
        match self {
            Self::Ratio(spec) => Some(spec.denominator()),
            _ => None,
        }
    }

    pub fn bleach(&self) -> bool {
        matches!(self, Self::Bleach)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DerivedSpec {
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    source: usize,
    operation: Operation,
}

impl DerivedSpec {
    pub fn new(
        source: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        operation: Operation,
    ) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "a derived channel carries a name for its display"
        );
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        assert!(
            intensity.span() > 0.0,
            "a derived channel declares a positive intensity span"
        );
        Self {
            name,
            tint,
            intensity,
            source,
            operation,
        }
    }

    pub fn masked(
        set: ObjectSetId,
        source: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
    ) -> Self {
        Self::new(source, name, tint, intensity, Operation::Mask { set })
    }

    pub fn restored(
        source: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        psf: Psf,
        iterations: u32,
    ) -> Self {
        Self::new(
            source,
            name,
            tint,
            intensity,
            Operation::Restore(RestoreSpec::new(psf, iterations)),
        )
    }

    pub fn smoothed(
        source: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        sigma: f32,
    ) -> Self {
        Self::new(
            source,
            name,
            tint,
            intensity,
            Operation::Smooth(SmoothSpec::new(sigma)),
        )
    }

    pub fn background_subtracted(
        source: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        radius: f32,
    ) -> Self {
        Self::new(
            source,
            name,
            tint,
            intensity,
            Operation::Background(BackgroundSpec::new(radius)),
        )
    }

    pub fn ratio(
        numerator: usize,
        denominator: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
    ) -> Self {
        assert_ne!(
            numerator, denominator,
            "a ratio channel divides two distinct channels"
        );
        Self::new(
            numerator,
            name,
            tint,
            intensity,
            Operation::Ratio(RatioSpec::new(denominator)),
        )
    }

    pub fn bleached(
        source: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
    ) -> Self {
        Self::new(source, name, tint, intensity, Operation::Bleach)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    pub fn source(&self) -> usize {
        self.source
    }

    pub fn operation(&self) -> &Operation {
        &self.operation
    }

    pub fn replacing(&self, operation: Operation, intensity: Intensity) -> Self {
        Self::new(
            self.source,
            self.name.clone(),
            self.tint,
            intensity,
            operation,
        )
    }

    pub fn rebased(&self, source: usize, denominator: Option<usize>) -> Self {
        let operation = match self.operation {
            Operation::Ratio(_) => Operation::Ratio(RatioSpec::new(
                denominator.expect("a ratio channel keeps its denominator"),
            )),
            Operation::Mask { .. }
            | Operation::Restore(_)
            | Operation::Smooth(_)
            | Operation::Background(_)
            | Operation::Bleach => self.operation.clone(),
        };
        Self {
            name: self.name.clone(),
            tint: self.tint,
            intensity: self.intensity,
            source,
            operation,
        }
    }

    pub fn mask(&self) -> Option<ObjectSetId> {
        self.operation.mask()
    }

    pub fn restore(&self) -> Option<&RestoreSpec> {
        self.operation.restore()
    }

    pub fn smooth(&self) -> Option<f32> {
        self.operation.smooth()
    }

    pub fn background(&self) -> Option<f32> {
        self.operation.background()
    }

    pub fn ratio_denominator(&self) -> Option<usize> {
        self.operation.ratio()
    }

    pub fn info(&self, source: &ChannelInfo) -> ChannelInfo {
        let format = if self.mask().is_some() {
            source.format()
        } else {
            VoxelFormat::F32
        };
        ChannelInfo::new(
            self.name.clone(),
            self.tint,
            self.intensity,
            self.intensity,
            format,
        )
    }
}

pub struct Derived {
    spec: Arc<DerivedSpec>,
    mask: Option<Arc<MaskChannel>>,
}

impl Derived {
    pub fn plain(spec: DerivedSpec) -> Self {
        assert!(
            spec.mask().is_none(),
            "a plain derived channel does not bind object frames"
        );
        Self {
            spec: Arc::new(spec),
            mask: None,
        }
    }

    pub fn masked(spec: DerivedSpec, frames: MaskFrames, radius: f32, stamp: u64) -> Self {
        assert!(
            spec.mask().is_some(),
            "a bound mask channel masks an object set"
        );
        let channel = Arc::new(MaskChannel::new(spec.source(), frames, radius, stamp));
        Self {
            spec: Arc::new(spec),
            mask: Some(channel),
        }
    }

    pub fn spec(&self) -> &DerivedSpec {
        &self.spec
    }

    pub fn shared(&self) -> Arc<DerivedSpec> {
        Arc::clone(&self.spec)
    }

    pub fn mask(&self) -> Option<&Arc<MaskChannel>> {
        self.mask.as_ref()
    }

    pub fn frames(&self) -> Option<MaskFrames> {
        self.mask.as_ref().map(|channel| channel.frames())
    }
}

pub fn derived_infos(
    specs: &[DerivedSpec],
    base: &[ChannelInfo],
) -> Result<Vec<ChannelInfo>, String> {
    let mut channels = base.to_vec();
    let mut infos = Vec::with_capacity(specs.len());
    for spec in specs {
        let source = channels.get(spec.source()).ok_or_else(|| {
            format!(
                "the derived channel {} reads a channel the dataset does not hold",
                spec.name()
            )
        })?;
        if let Some(denominator) = spec.ratio_denominator()
            && denominator >= channels.len()
        {
            return Err(format!(
                "the ratio channel {} divides a channel the dataset does not hold",
                spec.name()
            ));
        }
        let info = spec.info(source);
        channels.push(info.clone());
        infos.push(info);
    }
    Ok(infos)
}

pub fn mask_info(name: &str, tint: [f32; 3], source: &ChannelInfo) -> ChannelInfo {
    let intensity = mask_intensity(source);
    ChannelInfo::new(
        name.to_string(),
        tint,
        intensity,
        intensity,
        source.format(),
    )
}

pub fn mask_intensity(source: &ChannelInfo) -> Intensity {
    let intensity = source.intensity();
    Intensity::new(
        intensity.min().min(INTENSITY_FLOOR),
        intensity.max().max(INTENSITY_FLOOR),
    )
}

struct CacheEntry {
    stamp: u64,
    time: u32,
    level: Level,
    mask: Arc<Mask>,
}

struct Gate {
    frames: MaskFrames,
    radius: f32,
    stamp: u64,
}

pub struct MaskChannel {
    source: usize,
    gate: Mutex<Gate>,
    cache: Mutex<Vec<CacheEntry>>,
}

impl MaskChannel {
    pub fn new(source: usize, frames: MaskFrames, radius: f32, stamp: u64) -> Self {
        Self {
            source,
            gate: Mutex::new(Gate {
                frames,
                radius,
                stamp,
            }),
            cache: Mutex::new(Vec::new()),
        }
    }

    pub fn source(&self) -> usize {
        self.source
    }

    pub fn frames(&self) -> MaskFrames {
        let gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        Arc::clone(&gate.frames)
    }

    pub fn stamp(&self) -> u64 {
        self.gate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .stamp
    }

    pub fn refresh(&self, frames: MaskFrames, radius: f32, stamp: u64) -> bool {
        let mut gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        if gate.stamp == stamp {
            return false;
        }
        gate.frames = frames;
        gate.radius = radius;
        gate.stamp = stamp;
        true
    }

    pub fn mask(&self, time: u32, level: Level) -> Result<Arc<Mask>, String> {
        let (frames, radius, stamp) = {
            let gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
            (Arc::clone(&gate.frames), gate.radius, gate.stamp)
        };
        {
            let cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(entry) = cache
                .iter()
                .find(|entry| entry.stamp == stamp && entry.time == time && entry.level == level)
            {
                return Ok(Arc::clone(&entry.mask));
            }
        }
        let roi = Roi::paged(frames.len(), move |time| {
            let Some(frame) = frames.get(time as usize).and_then(Option::as_ref) else {
                return Ok(None);
            };
            frame.shape().map(Some).map_err(|error| error.to_string())
        })
        .with_radius(radius);
        let mask = Arc::new(roi.mask(level, time)?);
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        cache.retain(|entry| entry.stamp == stamp && entry.time != time);
        cache.push(CacheEntry {
            stamp,
            time,
            level,
            mask: Arc::clone(&mask),
        });
        if cache.len() > CACHE_LIMIT {
            cache.remove(0);
        }
        Ok(mask)
    }
}
