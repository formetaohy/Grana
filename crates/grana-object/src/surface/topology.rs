use crate::intensity::SampleAccumulator;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basin {
    reach: u32,
    voxels: u64,
    sum: [f64; 3],
    low: [u32; 3],
    high: [u32; 3],
    contact: Option<u32>,
    seed: Option<u32>,
}

impl Basin {
    pub fn new(
        reach: u32,
        voxels: u64,
        sum: [f64; 3],
        low: [u32; 3],
        high: [u32; 3],
        contact: Option<u32>,
        seed: Option<u32>,
    ) -> Self {
        Self {
            reach,
            voxels,
            sum,
            low,
            high,
            contact,
            seed,
        }
    }

    pub fn reach(self) -> u32 {
        self.reach
    }

    pub fn voxels(self) -> u64 {
        self.voxels
    }

    pub fn sum(self) -> [f64; 3] {
        self.sum
    }

    pub fn low(self) -> [u32; 3] {
        self.low
    }

    pub fn high(self) -> [u32; 3] {
        self.high
    }

    pub fn contact(self) -> Option<u32> {
        self.contact
    }

    pub fn seed(self) -> Option<u32> {
        self.seed
    }

    pub fn absorb(&mut self, other: Self) {
        self.reach = self.reach.max(other.reach);
        self.voxels += other.voxels;
        for axis in 0..3 {
            self.sum[axis] += other.sum[axis];
            self.low[axis] = self.low[axis].min(other.low[axis]);
            self.high[axis] = self.high[axis].max(other.high[axis]);
        }
        if self.contact.is_none() {
            self.contact = other.contact;
        }
        if let Some(seed) = other.seed {
            self.seed = Some(self.seed.map_or(seed, |current| current.min(seed)));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seam {
    left: u32,
    right: u32,
    saddle: u32,
}

impl Seam {
    pub fn new(left: u32, right: u32, saddle: u32) -> Self {
        assert!(left != right, "a seam joins two watershed basins");
        Self {
            left: left.min(right),
            right: left.max(right),
            saddle,
        }
    }

    pub fn left(self) -> u32 {
        self.left
    }

    pub fn right(self) -> u32 {
        self.right
    }

    pub fn saddle(self) -> u32 {
        self.saddle
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopologyInfo {
    level: usize,
    channels: usize,
    detection: usize,
    spacing: [f32; 3],
    world: [f32; 3],
    voxel_volume: f64,
    span: f32,
    seeds: usize,
}

impl TopologyInfo {
    pub fn new(
        level: usize,
        channels: usize,
        detection: usize,
        spacing: [f32; 3],
        world: [f32; 3],
    ) -> Self {
        assert!(channels > 0, "a surface measures at least one channel");
        assert!(
            detection < channels,
            "the detection channel is one of the measured channels"
        );
        assert!(
            spacing
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            "a surface voxel is a positive size on every axis"
        );
        assert!(
            world.iter().all(|value| value.is_finite() && *value > 0.0),
            "a surface extent is a positive length on every axis"
        );
        Self {
            level,
            channels,
            detection,
            spacing,
            world,
            voxel_volume: 1.0,
            span: 0.0,
            seeds: 0,
        }
    }

    pub fn with_sampling(mut self, voxel_volume: f64, span: f32, seeds: usize) -> Self {
        assert!(
            voxel_volume.is_finite() && voxel_volume > 0.0,
            "a surface voxel encloses a positive volume"
        );
        assert!(
            span.is_finite() && span >= 0.0,
            "a surface intensity span is a non negative range"
        );
        self.voxel_volume = voxel_volume;
        self.span = span;
        self.seeds = seeds;
        self
    }

    pub fn level(self) -> usize {
        self.level
    }

    pub fn channels(self) -> usize {
        self.channels
    }

    pub fn detection(self) -> usize {
        self.detection
    }

    pub fn spacing(self) -> [f32; 3] {
        self.spacing
    }

    pub fn world(self) -> [f32; 3] {
        self.world
    }

    pub fn voxel_volume(self) -> f64 {
        self.voxel_volume
    }

    pub fn span(self) -> f32 {
        self.span
    }

    pub fn seeds(self) -> usize {
        self.seeds
    }
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfacePatches {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    triangles: Vec<[u32; 3]>,
    basins: Vec<u32>,
}

impl SurfacePatches {
    pub fn new(
        positions: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        triangles: Vec<[u32; 3]>,
        basins: Vec<u32>,
    ) -> Self {
        assert_eq!(
            positions.len(),
            normals.len(),
            "every surface vertex carries a normal"
        );
        assert_eq!(
            triangles.len(),
            basins.len(),
            "every surface triangle names a watershed basin"
        );
        assert!(
            positions
                .iter()
                .chain(normals.iter())
                .flatten()
                .all(|value| value.is_finite()),
            "surface geometry is finite"
        );
        assert!(
            triangles
                .iter()
                .flatten()
                .all(|index| (*index as usize) < positions.len()),
            "every surface triangle addresses a patch vertex"
        );
        Self {
            positions,
            normals,
            triangles,
            basins,
        }
    }

    pub fn positions(&self) -> &[[f32; 3]] {
        &self.positions
    }

    pub fn normals(&self) -> &[[f32; 3]] {
        &self.normals
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    pub fn basins(&self) -> &[u32] {
        &self.basins
    }

    pub fn bytes(&self) -> u64 {
        (4 * self.positions.len() * size_of::<f32>() + 3 * self.triangles.len() * size_of::<u32>())
            as u64
    }
}

pub struct SurfaceTopology {
    info: TopologyInfo,
    basins: Vec<Basin>,
    accumulators: Vec<SampleAccumulator>,
    seams: Vec<Seam>,
    patches: SurfacePatches,
}

impl SurfaceTopology {
    pub fn new(
        info: TopologyInfo,
        basins: Vec<Basin>,
        accumulators: Vec<SampleAccumulator>,
        seams: Vec<Seam>,
        patches: SurfacePatches,
    ) -> Self {
        assert!(
            (basins.len() as u64) < u64::from(u32::MAX),
            "a surface frame holds fewer basins than the empty sentinel"
        );
        assert_eq!(
            accumulators.len(),
            basins.len() * info.channels(),
            "every watershed basin is measured in every channel"
        );
        assert!(
            seams
                .iter()
                .all(|seam| (seam.right() as usize) < basins.len()),
            "a seam joins two watershed basins"
        );
        assert!(
            patches
                .basins()
                .iter()
                .all(|basin| (*basin as usize) < basins.len()),
            "every surface triangle names a watershed basin"
        );
        Self {
            info,
            basins,
            accumulators,
            seams,
            patches,
        }
    }

    pub fn info(&self) -> TopologyInfo {
        self.info
    }

    pub fn level(&self) -> usize {
        self.info.level()
    }

    pub fn basins(&self) -> &[Basin] {
        &self.basins
    }

    pub fn accumulators(&self) -> &[SampleAccumulator] {
        &self.accumulators
    }

    pub fn seams(&self) -> &[Seam] {
        &self.seams
    }

    pub fn patches(&self) -> &SurfacePatches {
        &self.patches
    }

    pub fn bytes(&self) -> u64 {
        (self.basins.len() * size_of::<Basin>()
            + self.accumulators.len() * size_of::<SampleAccumulator>()
            + self.seams.len() * size_of::<Seam>()) as u64
            + self.patches.bytes()
    }
}

impl std::fmt::Debug for SurfaceTopology {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceTopology")
            .field("level", &self.info.level())
            .field("basins", &self.basins.len())
            .field("seams", &self.seams.len())
            .field("triangles", &self.patches.triangles().len())
            .field("bytes", &self.bytes())
            .finish()
    }
}
