use grana_display::{CameraState, SectionState};

const KEY_EPSILON: f32 = 1.0e-3;
const MAX_FPS: u32 = 240;
const MAX_DURATION: f32 = 3600.0;
const HASH_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const HASH_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    pub at: f32,
    pub time: u32,
    pub camera: CameraState,
    pub section: SectionState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub time: u32,
    pub camera: CameraState,
    pub section: SectionState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    fps: u32,
    duration: f32,
    keys: Vec<Keyframe>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new(30, 10.0)
    }
}

impl Timeline {
    pub const MAX_FPS: u32 = MAX_FPS;
    pub const MAX_DURATION: f32 = MAX_DURATION;

    pub fn new(fps: u32, duration: f32) -> Self {
        assert!(
            (1..=MAX_FPS).contains(&fps),
            "a movie plays one to {MAX_FPS} frames per second"
        );
        assert!(
            duration.is_finite() && duration > 0.0 && duration <= MAX_DURATION,
            "a movie lasts a positive moment of at most {MAX_DURATION} seconds"
        );
        Self {
            fps,
            duration,
            keys: Vec::new(),
        }
    }

    pub fn from_parts(fps: u32, duration: f32, keys: Vec<Keyframe>) -> Result<Self, &'static str> {
        if !(1..=MAX_FPS).contains(&fps) {
            return Err("the timeline declares an impossible frame rate");
        }
        if !duration.is_finite() || duration <= 0.0 || duration > MAX_DURATION {
            return Err("the timeline declares an impossible duration");
        }
        for key in &keys {
            if !key.at.is_finite() || !(0.0..=MAX_DURATION).contains(&key.at) {
                return Err("a keyframe lies outside the timeline");
            }
            if !plausible_camera(&key.camera) || !plausible_section(&key.section) {
                return Err("a keyframe describes a view outside its domain");
            }
        }
        if keys.windows(2).any(|pair| pair[0].at >= pair[1].at) {
            return Err("the keyframes are not sorted along the timeline");
        }
        Ok(Self {
            fps,
            duration,
            keys,
        })
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    pub fn set_fps(&mut self, fps: u32) {
        assert!(
            (1..=MAX_FPS).contains(&fps),
            "a movie plays one to {MAX_FPS} frames per second"
        );
        self.fps = fps;
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn set_duration(&mut self, duration: f32) {
        assert!(
            duration.is_finite() && duration > 0.0 && duration <= MAX_DURATION,
            "a movie lasts a positive moment of at most {MAX_DURATION} seconds"
        );
        self.duration = duration;
    }

    pub fn keys(&self) -> &[Keyframe] {
        &self.keys
    }

    pub fn key(&self, index: usize) -> Option<&Keyframe> {
        self.keys.get(index)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn frames(&self) -> u32 {
        (self.duration * self.fps as f32).round().max(1.0) as u32
    }

    pub fn insert(&mut self, key: Keyframe) -> usize {
        assert!(
            key.at.is_finite() && (0.0..=MAX_DURATION).contains(&key.at),
            "a keyframe lies on the timeline"
        );
        assert!(
            plausible_camera(&key.camera) && plausible_section(&key.section),
            "a keyframe describes a plausible view"
        );
        let index = self
            .keys
            .partition_point(|other| other.at < key.at - KEY_EPSILON);
        match self.keys.get_mut(index) {
            Some(existing) if (existing.at - key.at).abs() <= KEY_EPSILON => {
                *existing = key;
            }
            _ => self.keys.insert(index, key),
        }
        index
    }

    pub fn remove(&mut self, index: usize) -> Option<Keyframe> {
        (index < self.keys.len()).then(|| self.keys.remove(index))
    }

    pub fn clear(&mut self) {
        self.keys.clear();
    }

    pub fn sample(&self, at: f32) -> Option<Sample> {
        assert!(at.is_finite(), "a playhead marks a finite moment");
        let first = self.keys.first()?;
        let last = self.keys.last()?;
        if at <= first.at {
            return Some(held(*first));
        }
        if at >= last.at {
            return Some(held(*last));
        }
        let index = self.keys.partition_point(|key| key.at <= at);
        Some(blended(self.keys[index - 1], self.keys[index], at))
    }

    pub fn sample_frame(&self, index: u32) -> Option<Sample> {
        if index >= self.frames() {
            return None;
        }
        self.sample(index as f32 / self.fps as f32)
    }

    pub fn document(&self) -> u64 {
        let mut hash = HASH_OFFSET;
        hash = mix(hash, u64::from(self.fps));
        hash = mix(hash, u64::from(self.duration.to_bits()));
        for key in &self.keys {
            hash = mix(hash, u64::from(key.at.to_bits()));
            hash = mix(hash, u64::from(key.time));
            for value in key.camera.pivot {
                hash = mix(hash, u64::from(value.to_bits()));
            }
            hash = mix(hash, u64::from(key.camera.yaw.to_bits()));
            hash = mix(hash, u64::from(key.camera.pitch.to_bits()));
            hash = mix(hash, u64::from(key.camera.distance.to_bits()));
            hash = mix(hash, u64::from(key.camera.fov_y.to_bits()));
            hash = mix(hash, key.section.axis.index() as u64);
            hash = mix(hash, u64::from(key.section.span.to_bits()));
        }
        hash
    }
}

fn held(key: Keyframe) -> Sample {
    Sample {
        time: key.time,
        camera: key.camera,
        section: key.section,
    }
}

fn blended(from: Keyframe, to: Keyframe, at: f32) -> Sample {
    let span = to.at - from.at;
    assert!(span > 0.0, "a blended segment spans a positive moment");
    let t = ((at - from.at) / span).clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    let lerp = |start: f32, end: f32| start + (end - start) * eased;
    let camera = CameraState {
        pivot: [
            lerp(from.camera.pivot[0], to.camera.pivot[0]),
            lerp(from.camera.pivot[1], to.camera.pivot[1]),
            lerp(from.camera.pivot[2], to.camera.pivot[2]),
        ],
        yaw: lerp(from.camera.yaw, to.camera.yaw),
        pitch: lerp(from.camera.pitch, to.camera.pitch),
        distance: lerp(from.camera.distance, to.camera.distance),
        fov_y: lerp(from.camera.fov_y, to.camera.fov_y),
    };
    let section = SectionState {
        axis: if eased < 0.5 {
            from.section.axis
        } else {
            to.section.axis
        },
        span: lerp(from.section.span, to.section.span),
    };
    let time = lerp(from.time as f32, to.time as f32)
        .round()
        .clamp(from.time.min(to.time) as f32, from.time.max(to.time) as f32) as u32;
    Sample {
        time,
        camera,
        section,
    }
}

fn plausible_camera(camera: &CameraState) -> bool {
    camera.pivot.iter().all(|value| value.is_finite())
        && camera.yaw.is_finite()
        && camera.pitch.is_finite()
        && camera.distance.is_finite()
        && camera.distance > 0.0
        && camera.fov_y > 1.0f32.to_radians()
        && camera.fov_y < 179.0f32.to_radians()
}

fn plausible_section(section: &SectionState) -> bool {
    section.span.is_finite() && section.span > 0.0
}

fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(HASH_PRIME)
}
