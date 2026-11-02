use crate::correlation::Pearson;

pub const COLOC_BINS: usize = 1024;
pub const COLOC_SCATTER: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocPair {
    a: usize,
    b: usize,
}

impl ColocPair {
    pub fn new(a: usize, b: usize) -> Self {
        assert!(a != b, "a colocalization pair names two distinct channels");
        Self { a, b }
    }

    pub fn a(self) -> usize {
        self.a
    }

    pub fn b(self) -> usize {
        self.b
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColocThresholds {
    a: f32,
    b: f32,
}

impl ColocThresholds {
    pub fn new(a: f32, b: f32) -> Self {
        assert!(
            (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b),
            "a colocalization threshold is a unit fraction"
        );
        Self { a, b }
    }

    pub fn a(self) -> f32 {
        self.a
    }

    pub fn b(self) -> f32 {
        self.b
    }

    fn bins(self) -> [usize; 2] {
        [self.a, self.b]
            .map(|threshold| ((threshold * COLOC_BINS as f32) as usize).min(COLOC_BINS - 1))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColocMoments {
    correlation: Pearson,
}

impl ColocMoments {
    pub fn new(voxels: u64, sums: [f64; 5]) -> Self {
        Self {
            correlation: Pearson::from_sums(voxels, sums),
        }
    }

    pub fn voxels(self) -> u64 {
        self.correlation.count()
    }

    pub fn sums(self) -> [f64; 5] {
        self.correlation.sums()
    }

    pub fn push(&mut self, a: f64, b: f64) {
        self.correlation.push(a, b);
    }

    pub fn merge(&mut self, other: &Self) {
        self.correlation.merge(&other.correlation);
    }

    fn pearson(self) -> Option<f64> {
        self.correlation.value()
    }

    fn overlap(self) -> Option<f64> {
        let [_, _, sum_aa, sum_bb, sum_ab] = self.correlation.sums();
        let mass = sum_aa * sum_bb;
        if mass <= 0.0 {
            return None;
        }
        Some((sum_ab / mass.sqrt()).clamp(0.0, 1.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColocStatistics {
    voxels: u64,
    pearson: Option<f64>,
    overlap: Option<f64>,
    manders: [Option<f64>; 2],
}

impl ColocStatistics {
    pub fn voxels(self) -> u64 {
        self.voxels
    }

    pub fn pearson(self) -> Option<f64> {
        self.pearson
    }

    pub fn overlap(self) -> Option<f64> {
        self.overlap
    }

    pub fn manders(self) -> [Option<f64>; 2] {
        self.manders
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocFrame {
    moments: ColocMoments,
    a_by_b: Vec<f64>,
    b_by_a: Vec<f64>,
}

impl ColocFrame {
    pub fn new() -> Self {
        Self {
            moments: ColocMoments::default(),
            a_by_b: vec![0.0; COLOC_BINS],
            b_by_a: vec![0.0; COLOC_BINS],
        }
    }

    pub fn from_parts(moments: ColocMoments, a_by_b: Vec<f64>, b_by_a: Vec<f64>) -> Self {
        assert!(
            a_by_b.len() == COLOC_BINS && b_by_a.len() == COLOC_BINS,
            "a colocalization frame carries one sum per intensity bin"
        );
        Self {
            moments,
            a_by_b,
            b_by_a,
        }
    }

    pub fn moments(&self) -> ColocMoments {
        self.moments
    }

    pub fn a_by_b(&self) -> &[f64] {
        &self.a_by_b
    }

    pub fn b_by_a(&self) -> &[f64] {
        &self.b_by_a
    }

    pub fn push(&mut self, a: f32, b: f32) {
        let bin_a = bin(a);
        let bin_b = bin(b);
        self.moments.push(f64::from(a), f64::from(b));
        self.a_by_b[bin_b] += f64::from(a);
        self.b_by_a[bin_a] += f64::from(b);
    }

    pub fn merge(&mut self, other: &Self) {
        self.moments.merge(&other.moments);
        for (target, source) in self.a_by_b.iter_mut().zip(&other.a_by_b) {
            *target += source;
        }
        for (target, source) in self.b_by_a.iter_mut().zip(&other.b_by_a) {
            *target += source;
        }
    }

    pub fn statistics(&self, thresholds: ColocThresholds) -> ColocStatistics {
        let [cut_a, cut_b] = thresholds.bins();
        let [a_mass, b_mass, ..] = self.moments.sums();
        let first = (a_mass > 0.0).then(|| (above(&self.a_by_b, cut_b) / a_mass).clamp(0.0, 1.0));
        let second = (b_mass > 0.0).then(|| (above(&self.b_by_a, cut_a) / b_mass).clamp(0.0, 1.0));
        ColocStatistics {
            voxels: self.moments.voxels(),
            pearson: self.moments.pearson(),
            overlap: self.moments.overlap(),
            manders: [first, second],
        }
    }
}

impl Default for ColocFrame {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocSeries {
    pair: ColocPair,
    frames: Vec<ColocFrame>,
    counts_a: Vec<u64>,
    counts_b: Vec<u64>,
    scatter: Vec<u64>,
    total: ColocFrame,
}

impl ColocSeries {
    pub fn new(
        pair: ColocPair,
        frames: Vec<ColocFrame>,
        counts_a: Vec<u64>,
        counts_b: Vec<u64>,
        scatter: Vec<u64>,
    ) -> Self {
        let mut total = ColocFrame::new();
        for frame in &frames {
            total.merge(frame);
        }
        Self::from_parts(pair, frames, counts_a, counts_b, scatter, total)
    }

    pub fn from_parts(
        pair: ColocPair,
        frames: Vec<ColocFrame>,
        counts_a: Vec<u64>,
        counts_b: Vec<u64>,
        scatter: Vec<u64>,
        total: ColocFrame,
    ) -> Self {
        assert!(
            counts_a.len() == COLOC_BINS && counts_b.len() == COLOC_BINS,
            "a colocalization series counts every intensity bin"
        );
        assert!(
            scatter.len() == COLOC_SCATTER * COLOC_SCATTER,
            "a colocalization series maps the square scatter grid"
        );
        Self {
            pair,
            frames,
            counts_a,
            counts_b,
            scatter,
            total,
        }
    }

    pub fn pair(&self) -> ColocPair {
        self.pair
    }

    pub fn frames(&self) -> &[ColocFrame] {
        &self.frames
    }

    pub fn frame(&self, time: u32) -> Option<&ColocFrame> {
        self.frames.get(time as usize)
    }

    pub fn counts_a(&self) -> &[u64] {
        &self.counts_a
    }

    pub fn counts_b(&self) -> &[u64] {
        &self.counts_b
    }

    pub fn scatter(&self) -> &[u64] {
        &self.scatter
    }

    pub fn total(&self) -> &ColocFrame {
        &self.total
    }

    pub fn scatter_at(&self, a: usize, b: usize) -> u64 {
        assert!(
            a < COLOC_SCATTER && b < COLOC_SCATTER,
            "the scatter cell lies on the grid"
        );
        self.scatter[a + COLOC_SCATTER * b]
    }

    pub fn statistics(&self, thresholds: ColocThresholds) -> ColocStatistics {
        self.total.statistics(thresholds)
    }

    pub fn frame_statistics(
        &self,
        time: u32,
        thresholds: ColocThresholds,
    ) -> Option<ColocStatistics> {
        self.frame(time).map(|frame| frame.statistics(thresholds))
    }

    pub fn otsu(&self) -> ColocThresholds {
        ColocThresholds::new(otsu(&self.counts_a), otsu(&self.counts_b))
    }

    pub fn csv(&self, thresholds: ColocThresholds) -> String {
        let mut text = String::from("scope,frame,voxels,pearson,overlap,manders_a,manders_b\n");
        let mut rows = vec![(
            "series".to_string(),
            String::new(),
            self.statistics(thresholds),
        )];
        for (time, frame) in self.frames.iter().enumerate() {
            rows.push((
                "frame".to_string(),
                time.to_string(),
                frame.statistics(thresholds),
            ));
        }
        for (scope, frame, statistics) in rows {
            text.push_str(&format!(
                "{scope},{frame},{},{},{},{},{}\n",
                statistics.voxels(),
                option(statistics.pearson()),
                option(statistics.overlap()),
                option(statistics.manders()[0]),
                option(statistics.manders()[1])
            ));
        }
        text
    }
}

fn bin(unit: f32) -> usize {
    ((unit.clamp(0.0, 1.0) * COLOC_BINS as f32) as usize).min(COLOC_BINS - 1)
}

fn above(sums: &[f64], cut: usize) -> f64 {
    sums.get(cut + 1..).map_or(0.0, |tail| tail.iter().sum())
}

fn otsu(counts: &[u64]) -> f32 {
    let total: f64 = counts.iter().map(|count| *count as f64).sum();
    if total <= 0.0 {
        return 0.5;
    }
    let mut sum_all = 0.0;
    for (index, count) in counts.iter().enumerate() {
        sum_all += (index as f64 + 0.5) * (*count as f64);
    }
    let mut weight = 0.0;
    let mut sum = 0.0;
    let mut best = 0.0;
    let mut split = 0;
    for (index, count) in counts.iter().enumerate() {
        weight += *count as f64;
        sum += (index as f64 + 0.5) * (*count as f64);
        let remaining = total - weight;
        if weight <= 0.0 || remaining <= 0.0 {
            continue;
        }
        let between = weight * remaining * (sum / weight - (sum_all - sum) / remaining).powi(2);
        if between >= best {
            best = between;
            split = index;
        }
    }
    split as f32 / counts.len() as f32
}

fn option(value: Option<f64>) -> String {
    value.map_or_else(String::new, |value| format!("{value:.6}"))
}
