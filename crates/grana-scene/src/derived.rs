use std::sync::{Arc, Mutex};

use grana_object::{Mask, Roi};
use grana_volume::{ChannelInfo, Intensity, Level};

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
    frames: usize,
    time: u32,
    level: Level,
    mask: Arc<Mask>,
}

pub struct MaskChannel {
    source: usize,
    frames: Mutex<MaskFrames>,
    cache: Mutex<Vec<CacheEntry>>,
}

impl MaskChannel {
    pub fn new(source: usize, frames: MaskFrames) -> Self {
        Self {
            source,
            frames: Mutex::new(frames),
            cache: Mutex::new(Vec::new()),
        }
    }

    pub fn source(&self) -> usize {
        self.source
    }

    pub fn frames(&self) -> MaskFrames {
        Arc::clone(
            &self
                .frames
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
        )
    }

    pub fn set_frames(&self, frames: MaskFrames) -> bool {
        let mut held = self
            .frames
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if Arc::ptr_eq(&held, &frames) {
            return false;
        }
        *held = frames;
        true
    }

    pub fn mask(&self, time: u32, level: Level) -> Result<Arc<Mask>, String> {
        let frames = self.frames();
        let key = Arc::as_ptr(&frames) as usize;
        {
            let cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(entry) = cache
                .iter()
                .find(|entry| entry.frames == key && entry.time == time && entry.level == level)
            {
                return Ok(Arc::clone(&entry.mask));
            }
        }
        let roi = Roi::paged(frames.len(), move |time| {
            let Some(frame) = frames.get(time as usize).and_then(Option::as_ref) else {
                return Ok(None);
            };
            frame.shape().map(Some).map_err(|error| error.to_string())
        });
        let mask = Arc::new(roi.mask(level, time)?);
        let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
        cache.retain(|entry| entry.frames == key && entry.time != time);
        cache.push(CacheEntry {
            frames: key,
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
