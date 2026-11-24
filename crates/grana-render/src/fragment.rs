use wgpu::Limits;

const COUNT_BYTES: u64 = 4;
const KEY_BYTES: u64 = 4;
const COLOR_BYTES: u64 = 8;
const SLOT_BYTES: u64 = KEY_BYTES + COLOR_BYTES;
const RESIDUAL_BYTES: u64 = 16;
const PIXEL_STEP: u64 = 1 << 17;
const LAYER_LIMIT: u64 = 8;

pub const LAYERS: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fragment {
    size: [u32; 2],
    capacity: u64,
    layers: u64,
}

impl Fragment {
    pub(crate) fn singleton() -> Self {
        Self {
            size: [1, 1],
            capacity: PIXEL_STEP,
            layers: 1,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn layers(&self) -> u32 {
        self.layers as u32
    }

    pub fn keys(&self) -> u64 {
        self.capacity * self.layers * KEY_BYTES
    }

    pub fn colors(&self) -> u64 {
        self.capacity * self.layers * COLOR_BYTES
    }

    pub fn counts(&self) -> u64 {
        self.capacity * COUNT_BYTES
    }

    pub fn bytes(&self) -> u64 {
        layered(self.size, self.layers)
    }
}

fn capacity(size: [u32; 2]) -> u64 {
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    pixels.div_ceil(PIXEL_STEP) * PIXEL_STEP
}

pub(crate) fn layered(size: [u32; 2], layers: u64) -> u64 {
    assert!(
        (1..=LAYER_LIMIT).contains(&layers),
        "a fragment store holds one to {LAYER_LIMIT} layers"
    );
    assert!(
        size[0] > 0 && size[1] > 0,
        "a fragment store covers a non empty view"
    );
    let capacity = capacity(size);
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    capacity * layers * SLOT_BYTES + capacity * COUNT_BYTES + pixels * RESIDUAL_BYTES
}

pub fn fit(
    size: [u32; 2],
    allowance: u64,
    limits: &Limits,
    required: u64,
) -> Result<Fragment, String> {
    plan(size, allowance, limits, required, LAYER_LIMIT)
}

pub(crate) fn degrade(
    size: [u32; 2],
    allowance: u64,
    limits: &Limits,
    preferred: u64,
) -> Result<Fragment, String> {
    assert!(
        (1..=LAYER_LIMIT).contains(&preferred),
        "a fragment store prefers one to {LAYER_LIMIT} layers"
    );
    plan(size, allowance, limits, 1, preferred)
}

fn plan(
    size: [u32; 2],
    allowance: u64,
    limits: &Limits,
    required: u64,
    preferred: u64,
) -> Result<Fragment, String> {
    assert!(
        (1..=LAYER_LIMIT).contains(&required),
        "a fragment store plans one to {LAYER_LIMIT} layers"
    );
    assert!(
        size[0] > 0 && size[1] > 0,
        "a fragment store covers a non empty view"
    );
    let dimension = limits.max_texture_dimension_2d;
    if size[0] > dimension || size[1] > dimension {
        return Err(format!(
            "a {} × {} transparent view exceeds the {dimension} pixel limit of this device",
            size[0], size[1]
        ));
    }
    let pixels = u64::from(size[0]) * u64::from(size[1]);
    let capacity = capacity(size);
    let fixed = capacity * COUNT_BYTES + pixels * RESIDUAL_BYTES;
    let slot = capacity * SLOT_BYTES;
    let budgeted = allowance.saturating_sub(fixed) / slot;
    let addressable = limits
        .max_buffer_size
        .min(limits.max_storage_buffer_binding_size)
        / (capacity * COLOR_BYTES);
    let layers = budgeted.min(addressable).min(preferred).min(LAYER_LIMIT);
    if layers >= required {
        return Ok(Fragment {
            size,
            capacity,
            layers,
        });
    }
    let needed = fixed + required * slot;
    if addressable < required {
        Err(format!(
            "a {} × {} transparent view needs {:.0} MiB per buffer for {required} layers, above the {:.0} MiB buffer limit of this device",
            size[0],
            size[1],
            (capacity * COLOR_BYTES * required) as f64 / (1 << 20) as f64,
            limits
                .max_buffer_size
                .min(limits.max_storage_buffer_binding_size) as f64
                / (1 << 20) as f64,
        ))
    } else {
        Err(format!(
            "a {} × {} transparent view needs {:.0} MiB for {required} layers, above the {:.0} MiB fragment budget",
            size[0],
            size[1],
            needed as f64 / (1 << 20) as f64,
            allowance as f64 / (1 << 20) as f64,
        ))
    }
}
