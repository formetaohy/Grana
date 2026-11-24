use grana_gpu::Memory;

const FALLBACK: u64 = 512 << 20;

const FLOOR: u64 = 256 << 20;

const CEILING: u64 = 4 << 30;

const DISCRETE_SHARE: u64 = 3;

const INTEGRATED_SHARE: u64 = 8;

const SCENE_BYTES: u64 = 12;

const HISTORY_BYTES: u64 = 40;

const NORMAL_BYTES: u64 = 4;

const AO_BYTES: u64 = 1;

const SURFACE_BYTES: u64 = 16;

const VOLUME_BYTES: u64 = 24;

const FADE_BYTES: u64 = 8;

pub fn texture_budget(memory: Option<&Memory>) -> u64 {
    let Some(memory) = memory else {
        return FALLBACK;
    };
    let pool = memory.pool();
    let (pool, share) = if memory.integrated() {
        (
            pool.local().saturating_add(pool.non_local()),
            INTEGRATED_SHARE,
        )
    } else {
        (pool.local(), DISCRETE_SHARE)
    };
    (pool / share).clamp(FLOOR, CEILING)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frames {
    pub(crate) textures: u64,
    pub(crate) fragments: u64,
}

impl Frames {
    pub(crate) fn of(size: [u32; 2]) -> Self {
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let half = u64::from(size[0].div_ceil(2)) * u64::from(size[1].div_ceil(2));
        Self {
            textures: pixels
                * (SCENE_BYTES
                    + HISTORY_BYTES
                    + NORMAL_BYTES
                    + VOLUME_BYTES
                    + FADE_BYTES
                    + AO_BYTES)
                + half * (AO_BYTES + SURFACE_BYTES),
            fragments: crate::fragment::layered(size, crate::fragment::LAYERS),
        }
    }

    pub(crate) fn bytes(self) -> u64 {
        self.textures + self.fragments
    }
}

pub fn target_bytes(size: [u32; 2]) -> u64 {
    assert!(
        size[0] > 0 && size[1] > 0,
        "a frame target covers a non empty view"
    );
    Frames::of(size).bytes()
}

pub(crate) fn split(total: u64, size: [u32; 2]) -> Split {
    assert!(total > 0, "a renderer needs a positive texture budget");
    let single = Frames::of(size).bytes();
    let frames = single.min(total);
    match total.checked_sub(frames) {
        Some(residency) if residency > 0 => Split { residency, frames },
        _ => Split {
            residency: (total / 4).max(1),
            frames: single,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Split {
    pub(crate) residency: u64,
    pub(crate) frames: u64,
}

pub(crate) fn fragment_allowance(grant: u64, size: [u32; 2]) -> u64 {
    let lean = crate::fragment::layered(size, 1);
    grant.saturating_sub(Frames::of(size).textures).max(lean)
}
