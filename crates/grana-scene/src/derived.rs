use std::sync::{Arc, Mutex};

use grana_object::{Mask, Roi};
use grana_optics::PsfParams;
use grana_volume::{ChannelInfo, Intensity, Level, VoxelFormat};

use crate::object::ObjectFrame;
use crate::object_set::ObjectSetId;

const CACHE_LIMIT: usize = 4;
const INTENSITY_FLOOR: f32 = 0.0;

pub type MaskFrames = Arc<Vec<Option<Arc<ObjectFrame>>>>;

#[derive(Clone, Debug, PartialEq)]
pub struct MaskSpec {
    set: ObjectSetId,
    channel: usize,
    name: String,
    tint: [f32; 3],
}

impl MaskSpec {
    pub fn new(set: ObjectSetId, channel: usize, name: impl Into<String>, tint: [f32; 3]) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "a mask channel carries a name for its display"
        );
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        Self {
            set,
            channel,
            name,
            tint,
        }
    }

    pub fn set(&self) -> ObjectSetId {
        self.set
    }

    pub fn channel(&self) -> usize {
        self.channel
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tint(&self) -> [f32; 3] {
        self.tint
    }

    pub fn info(&self, source: &ChannelInfo) -> ChannelInfo {
        mask_info(&self.name, self.tint, source)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RestoreSpec {
    channel: usize,
    name: String,
    tint: [f32; 3],
    intensity: Intensity,
    params: PsfParams,
    iterations: u32,
}

impl RestoreSpec {
    pub fn new(
        channel: usize,
        name: impl Into<String>,
        tint: [f32; 3],
        intensity: Intensity,
        params: PsfParams,
        iterations: u32,
    ) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "a restored channel carries a name for its display"
        );
        assert!(
            tint.iter().all(|component| (0.0..=1.0).contains(component)),
            "a channel tint is an srgb triple"
        );
        assert!(
            intensity.span() > 0.0,
            "a restored channel declares a positive intensity span"
        );
        assert!(
            (1..=1000).contains(&iterations),
            "richardson-lucy runs a bounded number of iterations"
        );
        Self {
            channel,
            name,
            tint,
            intensity,
            params,
            iterations,
        }
    }

    pub fn channel(&self) -> usize {
        self.channel
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

    pub fn params(&self) -> PsfParams {
        self.params
    }

    pub fn iterations(&self) -> u32 {
        self.iterations
    }

    pub fn info(&self) -> ChannelInfo {
        ChannelInfo::new(
            self.name.clone(),
            self.tint,
            self.intensity,
            self.intensity,
            VoxelFormat::F32,
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DerivedSpec {
    Mask(MaskSpec),
    Restore(RestoreSpec),
}

impl DerivedSpec {
    pub fn name(&self) -> &str {
        match self {
            Self::Mask(spec) => spec.name(),
            Self::Restore(spec) => spec.name(),
        }
    }

    pub fn tint(&self) -> [f32; 3] {
        match self {
            Self::Mask(spec) => spec.tint(),
            Self::Restore(spec) => spec.tint(),
        }
    }

    pub fn source(&self) -> usize {
        match self {
            Self::Mask(spec) => spec.channel(),
            Self::Restore(spec) => spec.channel(),
        }
    }

    pub fn mask(&self) -> Option<&MaskSpec> {
        match self {
            Self::Mask(spec) => Some(spec),
            Self::Restore(_) => None,
        }
    }

    pub fn restore(&self) -> Option<&RestoreSpec> {
        match self {
            Self::Mask(_) => None,
            Self::Restore(spec) => Some(spec),
        }
    }

    pub fn info(&self, source: &ChannelInfo) -> ChannelInfo {
        match self {
            Self::Mask(spec) => spec.info(source),
            Self::Restore(spec) => spec.info(),
        }
    }
}

pub struct Derived {
    spec: DerivedSpec,
    mask: Option<Arc<MaskChannel>>,
}

impl Derived {
    pub fn restored(spec: RestoreSpec) -> Self {
        Self {
            spec: DerivedSpec::Restore(spec),
            mask: None,
        }
    }

    pub fn masked(spec: MaskSpec, frames: MaskFrames, radius: f32, stamp: u64) -> Self {
        let channel = Arc::new(MaskChannel::new(spec.channel(), frames, radius, stamp));
        Self {
            spec: DerivedSpec::Mask(spec),
            mask: Some(channel),
        }
    }

    pub fn spec(&self) -> &DerivedSpec {
        &self.spec
    }

    pub fn mask(&self) -> Option<&Arc<MaskChannel>> {
        self.mask.as_ref()
    }

    pub fn frames(&self) -> Option<MaskFrames> {
        self.mask.as_ref().map(|channel| channel.frames())
    }
}

pub fn mask_info(name: &str, tint: [f32; 3], source: &ChannelInfo) -> ChannelInfo {
    let intensity = Intensity::new(
        source.intensity().min().min(INTENSITY_FLOOR),
        source.intensity().max().max(INTENSITY_FLOOR),
    );
    ChannelInfo::new(
        name.to_string(),
        tint,
        intensity,
        intensity,
        source.format(),
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
