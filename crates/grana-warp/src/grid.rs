use std::sync::Arc;

const LIMIT: f32 = 1_048_576.0;
const DIGEST: u64 = 0xcbf2_9ce4_8422_2325;
const DIGEST_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Debug)]
pub struct Grid {
    dims: [u32; 3],
    origin: [f32; 3],
    step: [f32; 3],
    values: Arc<[[f32; 3]]>,
}

impl Grid {
    pub fn new(dims: [u32; 3], origin: [f32; 3], step: [f32; 3], values: Vec<[f32; 3]>) -> Self {
        assert!(
            dims.iter().all(|axis| *axis > 0),
            "a displacement grid spans at least one node per axis"
        );
        assert!(
            origin
                .iter()
                .all(|value| value.is_finite() && value.abs() <= LIMIT),
            "a displacement grid starts at a finite world point"
        );
        assert!(
            step.iter().all(|value| value.is_finite() && *value != 0.0),
            "a displacement grid steps by a nonzero world distance"
        );
        assert_eq!(
            values.len(),
            (dims[0] * dims[1] * dims[2]) as usize,
            "a displacement grid holds one offset per node"
        );
        assert!(
            values
                .iter()
                .flatten()
                .all(|value| value.is_finite() && value.abs() <= LIMIT),
            "a displacement grid holds finite offsets"
        );
        Self {
            dims,
            origin,
            step,
            values: values.into(),
        }
    }

    pub fn sampled(
        dims: [u32; 3],
        origin: [f32; 3],
        step: [f32; 3],
        sample: impl Fn([f32; 3]) -> [f32; 3],
    ) -> Self {
        let mut values = Vec::with_capacity((dims[0] * dims[1] * dims[2]) as usize);
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    values.push(sample([
                        origin[0] + x as f32 * step[0],
                        origin[1] + y as f32 * step[1],
                        origin[2] + z as f32 * step[2],
                    ]));
                }
            }
        }
        Self::new(dims, origin, step, values)
    }

    pub fn dims(&self) -> [u32; 3] {
        self.dims
    }

    pub fn origin(&self) -> [f32; 3] {
        self.origin
    }

    pub fn step(&self) -> [f32; 3] {
        self.step
    }

    pub fn values(&self) -> &[[f32; 3]] {
        &self.values
    }

    pub fn sample(&self, point: [f32; 3]) -> [f32; 3] {
        let local = [0, 1, 2].map(|axis| {
            let position = (point[axis] - self.origin[axis]) / self.step[axis];
            position.clamp(0.0, (self.dims[axis] - 1) as f32)
        });
        let low = local.map(|value| value.floor() as u32);
        let high = [0, 1, 2].map(|axis| (low[axis] + 1).min(self.dims[axis] - 1));
        let weight = [0, 1, 2].map(|axis| local[axis] - low[axis] as f32);
        let node = |corner: usize| {
            [
                if corner & 1 == 0 { low[0] } else { high[0] },
                if corner & 2 == 0 { low[1] } else { high[1] },
                if corner & 4 == 0 { low[2] } else { high[2] },
            ]
        };
        let mut sum = [0.0f32; 3];
        for corner in 0..8usize {
            let mut factor = 1.0f32;
            for (axis, component) in weight.iter().enumerate() {
                factor *= if (corner >> axis) & 1 == 1 {
                    *component
                } else {
                    1.0 - *component
                };
            }
            if factor == 0.0 {
                continue;
            }
            let index = node(corner);
            let value = self.values
                [(index[0] + self.dims[0] * (index[1] + self.dims[1] * index[2])) as usize];
            for (sum, component) in sum.iter_mut().zip(value) {
                *sum += component * factor;
            }
        }
        sum
    }

    pub fn magnitude(&self) -> f32 {
        self.values
            .iter()
            .flatten()
            .map(|value| value.abs())
            .fold(0.0, f32::max)
    }

    pub fn translation(&self) -> [f32; 3] {
        let count = self.values.len() as f64;
        let mut sum = [0.0f64; 3];
        for value in self.values.iter() {
            for axis in 0..3 {
                sum[axis] += f64::from(value[axis]);
            }
        }
        sum.map(|value| (value / count) as f32)
    }

    pub fn offset(&self, delta: [f32; 3]) -> Self {
        let values = self
            .values
            .iter()
            .map(|value| [0, 1, 2].map(|axis| value[axis] + delta[axis]))
            .collect();
        Self::new(self.dims, self.origin, self.step, values)
    }

    pub fn map(&self, transform: impl Fn([f32; 3]) -> [f32; 3]) -> Self {
        let values = self.values.iter().map(|value| transform(*value)).collect();
        Self::new(self.dims, self.origin, self.step, values)
    }

    pub fn combine(&self, other: &Self, blend: impl Fn([f32; 3], [f32; 3]) -> [f32; 3]) -> Self {
        assert!(
            self.same_lattice(other),
            "a displacement grid combines with a grid of its own lattice"
        );
        let values = self
            .values
            .iter()
            .zip(other.values.iter())
            .map(|(left, right)| blend(*left, *right))
            .collect();
        Self::new(self.dims, self.origin, self.step, values)
    }

    pub fn same_lattice(&self, other: &Self) -> bool {
        self.dims == other.dims && self.origin == other.origin && self.step == other.step
    }

    pub fn digest(&self) -> u64 {
        let mut digest = DIGEST;
        for axis in 0..3 {
            digest = mix(digest, u64::from(self.dims[axis]));
            digest = mix(digest, u64::from(self.origin[axis].to_bits()));
            digest = mix(digest, u64::from(self.step[axis].to_bits()));
        }
        for value in self.values.iter() {
            for component in value {
                digest = mix(digest, u64::from(component.to_bits()));
            }
        }
        digest
    }
}

impl PartialEq for Grid {
    fn eq(&self, other: &Self) -> bool {
        self.dims == other.dims
            && self.origin == other.origin
            && self.step == other.step
            && self.values == other.values
    }
}

pub(crate) fn mix(digest: u64, value: u64) -> u64 {
    (digest ^ value).wrapping_mul(DIGEST_PRIME)
}

pub(crate) const DIGEST_BASIS: u64 = DIGEST;
