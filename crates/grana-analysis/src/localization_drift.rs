use crate::hash::Map;

const SEGMENT_LIMIT: usize = 2048;
const MINIMUM_SEGMENT_FRAMES: usize = 25;
const SEARCH_REACH: f32 = 4.0;
const LATERAL_MINIMUM: f32 = 1.5e-2;
const LATERAL_MAXIMUM: f32 = 1.5e-1;
const AXIAL_MINIMUM: f32 = 3.0e-2;
const AXIAL_MAXIMUM: f32 = 3.0e-1;
const WIDEN_LIMIT: u32 = 4;
const ITERATIONS: usize = 8;
const CONVERGED: f32 = 1.0e-4;
const STALL: f32 = 1.5;
const MINIMUM_POINTS: usize = 100;
const MINIMUM_PAIRS: usize = 24;
const PAIR_SHARE: usize = 100;
const MINIMUM_SPAN: f32 = 1.0e-3;
const SAMPLE: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalizationPoint {
    center: [f32; 3],
    precision: [f32; 3],
}

impl LocalizationPoint {
    pub fn new(center: [f32; 3], precision: [f32; 3]) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a localization point lies at a finite center"
        );
        assert!(
            precision
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            "a localization point carries a positive precision"
        );
        Self { center, precision }
    }

    pub fn center(&self) -> [f32; 3] {
        self.center
    }

    pub fn precision(&self) -> [f32; 3] {
        self.precision
    }
}

pub fn localization_drift(
    frames: &[Vec<LocalizationPoint>],
    reference: u32,
    report: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Option<Vec<[f32; 3]>>, String> {
    if frames.len() < 2 {
        return Err("the object set holds one frame to align".to_string());
    }
    if reference as usize >= frames.len() {
        return Err("the drift reference lies outside the time series".to_string());
    }
    let total = frames.iter().map(Vec::len).sum::<usize>();
    if total < MINIMUM_POINTS {
        return Err(format!(
            "the object set holds {total} localizations, too few to align"
        ));
    }
    let reach = Reach::measure(frames);
    let segmentation = Segmentation::new(frames.len(), reference);
    let Some(reference_cloud) = segmentation.centers(frames, segmentation.reference) else {
        return Err("the reference frames hold no localization to pair".to_string());
    };
    let cloud = Cloud::new(&reference_cloud, reach);
    let segments = segmentation.len();
    let mut drift = vec![None; segments];
    drift[segmentation.reference] = Some([0.0; 3]);
    let mut ring = 0usize;
    for distance in 1..segments {
        for segment in segmentation.ring(distance) {
            let Some(points) = segmentation.centers(frames, segment) else {
                continue;
            };
            if points.len() < MINIMUM_PAIRS {
                continue;
            }
            let start = extrapolate(&drift, &segmentation, segment);
            let Some(shift) = cloud.estimate(&points, reach, start) else {
                return Err(format!(
                    "the frames {} to {} pair too few localizations to measure drift",
                    segmentation.bounds(segment)[0],
                    segmentation.bounds(segment)[1] - 1
                ));
            };
            drift[segment] = Some(shift);
        }
        ring = distance;
        if !report(distance, segments) {
            return Ok(None);
        }
    }
    let Some(offsets) = interpolate(&drift, &segmentation, frames.len()) else {
        return Err("the localizations fill fewer than two segments to align".to_string());
    };
    if !report(ring + 1, ring + 1) {
        return Ok(None);
    }
    let anchor = offsets[reference as usize];
    if !anchor.iter().all(|value| value.is_finite()) {
        return Err("the drift estimate carries a non-finite offset".to_string());
    }
    Ok(Some(
        offsets
            .into_iter()
            .map(|offset| {
                [0, 1, 2].map(|axis| {
                    if axis == 2 && !reach.axial_measured {
                        0.0
                    } else {
                        offset[axis] - anchor[axis]
                    }
                })
            })
            .collect(),
    ))
}

#[derive(Clone, Copy)]
struct Reach {
    lateral: f32,
    axial: f32,
    axial_measured: bool,
}

impl Reach {
    fn measure(frames: &[Vec<LocalizationPoint>]) -> Self {
        let total = frames.iter().map(Vec::len).sum::<usize>();
        let stride = (total / SAMPLE).max(1);
        let mut lateral = Vec::with_capacity(total.div_ceil(stride).min(SAMPLE) + 1);
        let mut axial = Vec::with_capacity(total.div_ceil(stride).min(SAMPLE) + 1);
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        let mut seen = 0usize;
        for frame in frames {
            for point in frame {
                if seen.is_multiple_of(stride) {
                    let precision = point.precision();
                    lateral.push(0.5 * (precision[0] + precision[1]));
                    axial.push(precision[2]);
                    low = low.min(point.center[2]);
                    high = high.max(point.center[2]);
                }
                seen += 1;
            }
        }
        lateral.sort_by(f32::total_cmp);
        axial.sort_by(f32::total_cmp);
        let median = |values: &[f32]| values.get(values.len() / 2).copied().unwrap_or(0.0);
        Self {
            lateral: (SEARCH_REACH * median(&lateral)).clamp(LATERAL_MINIMUM, LATERAL_MAXIMUM),
            axial: (SEARCH_REACH * median(&axial)).clamp(AXIAL_MINIMUM, AXIAL_MAXIMUM),
            axial_measured: high - low >= MINIMUM_SPAN,
        }
    }

    fn scaled(&self, widen: u32) -> Self {
        let scale = widen as f32;
        Self {
            lateral: self.lateral * scale,
            axial: self.axial * scale,
            axial_measured: self.axial_measured,
        }
    }

    fn spacing(&self) -> [f32; 3] {
        [self.lateral, self.lateral, self.axial]
    }
}

struct Segmentation {
    frames: usize,
    length: usize,
    reference: usize,
}

impl Segmentation {
    fn new(frames: usize, reference: u32) -> Self {
        let shortest = (frames / 2).clamp(1, MINIMUM_SEGMENT_FRAMES);
        let length = frames.div_ceil(SEGMENT_LIMIT).max(shortest).min(frames);
        let segments = frames.div_ceil(length);
        let reference = (reference as usize / length).min(segments - 1);
        Self {
            frames,
            length,
            reference,
        }
    }

    fn len(&self) -> usize {
        self.frames.div_ceil(self.length)
    }

    fn bounds(&self, segment: usize) -> [usize; 2] {
        [
            segment * self.length,
            ((segment + 1) * self.length).min(self.frames),
        ]
    }

    fn center(&self, segment: usize) -> f32 {
        let [start, end] = self.bounds(segment);
        start as f32 + (end - start - 1) as f32 * 0.5
    }

    fn ring(&self, distance: usize) -> impl Iterator<Item = usize> {
        let below = self.reference.checked_sub(distance);
        let above = {
            let segment = self.reference + distance;
            (segment < self.len()).then_some(segment)
        };
        below.into_iter().chain(above)
    }

    fn centers(&self, frames: &[Vec<LocalizationPoint>], segment: usize) -> Option<Vec<[f32; 3]>> {
        let [start, end] = self.bounds(segment);
        let mut points = Vec::new();
        for frame in &frames[start..end] {
            points.extend(frame.iter().map(LocalizationPoint::center));
        }
        (!points.is_empty()).then_some(points)
    }
}

fn extrapolate(
    drift: &[Option<[f32; 3]>],
    segmentation: &Segmentation,
    segment: usize,
) -> [f32; 3] {
    let direction: i64 = if segment > segmentation.reference {
        -1
    } else {
        1
    };
    let mut near = None;
    let mut far = None;
    let mut at = segment as i64 + direction;
    while at >= 0 && at < segmentation.len() as i64 {
        if let Some(offset) = drift[at as usize] {
            if near.is_none() {
                near = Some((at as usize, offset));
            } else {
                far = Some((at as usize, offset));
                break;
            }
        }
        at += direction;
    }
    let Some((near, near_offset)) = near else {
        return [0.0; 3];
    };
    let Some((far, far_offset)) = far else {
        return near_offset;
    };
    let near_center = segmentation.center(near);
    let far_center = segmentation.center(far);
    let span = near_center - far_center;
    if !(span.is_finite() && span.abs() > 0.0) {
        return near_offset;
    }
    let slope = [0, 1, 2].map(|axis| (near_offset[axis] - far_offset[axis]) / span);
    let ahead = segmentation.center(segment) - near_center;
    [0, 1, 2].map(|axis| near_offset[axis] + slope[axis] * ahead)
}

fn interpolate(
    drift: &[Option<[f32; 3]>],
    segmentation: &Segmentation,
    frames: usize,
) -> Option<Vec<[f32; 3]>> {
    let solved = (0..segmentation.len())
        .filter_map(|segment| drift[segment].map(|offset| (segmentation.center(segment), offset)))
        .collect::<Vec<_>>();
    if solved.len() < 2 {
        return None;
    }
    let mut offsets = Vec::with_capacity(frames);
    for time in 0..frames {
        let time = time as f32;
        let first = solved[0];
        let last = solved[solved.len() - 1];
        let sample = if time <= first.0 {
            first.1
        } else if time >= last.0 {
            last.1
        } else {
            let mut at = 0usize;
            while at + 1 < solved.len() && solved[at + 1].0 <= time {
                at += 1;
            }
            let (left, left_offset) = solved[at];
            let (right, right_offset) = solved[at + 1];
            let weight = if right > left {
                ((time - left) / (right - left)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            [0, 1, 2]
                .map(|axis| left_offset[axis] + weight * (right_offset[axis] - left_offset[axis]))
        };
        offsets.push(sample);
    }
    Some(offsets)
}

struct Cloud {
    reach: [f32; 3],
    cells: Map<[i64; 3], Vec<u32>>,
    centers: Vec<[f32; 3]>,
}

impl Cloud {
    fn new(points: &[[f32; 3]], reach: Reach) -> Self {
        let spacing = reach.spacing();
        let mut cells = Map::<[i64; 3], Vec<u32>>::default();
        for (index, center) in points.iter().enumerate() {
            cells
                .entry(cell(*center, spacing))
                .or_default()
                .push(index as u32);
        }
        Self {
            reach: spacing,
            cells,
            centers: points.to_vec(),
        }
    }

    fn estimate(&self, points: &[[f32; 3]], reach: Reach, start: [f32; 3]) -> Option<[f32; 3]> {
        let minimum = (points.len() / PAIR_SHARE)
            .max(MINIMUM_PAIRS)
            .min(points.len());
        let mut shift = start;
        let mut widen = 1u32;
        while widen <= WIDEN_LIMIT {
            let held;
            let cloud = if widen == 1 {
                self
            } else {
                held = Cloud::new(&self.centers, reach.scaled(widen));
                &held
            };
            if cloud.refine(points, &mut shift, minimum) {
                if widen > 1 {
                    self.refine(points, &mut shift, minimum);
                }
                return shift.iter().all(|value| value.is_finite()).then_some(shift);
            }
            widen *= 2;
        }
        None
    }

    fn refine(&self, points: &[[f32; 3]], shift: &mut [f32; 3], minimum: usize) -> bool {
        let mut previous = f32::INFINITY;
        for _ in 0..ITERATIONS {
            let mut sum = [0.0f64; 3];
            let mut pairs = 0usize;
            for point in points {
                let moved = [0, 1, 2].map(|axis| point[axis] - shift[axis]);
                let Some(found) = self.nearest(moved) else {
                    continue;
                };
                for axis in 0..3 {
                    sum[axis] += f64::from(moved[axis] - found[axis]);
                }
                pairs += 1;
            }
            if pairs < minimum {
                return false;
            }
            let mean = [0, 1, 2].map(|axis| (sum[axis] / pairs as f64) as f32);
            let norm = mean.iter().map(|value| value * value).sum::<f32>().sqrt();
            if norm > previous * STALL {
                return true;
            }
            for axis in 0..3 {
                shift[axis] += mean[axis];
            }
            if norm < CONVERGED {
                return true;
            }
            previous = norm;
        }
        true
    }

    fn nearest(&self, point: [f32; 3]) -> Option<[f32; 3]> {
        let at = cell(point, self.reach);
        let mut best = None;
        let mut nearest = f32::INFINITY;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let Some(held) = self.cells.get(&[at[0] + dx, at[1] + dy, at[2] + dz]) else {
                        continue;
                    };
                    for index in held {
                        let center = self.centers[*index as usize];
                        let mut distance = 0.0f32;
                        for axis in 0..3 {
                            let delta = (center[axis] - point[axis]) / self.reach[axis];
                            distance += delta * delta;
                        }
                        if distance <= 1.0 && distance < nearest {
                            nearest = distance;
                            best = Some(center);
                        }
                    }
                }
            }
        }
        best
    }
}

fn cell(point: [f32; 3], spacing: [f32; 3]) -> [i64; 3] {
    [0, 1, 2].map(|axis| (point[axis] / spacing[axis]).floor() as i64)
}
