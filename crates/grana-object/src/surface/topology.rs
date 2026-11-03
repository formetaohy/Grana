use std::any::Any;
use std::sync::Arc;

use grana_codec::{Decoder, Encoder, FAULT_VALUE, Fault};
use grana_geometry::{Decode, Payload};

use crate::intensity::{AccumulatorParts, SampleAccumulator};

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

    pub fn to_bytes(&self) -> Vec<u8> {
        let info = self.info;
        let mut bytes = Vec::with_capacity(self.stored_bytes());
        let mut writer = Encoder::memory(&mut bytes);
        writer.count(info.level());
        writer.count(info.channels());
        writer.count(info.detection());
        for value in info.spacing().into_iter().chain(info.world()) {
            writer.f32(value);
        }
        writer.f64(info.voxel_volume());
        writer.f32(info.span());
        writer.count(info.seeds());
        writer.count(self.basins.len());
        for basin in &self.basins {
            writer.u32(basin.reach());
            writer.u64(basin.voxels());
            for value in basin.sum() {
                writer.f64(value);
            }
            for value in basin.low().into_iter().chain(basin.high()) {
                writer.u32(value);
            }
            writer.u32(basin.contact().unwrap_or(u32::MAX));
            writer.u32(basin.seed().unwrap_or(u32::MAX));
        }
        writer.count(self.accumulators.len());
        for accumulator in &self.accumulators {
            let parts = accumulator.parts();
            writer.u64(parts.count);
            writer.f64(parts.mean);
            writer.f64(parts.squared);
            writer.f32(parts.minimum);
            writer.f32(parts.maximum);
            writer.f64(parts.sum);
        }
        writer.count(self.seams.len());
        for seam in &self.seams {
            writer.u32(seam.left());
            writer.u32(seam.right());
            writer.u32(seam.saddle());
        }
        let patches = &self.patches;
        writer.count(patches.positions().len());
        writer.count(patches.normals().len());
        writer.count(patches.triangles().len());
        writer.count(patches.basins().len());
        for position in patches.positions() {
            for value in position {
                writer.f32(*value);
            }
        }
        for normal in patches.normals() {
            for value in normal {
                writer.f32(*value);
            }
        }
        for triangle in patches.triangles() {
            for index in triangle {
                writer.u32(*index);
            }
        }
        for basin in patches.basins() {
            writer.u32(*basin);
        }
        drop(writer);
        assert_eq!(
            bytes.len(),
            self.stored_bytes(),
            "the surface topology encodes its own size"
        );
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Self::decode(bytes).ok()
    }

    fn decode(bytes: &[u8]) -> Result<Self, Fault> {
        let mut reader = Decoder::memory(bytes);
        let level = reader.count(1)?;
        let channels = reader.count(1)?;
        let detection = reader.count(1)?;
        if channels == 0 || detection >= channels {
            return Err(FAULT_VALUE);
        }
        let mut spacing = [0.0f32; 3];
        let mut world = [0.0f32; 3];
        for value in &mut spacing {
            *value = finite(reader.f32()?)?;
            if *value <= 0.0 {
                return Err(FAULT_VALUE);
            }
        }
        for value in &mut world {
            *value = finite(reader.f32()?)?;
            if *value <= 0.0 {
                return Err(FAULT_VALUE);
            }
        }
        let voxel_volume = reader.f64()?;
        if !voxel_volume.is_finite() || voxel_volume <= 0.0 {
            return Err(FAULT_VALUE);
        }
        let span = reader.f32()?;
        if !span.is_finite() || span < 0.0 {
            return Err(FAULT_VALUE);
        }
        let seeds = reader.count(1)?;
        let basin_count = reader.count(BASIN_BYTES)?;
        if basin_count as u64 >= u64::from(u32::MAX) {
            return Err(FAULT_VALUE);
        }
        let mut basins = Vec::with_capacity(basin_count);
        for _ in 0..basin_count {
            let reach = reader.u32()?;
            let voxels = reader.u64()?;
            let mut sum = [0.0f64; 3];
            for value in &mut sum {
                *value = reader.f64()?;
                if !value.is_finite() {
                    return Err(FAULT_VALUE);
                }
            }
            let mut low = [0u32; 3];
            let mut high = [0u32; 3];
            for value in &mut low {
                *value = reader.u32()?;
            }
            for value in &mut high {
                *value = reader.u32()?;
            }
            let contact = reader.u32()?;
            let seed = reader.u32()?;
            basins.push(Basin::new(
                reach,
                voxels,
                sum,
                low,
                high,
                (contact != u32::MAX).then_some(contact),
                (seed != u32::MAX).then_some(seed),
            ));
        }
        let accumulator_count = reader.count(ACCUMULATOR_BYTES)?;
        if accumulator_count != basin_count.checked_mul(channels).ok_or(FAULT_VALUE)? {
            return Err(FAULT_VALUE);
        }
        let mut accumulators = Vec::with_capacity(accumulator_count);
        for _ in 0..accumulator_count {
            let accumulator = SampleAccumulator::restore(AccumulatorParts {
                count: reader.u64()?,
                mean: reader.f64()?,
                squared: reader.f64()?,
                minimum: reader.f32()?,
                maximum: reader.f32()?,
                sum: reader.f64()?,
            })
            .ok_or(FAULT_VALUE)?;
            accumulators.push(accumulator);
        }
        let seam_count = reader.count(SEAM_BYTES)?;
        let mut seams = Vec::with_capacity(seam_count);
        for _ in 0..seam_count {
            let left = reader.u32()?;
            let right = reader.u32()?;
            let saddle = reader.u32()?;
            if left >= right || right as usize >= basin_count {
                return Err(FAULT_VALUE);
            }
            seams.push(Seam::new(left, right, saddle));
        }
        let positions = reader.count(3 * size_of::<f32>())?;
        let normals = reader.count(3 * size_of::<f32>())?;
        let triangles = reader.count(3 * size_of::<u32>())?;
        let tagged = reader.count(size_of::<u32>())?;
        if positions != normals || triangles != tagged {
            return Err(FAULT_VALUE);
        }
        let mut decoded_positions = Vec::with_capacity(positions);
        for _ in 0..positions {
            let mut values = [0.0f32; 3];
            for value in &mut values {
                *value = finite(reader.f32()?)?;
            }
            decoded_positions.push(values);
        }
        let mut decoded_normals = Vec::with_capacity(normals);
        for _ in 0..normals {
            let mut values = [0.0f32; 3];
            for value in &mut values {
                *value = finite(reader.f32()?)?;
            }
            decoded_normals.push(values);
        }
        let mut decoded_triangles = Vec::with_capacity(triangles);
        for _ in 0..triangles {
            let mut values = [0u32; 3];
            for value in &mut values {
                *value = reader.u32()?;
                if *value as usize >= positions {
                    return Err(FAULT_VALUE);
                }
            }
            decoded_triangles.push(values);
        }
        let mut tagged_basins = Vec::with_capacity(tagged);
        for _ in 0..tagged {
            let basin = reader.u32()?;
            if basin as usize >= basin_count {
                return Err(FAULT_VALUE);
            }
            tagged_basins.push(basin);
        }
        reader.done()?;
        let patches = SurfacePatches::new(
            decoded_positions,
            decoded_normals,
            decoded_triangles,
            tagged_basins,
        );
        Ok(Self::new(
            TopologyInfo::new(level, channels, detection, spacing, world).with_sampling(
                voxel_volume,
                span,
                seeds,
            ),
            basins,
            accumulators,
            seams,
            patches,
        ))
    }

    fn stored_bytes(&self) -> usize {
        let patches = &self.patches;
        INFO_BYTES
            + 3 * size_of::<u32>()
            + self.basins.len() * BASIN_BYTES
            + self.accumulators.len() * ACCUMULATOR_BYTES
            + self.seams.len() * SEAM_BYTES
            + 4 * size_of::<u32>()
            + size_of_val(patches.positions())
            + size_of_val(patches.normals())
            + size_of_val(patches.triangles())
            + size_of_val(patches.basins())
    }
}

const INFO_BYTES: usize =
    4 * size_of::<u32>() + 6 * size_of::<f32>() + size_of::<f64>() + size_of::<f32>();
const BASIN_BYTES: usize = size_of::<u32>()
    + size_of::<u64>()
    + 3 * size_of::<f64>()
    + 6 * size_of::<u32>()
    + 2 * size_of::<u32>();
const ACCUMULATOR_BYTES: usize =
    size_of::<u64>() + 2 * size_of::<f64>() + 2 * size_of::<f32>() + size_of::<f64>();
const SEAM_BYTES: usize = 3 * size_of::<u32>();

fn finite(value: f32) -> Result<f32, Fault> {
    value.is_finite().then_some(value).ok_or(FAULT_VALUE)
}

impl Payload for SurfaceTopology {
    fn encode(&self) -> Vec<u8> {
        self.to_bytes()
    }

    fn stored_bytes(&self) -> u64 {
        SurfaceTopology::stored_bytes(self) as u64
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

impl Decode for SurfaceTopology {
    fn decode(bytes: &[u8]) -> Option<Self> {
        Self::from_bytes(bytes)
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
            .field("bytes", &self.stored_bytes())
            .finish()
    }
}
