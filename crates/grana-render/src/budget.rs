use grana_gpu::Memory;

const FALLBACK: u64 = 512 << 20;

const FLOOR: u64 = 256 << 20;

const CEILING: u64 = 4 << 30;

const DISCRETE_SHARE: u64 = 3;

const INTEGRATED_SHARE: u64 = 8;

const SCENE_BYTES: u64 = 12;

const HISTORY_BYTES: u64 = 40;

const FADE_BYTES: u64 = 8;

const NORMAL_BYTES: u64 = 4;

const AO_BYTES: u64 = 1;

const SURFACE_BYTES: u64 = 16;

const VOLUME_BYTES: u64 = 24;

const SPLAT_SHARE: u64 = 8;

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

pub fn splat_budget(total: u64) -> u64 {
    (total / SPLAT_SHARE).max(1)
}

pub(crate) fn splat_reservation(total: u64, demand: Demand) -> u64 {
    if demand.splats {
        splat_budget(total)
    } else {
        0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Demand {
    pub surfaces: bool,
    pub volume: bool,
    pub transparent: bool,
    pub splats: bool,
}

impl Demand {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn full() -> Self {
        Self {
            surfaces: true,
            volume: true,
            transparent: true,
            splats: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBudget {
    pub size: [u32; 2],
    pub demand: Demand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frames {
    pub(crate) textures: u64,
    pub(crate) fragments: u64,
}

impl Frames {
    pub(crate) fn of(size: [u32; 2], demand: Demand) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a frame target covers a non empty view"
        );
        let pixels = u64::from(size[0]) * u64::from(size[1]);
        let half = u64::from(size[0].div_ceil(2)) * u64::from(size[1].div_ceil(2));
        let mut textures = pixels * (SCENE_BYTES + HISTORY_BYTES + FADE_BYTES);
        if demand.volume {
            textures += pixels * VOLUME_BYTES;
        }
        if demand.surfaces {
            textures += pixels * (NORMAL_BYTES + AO_BYTES) + half * (AO_BYTES + SURFACE_BYTES);
        }
        let fragments = if demand.transparent {
            crate::fragment::layered(size, crate::fragment::LAYERS)
        } else {
            0
        };
        Self {
            textures,
            fragments,
        }
    }

    pub(crate) fn bytes(self) -> u64 {
        self.textures + self.fragments
    }
}

pub fn target_bytes(size: [u32; 2], demand: Demand) -> u64 {
    Frames::of(size, demand).bytes()
}

pub(crate) fn split(
    total: u64,
    ceiling: u64,
    size: [u32; 2],
    demand: Demand,
    held: u64,
    splats: u64,
) -> Result<Split, String> {
    assert!(total > 0, "a renderer needs a positive texture budget");
    assert!(ceiling > 0, "a renderer keeps a positive residency ceiling");
    let single = Frames::of(size, demand).bytes().max(held);
    if single.saturating_add(splats) >= total {
        return Err(format!(
            "a {} × {} view needs {:.1} MiB of frame targets and {:.1} MiB of splats, above the {:.1} MiB render budget; shorten the window or close running analyses",
            size[0],
            size[1],
            single as f64 / (1u64 << 20) as f64,
            splats as f64 / (1u64 << 20) as f64,
            total as f64 / (1u64 << 20) as f64
        ));
    }
    let residency = total
        .saturating_sub(single)
        .saturating_sub(splats)
        .min(ceiling);
    Ok(Split {
        residency,
        frames: single,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Split {
    pub(crate) residency: u64,
    pub(crate) frames: u64,
}

pub(crate) fn fragment_allowance(grant: u64, size: [u32; 2], demand: Demand) -> u64 {
    grant.saturating_sub(Frames::of(size, demand).textures)
}
