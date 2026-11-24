use std::ops::Range;
use std::sync::atomic::AtomicBool;

use grana_object::{IntensityTable, SampleAccumulator, Spot, SpotSet};
use grana_volume::{Intensity, Level, Pyramid, Region as VolumeRegion, VoxelData};

use grana_object::{Mask, SpotParams};

use crate::budget::Budget;
use crate::channel::ChannelSet;
use crate::detection::{DetectionImage, Plan as DetectionPlan};
use crate::local::BlockField;
use crate::schedule::Delivery;

const FWHM_PER_SIGMA: f32 = 2.354_82;
const MIN_SIGMA: f32 = 0.5;
const SIGMA_FLOOR: f32 = 1.0;
const FIT_REACH: f32 = 1.5;
const FIT_REACH_LIMIT: i64 = 8;
const FIT_SAMPLES: f64 = 8.0;
const FIT_MINIMUM: f64 = 0.5;
const FIT_MAXIMUM: f64 = 2.0;
const CORE_MIN: u32 = 64;
const CORE_MAX: u32 = 256;
const CORE_STEP: u32 = 32;
const CORE_VOXELS_PER_SIGMA: f32 = 8.0;
const PROGRESS_STRIDE: usize = 4;
const RESPONSE_REACH: u32 = 2;

pub type ChannelRead<'a> = dyn Fn(usize, &[Range<u32>; 3]) -> Result<VoxelData, String> + Sync + 'a;

type Region = [[usize; 2]; 3];

pub fn spot_level(
    pyramid: &Pyramid,
    region: VolumeRegion,
    params: SpotParams,
    budget: Budget,
) -> Result<usize, String> {
    let sigma = params.diameter() / FWHM_PER_SIGMA;
    let resolvable = (0..pyramid.level_count())
        .rev()
        .find(|index| {
            let spacing = pyramid.level(*index).spacing();
            [spacing.x(), spacing.y(), spacing.z()]
                .into_iter()
                .all(|step| sigma / step >= SIGMA_FLOOR)
        })
        .unwrap_or(0);
    let affordable = pyramid
        .fit_level(region, budget.voxels())
        .unwrap_or(pyramid.level_count() - 1);
    Ok(affordable.min(resolvable))
}

pub struct SpotScanner {
    level_index: usize,
    channels: ChannelSet,
    budget: Budget,
    template: SpotScan,
}

impl SpotScanner {
    pub fn new(
        level_index: usize,
        level: Level,
        intensity: Intensity,
        params: SpotParams,
        region: VolumeRegion,
        channels: ChannelSet,
        budget: Budget,
    ) -> Self {
        Self {
            level_index,
            channels,
            budget,
            template: SpotScan::new(level, intensity, params, region, budget),
        }
    }

    pub fn blocks(&self) -> usize {
        self.template.block_count()
    }

    pub fn frame(
        &self,
        cancel: &AtomicBool,
        roi: Option<&Mask>,
        read: &ChannelRead<'_>,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<Option<SpotSet>, String> {
        let channels = self.channels.count();
        let detection = self.channels.detection();
        let total = self.template.block_count();
        if total == 0 {
            return Ok(Some(SpotSet::new(
                self.level_index,
                Vec::new(),
                IntensityTable::new(channels, Vec::new()),
            )));
        }
        let workers = self
            .budget
            .workers()
            .min(self.budget.window(self.template.worker_bytes()));
        let detection_read = |ranges: &[Range<u32>; 3]| read(detection, ranges);
        let plan = DetectionPlan {
            cover: self.template.cover(),
            kernels: &self.template.kernels,
            anchor: self.template.field_anchor(),
            region: self.template.ranges.clone(),
            intensity: self.template.intensity,
            no_data: self.channels.no_data(detection),
            budget: self.budget,
            cancel,
            read: &detection_read,
        };
        let bands = plan.bands();
        let step = total + bands;
        let Some(build) = plan.build(&mut |done, _| progress(done, step))? else {
            return Ok(None);
        };
        let mut spots = Vec::new();
        let mut accumulators = Vec::new();
        let mut done = 0usize;
        let completed = crate::schedule::ordered(
            crate::schedule::Run {
                total,
                workers,
                cancel,
            },
            workers,
            || SpotWorker::new(&self.template, self.channels.clone(), roi),
            |worker, index, _| worker.run(index, &build.image, &build.field, read),
            |delivery| match delivery {
                Delivery::Progress { .. } => Ok(()),
                Delivery::Unit { value: block, .. } => {
                    spots.extend(block.spots);
                    accumulators.extend(block.accumulators);
                    done += 1;
                    if done.is_multiple_of(PROGRESS_STRIDE) || done == total {
                        progress(bands + done, step);
                    }
                    Ok(())
                }
            },
            |_| {},
        )?;
        if !completed {
            return Ok(None);
        }
        let intensities = accumulators
            .into_iter()
            .map(SampleAccumulator::statistics)
            .collect::<Vec<_>>();
        Ok(Some(SpotSet::new(
            self.level_index,
            spots,
            IntensityTable::new(channels, intensities),
        )))
    }
}

struct SpotWorker<'a> {
    scan: SpotScan,
    channels: ChannelSet,
    roi: Option<&'a Mask>,
    values: Vec<f32>,
    spots: Vec<Spot>,
    accumulators: Vec<SampleAccumulator>,
}

struct SpotBlock {
    spots: Vec<Spot>,
    accumulators: Vec<SampleAccumulator>,
}

impl<'a> SpotWorker<'a> {
    fn new(template: &SpotScan, channels: ChannelSet, roi: Option<&'a Mask>) -> Self {
        Self {
            scan: template.worker(),
            channels,
            roi,
            values: Vec::new(),
            spots: Vec::new(),
            accumulators: Vec::new(),
        }
    }

    fn run(
        &mut self,
        index: usize,
        image: &DetectionImage,
        field: &BlockField,
        read: &ChannelRead<'_>,
    ) -> Result<SpotBlock, String> {
        let channels = self.channels.count();
        let detection = self.channels.detection();
        let range = self.scan.read_range(index);
        self.load(detection, &range, read)?;
        self.spots.clear();
        self.spots
            .extend(self.scan.detect(index, image, field, self.roi)?);
        self.accumulators.clear();
        self.accumulators
            .resize(self.spots.len() * channels, SampleAccumulator::new());
        let measured = self.scan.measure(index, &self.spots, &self.values);
        for (slot, accumulator) in measured.into_iter().enumerate() {
            self.accumulators[slot * channels + detection] = accumulator;
        }
        for channel in 0..channels {
            if channel == detection {
                continue;
            }
            self.load(channel, &range, read)?;
            let measured = self.scan.measure(index, &self.spots, &self.values);
            for (slot, accumulator) in measured.into_iter().enumerate() {
                self.accumulators[slot * channels + channel] = accumulator;
            }
        }
        Ok(SpotBlock {
            spots: std::mem::take(&mut self.spots),
            accumulators: std::mem::take(&mut self.accumulators),
        })
    }

    fn load(
        &mut self,
        channel: usize,
        range: &[Range<u32>; 3],
        read: &ChannelRead<'_>,
    ) -> Result<(), String> {
        let data = read(channel, range)?;
        self.values.clear();
        self.values.resize(data.voxel_count(), 0.0);
        data.write_values_absent_as_nan(self.channels.no_data(channel), &mut self.values);
        Ok(())
    }
}

struct SpotScan {
    extent: [u32; 3],
    level: Level,
    spacing: [f32; 3],
    sigma_world: f32,
    halo: [u32; 3],
    core_side: u32,
    first: [u32; 3],
    counts: [u32; 3],
    ranges: [Range<u32>; 3],
    weights: [f32; 3],
    kernels: [Vec<f32>; 3],
    threshold: f32,
    intensity: Intensity,
    span: f32,
    radius: f32,
    blurred: Vec<f32>,
    response: Vec<f32>,
}

impl SpotScan {
    fn new(
        level: Level,
        intensity: Intensity,
        params: SpotParams,
        region: VolumeRegion,
        budget: Budget,
    ) -> Self {
        let span = intensity.span();
        assert!(
            span.is_finite() && span > 0.0,
            "a spot scan needs a positive intensity span"
        );
        let extent = level.extent();
        let extent = [extent.x(), extent.y(), extent.z()];
        let spacing = level.spacing();
        let spacing = [spacing.x(), spacing.y(), spacing.z()];
        let sigma_world = params.diameter() / FWHM_PER_SIGMA;
        let sigma = spacing.map(|step| (sigma_world / step).max(MIN_SIGMA));
        let truncate = sigma.map(|value| (3.0 * value).ceil() as usize);
        let halo = truncate.map(|value| value as u32 + RESPONSE_REACH);
        let maximum = sigma.iter().copied().fold(0.0f32, f32::max);
        let core_side = ((CORE_VOXELS_PER_SIGMA * maximum).ceil() as u32)
            .clamp(CORE_MIN, CORE_MAX)
            .div_ceil(CORE_STEP)
            * CORE_STEP;
        let (ranges, first, counts) = match region.voxel_range(level) {
            Some(ranges) => {
                let first = [0, 1, 2].map(|axis| ranges[axis].start / core_side);
                let last = [0, 1, 2].map(|axis| (ranges[axis].end - 1) / core_side);
                let counts = [0, 1, 2].map(|axis| last[axis] - first[axis] + 1);
                (ranges, first, counts)
            }
            None => ([0..0, 0..0, 0..0], [0; 3], [0; 3]),
        };
        if counts.iter().all(|count| *count > 0) {
            let maximum_read = (0..3)
                .map(|axis| u64::from(extent[axis].min(core_side) + 2 * halo[axis]))
                .product::<u64>();
            assert!(
                maximum_read <= budget.read_voxels(),
                "the spot diameter needs more than {} voxels per block",
                budget.read_voxels()
            );
        }
        let kernels = [0, 1, 2].map(|axis| gaussian(sigma[axis], truncate[axis]));
        let weights = spacing.map(|step| 1.0 / (step * step));
        Self {
            extent,
            level,
            spacing,
            sigma_world,
            halo,
            core_side,
            first,
            counts,
            ranges,
            weights,
            kernels,
            threshold: params.threshold(),
            intensity,
            span,
            radius: 0.5 * params.diameter(),
            blurred: Vec::new(),
            response: Vec::new(),
        }
    }

    fn block_count(&self) -> usize {
        self.counts[0] as usize * self.counts[1] as usize * self.counts[2] as usize
    }

    fn cover(&self) -> [Range<u32>; 3] {
        [0, 1, 2].map(|axis| {
            let start = self.ranges[axis].start.saturating_sub(self.halo[axis]);
            let end = self.ranges[axis]
                .end
                .saturating_add(self.halo[axis])
                .min(self.extent[axis]);
            start..end
        })
    }

    fn field_anchor(&self) -> [i64; 3] {
        [
            i64::from(self.ranges[0].start),
            i64::from(self.ranges[1].start),
            i64::from(self.ranges[2].start),
        ]
    }

    fn worker_bytes(&self) -> u64 {
        let voxel = std::mem::size_of::<f32>() as u64;
        let mut read = 0u64;
        let mut span = 0u64;
        for index in 0..self.block_count() {
            let range = self.read_range(index);
            read = read.max(
                (0..3)
                    .map(|axis| u64::from(range[axis].end - range[axis].start))
                    .product::<u64>(),
            );
            let core = self.core(index);
            span = span.max(
                (0..3)
                    .map(|axis| {
                        u64::from(core[axis].end - core[axis].start) + 2 * u64::from(RESPONSE_REACH)
                    })
                    .product::<u64>(),
            );
        }
        (read + 2 * span) * voxel
    }

    fn worker(&self) -> Self {
        Self {
            extent: self.extent,
            level: self.level,
            spacing: self.spacing,
            sigma_world: self.sigma_world,
            halo: self.halo,
            core_side: self.core_side,
            first: self.first,
            counts: self.counts,
            ranges: self.ranges.clone(),
            weights: self.weights,
            kernels: self.kernels.clone(),
            threshold: self.threshold,
            intensity: self.intensity,
            span: self.span,
            radius: self.radius,
            blurred: Vec::new(),
            response: Vec::new(),
        }
    }

    fn background(&self, field: &BlockField, voxel: [f32; 3]) -> f32 {
        let step = crate::local::BLOCK as f32;
        let block = [0, 1, 2].map(|axis| {
            let anchor = self.ranges[axis].start as f32;
            ((voxel[axis] - anchor) / step).floor() as i64
        });
        field.at(block).0
    }

    fn read_range(&self, index: usize) -> [Range<u32>; 3] {
        let core = self.core(index);
        [0, 1, 2].map(|axis| {
            let start = core[axis].start as i64 - i64::from(self.halo[axis]);
            let end = core[axis].end as i64 + i64::from(self.halo[axis]);
            start.max(0) as u32..end.min(i64::from(self.extent[axis])) as u32
        })
    }

    fn detect(
        &mut self,
        index: usize,
        image: &DetectionImage,
        field: &BlockField,
        roi: Option<&Mask>,
    ) -> Result<Vec<Spot>, String> {
        let core = self.core(index);
        let patch = image.patch(&[0, 1, 2].map(|axis| {
            let start = core[axis].start.saturating_sub(RESPONSE_REACH);
            let end = core[axis]
                .end
                .saturating_add(RESPONSE_REACH)
                .min(self.extent[axis]);
            start..end
        }))?;
        let patch_dims = patch.dims();
        let patch_origin = [
            core[0].start.saturating_sub(RESPONSE_REACH),
            core[1].start.saturating_sub(RESPONSE_REACH),
            core[2].start.saturating_sub(RESPONSE_REACH),
        ];
        let dims = [0, 1, 2].map(|axis| (core[axis].end - core[axis].start) as usize + 4);
        let origin = [0, 1, 2].map(|axis| core[axis].start as i64 - i64::from(RESPONSE_REACH));
        let count = dims[0] * dims[1] * dims[2];
        self.blurred.clear();
        self.blurred.resize(count, 0.0);
        let source = patch.values();
        for z in 0..dims[2] {
            let voxel = (origin[2] + z as i64).clamp(0, i64::from(self.extent[2]) - 1);
            let read_z = (voxel - i64::from(patch_origin[2])) as usize;
            for y in 0..dims[1] {
                let voxel = (origin[1] + y as i64).clamp(0, i64::from(self.extent[1]) - 1);
                let read_y = (voxel - i64::from(patch_origin[1])) as usize;
                for x in 0..dims[0] {
                    let voxel = (origin[0] + x as i64).clamp(0, i64::from(self.extent[0]) - 1);
                    let read_x = (voxel - i64::from(patch_origin[0])) as usize;
                    self.blurred[x + dims[0] * (y + dims[1] * z)] =
                        source[read_x + patch_dims[0] * (read_y + patch_dims[1] * read_z)];
                }
            }
        }
        let response = [0, 1, 2].map(|axis| [1, dims[axis] - 1]);
        let strides = [1usize, dims[0], dims[0] * dims[1]];
        self.response.clear();
        self.response.resize(count, 0.0);
        for z in response[2][0]..response[2][1] {
            for y in response[1][0]..response[1][1] {
                let row = dims[0] * (y + dims[1] * z);
                for x in response[0][0]..response[0][1] {
                    let cell = x + row;
                    let center = self.blurred[cell];
                    let mut laplacian = 0.0;
                    for (weight, stride) in self.weights.iter().zip(strides) {
                        laplacian += weight
                            * (self.blurred[cell + stride] - 2.0 * center
                                + self.blurred[cell - stride]);
                    }
                    self.response[cell] =
                        -self.sigma_world * self.sigma_world * laplacian / self.span;
                }
            }
        }
        let mut spots = Vec::new();
        for z in core[2].clone() {
            for y in core[1].clone() {
                let mut field_row = field.row(crate::local::RowKey {
                    y: i64::from(y),
                    z: i64::from(z),
                });
                for x in core[0].clone() {
                    let base = (x - core[0].start) as usize
                        + RESPONSE_REACH as usize
                        + dims[0]
                            * ((y - core[1].start) as usize
                                + RESPONSE_REACH as usize
                                + dims[1]
                                    * ((z - core[2].start) as usize + RESPONSE_REACH as usize));
                    let value = self.response[base];
                    if !value.is_finite() || value <= 0.0 {
                        continue;
                    }
                    let mut best = f32::NEG_INFINITY;
                    let mut best_index = base;
                    for dz in -1i64..=1 {
                        for dy in -1i64..=1 {
                            for dx in -1i64..=1 {
                                let neighbour = (base as i64
                                    + dx * strides[0] as i64
                                    + dy * strides[1] as i64
                                    + dz * strides[2] as i64)
                                    as usize;
                                let candidate = self.response[neighbour];
                                if candidate > best {
                                    best = candidate;
                                    best_index = neighbour;
                                }
                            }
                        }
                    }
                    if best_index != base {
                        continue;
                    }
                    let contrast = field_row.contrast(i64::from(x), self.blurred[base]);
                    if !contrast.is_finite() || contrast < self.threshold {
                        continue;
                    }
                    let mut voxel = [x as f32, y as f32, z as f32];
                    for axis in 0..3 {
                        let step = strides[axis];
                        let minus = self.response[base - step];
                        let plus = self.response[base + step];
                        let denominator = minus - 2.0 * value + plus;
                        if denominator < -1.0e-12 {
                            voxel[axis] += (0.5 * (minus - plus) / denominator).clamp(-0.5, 0.5);
                        }
                    }
                    let center = self.level.voxel_center(voxel);
                    if roi.is_some_and(|roi| !roi.contains(center)) {
                        continue;
                    }
                    let radius = self.fit_radius(
                        &origin,
                        &dims,
                        &response,
                        voxel,
                        self.background(field, voxel),
                    );
                    spots.push(Spot::new(center, radius, contrast.clamp(0.0, 1.0)));
                }
            }
        }
        Ok(spots)
    }

    fn measure(&self, index: usize, spots: &[Spot], values: &[f32]) -> Vec<SampleAccumulator> {
        let read = self.read_range(index);
        let dims = dimensions(&read);
        assert_eq!(
            values.len(),
            dims[0] * dims[1] * dims[2],
            "the block values cover the measured region"
        );
        let origin = [read[0].start, read[1].start, read[2].start];
        spots
            .iter()
            .map(|spot| {
                let center = spot.center();
                let ellipsoid = spot.ellipsoid();
                let radii = ellipsoid.radii();
                let mut accumulator = SampleAccumulator::new();
                let mut low = [0u32; 3];
                let mut high = [0u32; 3];
                for axis in 0..3 {
                    let voxel = self.level.voxel_of(center)[axis] - 0.5;
                    let reach = radii[axis] / self.spacing[axis];
                    let minimum = origin[axis] as f32;
                    let maximum = (origin[axis] + dims[axis] as u32 - 1) as f32;
                    low[axis] = (voxel - reach).floor().clamp(minimum, maximum) as u32;
                    high[axis] = (voxel + reach).ceil().clamp(minimum, maximum) as u32;
                }
                let mut sampled = false;
                for z in low[2]..=high[2] {
                    for y in low[1]..=high[1] {
                        for x in low[0]..=high[0] {
                            let world = self.level.voxel_center([x as f32, y as f32, z as f32]);
                            if !ellipsoid.contains(world) {
                                continue;
                            }
                            let index = (x - origin[0]) as usize
                                + dims[0]
                                    * ((y - origin[1]) as usize
                                        + dims[1] * (z - origin[2]) as usize);
                            accumulator.push(values[index]);
                            sampled = true;
                        }
                    }
                }
                if !sampled {
                    let mut index = 0;
                    for axis in 0..3 {
                        let voxel = self.level.voxel_of(center)[axis] - 0.5;
                        let minimum = origin[axis] as f32;
                        let maximum = (origin[axis] + dims[axis] as u32 - 1) as f32;
                        let nearest = voxel.round().clamp(minimum, maximum) as usize;
                        index += (nearest - origin[axis] as usize) * strides_for(&dims, axis);
                    }
                    accumulator.push(values[index]);
                }
                accumulator
            })
            .collect()
    }

    fn fit_radius(
        &self,
        origin: &[i64; 3],
        desired: &[usize; 3],
        response: &Region,
        voxel: [f32; 3],
        background: f32,
    ) -> f32 {
        let peak = [0, 1, 2].map(|axis| voxel[axis].round() as i64);
        let reach = [0, 1, 2].map(|axis| {
            let sigma = (self.sigma_world / self.spacing[axis]).max(MIN_SIGMA);
            ((FIT_REACH * sigma).ceil() as i64).clamp(1, FIT_REACH_LIMIT)
        });
        let center = self.level.voxel_center(voxel);
        let mut count = 0.0f64;
        let mut sum = 0.0f64;
        let mut logs = 0.0f64;
        let mut squares = 0.0f64;
        let mut crosses = 0.0f64;
        for dz in -reach[2]..=reach[2] {
            let z = peak[2] + dz;
            let local_z = z - origin[2];
            if local_z < response[2][0] as i64 || local_z >= response[2][1] as i64 {
                continue;
            }
            for dy in -reach[1]..=reach[1] {
                let y = peak[1] + dy;
                let local_y = y - origin[1];
                if local_y < response[1][0] as i64 || local_y >= response[1][1] as i64 {
                    continue;
                }
                for dx in -reach[0]..=reach[0] {
                    let x = peak[0] + dx;
                    let local_x = x - origin[0];
                    if local_x < response[0][0] as i64 || local_x >= response[0][1] as i64 {
                        continue;
                    }
                    let index = local_x as usize
                        + desired[0] * (local_y as usize + desired[1] * local_z as usize);
                    let value = self.blurred[index] - background;
                    if !value.is_finite() || value <= 0.0 {
                        continue;
                    }
                    let sample = self.level.voxel_center([x as f32, y as f32, z as f32]);
                    let squared = (0..3)
                        .map(|axis| f64::from(sample[axis] - center[axis]).powi(2))
                        .sum::<f64>();
                    let logarithm = f64::from(value.ln());
                    count += 1.0;
                    sum += squared;
                    logs += logarithm;
                    squares += squared * squared;
                    crosses += squared * logarithm;
                }
            }
        }
        let denominator = count * squares - sum * sum;
        if count < FIT_SAMPLES || denominator.abs() <= 1.0e-12 {
            return self.radius;
        }
        let slope = (count * crosses - sum * logs) / denominator;
        if slope >= 0.0 {
            return self.radius;
        }
        let fitted = -0.5 / slope - f64::from(self.sigma_world) * f64::from(self.sigma_world);
        if fitted <= 0.0 {
            return self.radius;
        }
        let diameter = FWHM_PER_SIGMA as f64 * fitted.sqrt();
        let nominal = 2.0 * f64::from(self.radius);
        (0.5 * diameter.clamp(FIT_MINIMUM * nominal, FIT_MAXIMUM * nominal)) as f32
    }

    fn core(&self, index: usize) -> [Range<u32>; 3] {
        let x = index % self.counts[0] as usize;
        let y = (index / self.counts[0] as usize) % self.counts[1] as usize;
        let z = index / (self.counts[0] as usize * self.counts[1] as usize);
        assert!(
            z < self.counts[2] as usize,
            "the spot scan block index is out of bounds"
        );
        [0, 1, 2].map(|axis| {
            let cell = self.first[axis] + [x, y, z][axis] as u32;
            let start = (cell * self.core_side).max(self.ranges[axis].start);
            let end = (cell * self.core_side + self.core_side).min(self.ranges[axis].end);
            start..end
        })
    }
}

fn dimensions(ranges: &[Range<u32>; 3]) -> [usize; 3] {
    [
        (ranges[0].end - ranges[0].start) as usize,
        (ranges[1].end - ranges[1].start) as usize,
        (ranges[2].end - ranges[2].start) as usize,
    ]
}

fn strides_for(dims: &[usize; 3], axis: usize) -> usize {
    match axis {
        0 => 1,
        1 => dims[0],
        _ => dims[0] * dims[1],
    }
}

fn gaussian(sigma: f32, truncate: usize) -> Vec<f32> {
    let mut kernel = (0..=2 * truncate)
        .map(|index| {
            let offset = index as f32 - truncate as f32;
            (-0.5 * (offset / sigma) * (offset / sigma)).exp()
        })
        .collect::<Vec<_>>();
    let total: f32 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    kernel
}
