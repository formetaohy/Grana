use crate::correlation::Pearson;

mod threshold;

pub const COLOC_BINS_MIN: usize = 32;
pub const COLOC_BINS_MAX: usize = 512;
pub const COLOC_PERMUTATIONS_DEFAULT: u32 = 200;
pub const COLOC_PERMUTATIONS_MAX: u32 = 2000;
pub const COLOC_BLOCK_DEFAULT: u32 = 3;
pub const COLOC_BLOCK_MAX: u32 = 32;

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

    fn cuts(self, bins: usize) -> [usize; 2] {
        assert!(
            bins > 0,
            "a colocalization histogram holds at least one bin"
        );
        [self.a, self.b].map(|threshold| ((threshold * bins as f32) as usize).min(bins - 1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColocThresholdRange {
    low: ColocThresholds,
    high: ColocThresholds,
}

impl ColocThresholdRange {
    pub fn new(low: ColocThresholds, high: ColocThresholds) -> Self {
        assert!(
            low.a() <= high.a() && low.b() <= high.b(),
            "a colocalization threshold range grows with its thresholds"
        );
        Self { low, high }
    }

    pub fn low(self) -> ColocThresholds {
        self.low
    }

    pub fn high(self) -> ColocThresholds {
        self.high
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocSettings {
    permutations: u32,
    block: u32,
}

impl ColocSettings {
    pub fn new(permutations: u32, block: u32) -> Self {
        assert!(
            (1..=COLOC_PERMUTATIONS_MAX).contains(&permutations),
            "a colocalization significance test runs between one and the maximum number of permutations"
        );
        assert!(
            (1..=COLOC_BLOCK_MAX).contains(&block),
            "a colocalization randomization block spans a supported number of voxels"
        );
        Self {
            permutations,
            block,
        }
    }

    pub fn permutations(self) -> u32 {
        self.permutations
    }

    pub fn block(self) -> u32 {
        self.block
    }
}

impl Default for ColocSettings {
    fn default() -> Self {
        Self::new(COLOC_PERMUTATIONS_DEFAULT, COLOC_BLOCK_DEFAULT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocSeriesInfo {
    level: usize,
    permutation_level: usize,
    permutation_stride: u32,
    mask: Option<u64>,
}

impl ColocSeriesInfo {
    pub fn new(
        level: usize,
        permutation_level: usize,
        permutation_stride: u32,
        mask: Option<u64>,
    ) -> Self {
        assert!(
            permutation_level >= level,
            "a colocalization significance test samples a coarser level than its scan"
        );
        assert!(
            permutation_stride > 0,
            "a colocalization significance test samples a positive stride"
        );
        Self {
            level,
            permutation_level,
            permutation_stride,
            mask,
        }
    }

    pub fn level(self) -> usize {
        self.level
    }

    pub fn permutation_level(self) -> usize {
        self.permutation_level
    }

    pub fn permutation_stride(self) -> u32 {
        self.permutation_stride
    }

    pub fn mask(self) -> Option<u64> {
        self.mask
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColocSignificance {
    p: f64,
    randomizations: u32,
    permutation_pearson: f64,
}

impl ColocSignificance {
    pub fn new(p: f64, randomizations: u32, permutation_pearson: f64) -> Self {
        assert!(
            p.is_finite() && (0.0..=1.0).contains(&p),
            "a permutation p value is a probability"
        );
        assert!(
            randomizations > 0,
            "a permutation test runs at least one randomization"
        );
        assert!(
            permutation_pearson.is_finite() && (-1.0..=1.0).contains(&permutation_pearson),
            "a permutation test compares Pearson coefficients"
        );
        Self {
            p,
            randomizations,
            permutation_pearson,
        }
    }

    pub fn p(self) -> f64 {
        self.p
    }

    pub fn randomizations(self) -> u32 {
        self.randomizations
    }

    pub fn permutation_pearson(self) -> f64 {
        self.permutation_pearson
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocCost {
    scan_voxels: u64,
    permutation_voxels: u64,
    permutation_frames: u32,
    randomizations: u32,
    blocks: u64,
}

impl ColocCost {
    pub fn new(
        scan_voxels: u64,
        permutation_voxels: u64,
        permutation_frames: u32,
        randomizations: u32,
        blocks: u64,
    ) -> Self {
        assert!(
            scan_voxels > 0,
            "a colocalization scan visits at least one voxel"
        );
        assert!(
            permutation_voxels > 0 && permutation_frames > 0 && blocks > 0,
            "a colocalization significance test samples blocks of voxels"
        );
        assert!(
            (1..=COLOC_PERMUTATIONS_MAX).contains(&randomizations),
            "a colocalization significance test runs a supported number of permutations"
        );
        Self {
            scan_voxels,
            permutation_voxels,
            permutation_frames,
            randomizations,
            blocks,
        }
    }

    pub fn scan_voxels(self) -> u64 {
        self.scan_voxels
    }

    pub fn permutation_voxels(self) -> u64 {
        self.permutation_voxels
    }

    pub fn permutation_frames(self) -> u32 {
        self.permutation_frames
    }

    pub fn randomizations(self) -> u32 {
        self.randomizations
    }

    pub fn blocks(self) -> u64 {
        self.blocks
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
    significance: Option<ColocSignificance>,
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

    pub fn significance(self) -> Option<ColocSignificance> {
        self.significance
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocFrame {
    moments: ColocMoments,
    a_by_b: Vec<f64>,
    b_by_a: Vec<f64>,
}

impl ColocFrame {
    pub fn new(bins: usize) -> Self {
        assert!(bins > 0, "a colocalization frame holds intensity bins");
        Self {
            moments: ColocMoments::default(),
            a_by_b: vec![0.0; bins],
            b_by_a: vec![0.0; bins],
        }
    }

    pub fn from_parts(moments: ColocMoments, a_by_b: Vec<f64>, b_by_a: Vec<f64>) -> Self {
        assert!(
            !a_by_b.is_empty() && a_by_b.len() == b_by_a.len(),
            "a colocalization frame carries one sum per intensity bin and channel"
        );
        Self {
            moments,
            a_by_b,
            b_by_a,
        }
    }

    pub fn bins(&self) -> usize {
        self.a_by_b.len()
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
        let bins = self.bins();
        let bin_a = bin(a, bins);
        let bin_b = bin(b, bins);
        self.moments.push(f64::from(a), f64::from(b));
        self.a_by_b[bin_b] += f64::from(a);
        self.b_by_a[bin_a] += f64::from(b);
    }

    pub fn merge(&mut self, other: &Self) {
        assert!(
            self.bins() == other.bins(),
            "colocalization frames merge at one bin resolution"
        );
        self.moments.merge(&other.moments);
        for (target, source) in self.a_by_b.iter_mut().zip(&other.a_by_b) {
            *target += source;
        }
        for (target, source) in self.b_by_a.iter_mut().zip(&other.b_by_a) {
            *target += source;
        }
    }

    pub fn statistics(&self, thresholds: ColocThresholds) -> ColocStatistics {
        let [cut_a, cut_b] = thresholds.cuts(self.bins());
        let [a_mass, b_mass, ..] = self.moments.sums();
        let first = (a_mass > 0.0).then(|| (above(&self.a_by_b, cut_b) / a_mass).clamp(0.0, 1.0));
        let second = (b_mass > 0.0).then(|| (above(&self.b_by_a, cut_a) / b_mass).clamp(0.0, 1.0));
        ColocStatistics {
            voxels: self.moments.voxels(),
            pearson: self.moments.pearson(),
            overlap: self.moments.overlap(),
            manders: [first, second],
            significance: None,
        }
    }
}

impl Default for ColocFrame {
    fn default() -> Self {
        Self::new(1)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocSeries {
    pair: ColocPair,
    info: ColocSeriesInfo,
    bins: usize,
    frames: Vec<ColocFrame>,
    joint: Vec<u64>,
    total: ColocFrame,
    significance: Option<ColocSignificance>,
    costes: Option<ColocThresholdRange>,
    cost: ColocCost,
}

impl ColocSeries {
    pub fn from_parts(
        pair: ColocPair,
        info: ColocSeriesInfo,
        bins: usize,
        frames: Vec<ColocFrame>,
        joint: Vec<u64>,
        significance: Option<ColocSignificance>,
        cost: ColocCost,
    ) -> Self {
        assert!(!frames.is_empty(), "a colocalization series covers a frame");
        assert!(
            (COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&bins),
            "a colocalization series bins intensities within the supported range"
        );
        assert!(
            joint.len() == bins * bins,
            "a colocalization series maps the square scatter grid"
        );
        assert!(
            frames.iter().all(|frame| frame.bins() == bins),
            "a colocalization series bins every frame at one resolution"
        );
        let mut total = ColocFrame::new(bins);
        for frame in &frames {
            total.merge(frame);
        }
        let voxels = total.moments().voxels();
        assert!(
            voxels > 0,
            "a colocalization series covers at least one voxel"
        );
        assert!(
            joint.iter().sum::<u64>() == voxels,
            "a colocalization series scatters every scanned voxel"
        );
        assert!(
            cost.scan_voxels() == voxels,
            "a colocalization cost counts the voxels of its series"
        );
        if let Some(significance) = significance {
            assert!(
                significance.randomizations() == cost.randomizations(),
                "a colocalization cost counts the randomizations of its significance test"
            );
        }
        let costes = threshold::costes(bins, &joint, total.moments());
        Self {
            pair,
            info,
            bins,
            frames,
            joint,
            total,
            significance,
            costes,
            cost,
        }
    }

    pub fn pair(&self) -> ColocPair {
        self.pair
    }

    pub fn info(&self) -> ColocSeriesInfo {
        self.info
    }

    pub fn level(&self) -> usize {
        self.info.level()
    }

    pub fn permutation_level(&self) -> usize {
        self.info.permutation_level()
    }

    pub fn permutation_stride(&self) -> u32 {
        self.info.permutation_stride()
    }

    pub fn mask(&self) -> Option<u64> {
        self.info.mask()
    }

    pub fn bins(&self) -> usize {
        self.bins
    }

    pub fn frames(&self) -> &[ColocFrame] {
        &self.frames
    }

    pub fn frame(&self, time: u32) -> Option<&ColocFrame> {
        self.frames.get(time as usize)
    }

    pub fn joint(&self) -> &[u64] {
        &self.joint
    }

    pub fn joint_at(&self, a: usize, b: usize) -> u64 {
        assert!(
            a < self.bins && b < self.bins,
            "the scatter cell lies on the grid"
        );
        self.joint[a + self.bins * b]
    }

    pub fn total(&self) -> &ColocFrame {
        &self.total
    }

    pub fn significance(&self) -> Option<ColocSignificance> {
        self.significance
    }

    pub fn costes(&self) -> Option<ColocThresholdRange> {
        self.costes
    }

    pub fn cost(&self) -> ColocCost {
        self.cost
    }

    pub fn statistics(&self, thresholds: ColocThresholds) -> ColocStatistics {
        let mut statistics = self.total.statistics(thresholds);
        statistics.significance = self.significance;
        statistics
    }

    pub fn frame_statistics(
        &self,
        time: u32,
        thresholds: ColocThresholds,
    ) -> Option<ColocStatistics> {
        self.frame(time).map(|frame| frame.statistics(thresholds))
    }

    pub fn otsu(&self) -> ColocThresholds {
        let (counts_a, counts_b) = self.marginals();
        ColocThresholds::new(otsu(&counts_a), otsu(&counts_b))
    }

    pub fn csv(&self, thresholds: ColocThresholds) -> String {
        let mut text = String::from(
            "scope,frame,voxels,pearson,overlap,manders_a,manders_b,p,randomizations\n",
        );
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
        let randomizations = self
            .significance
            .map(|significance| significance.randomizations().to_string())
            .unwrap_or_default();
        for (scope, frame, statistics) in rows {
            let p = statistics
                .significance
                .map(|significance| format!("{:.6}", significance.p()))
                .unwrap_or_default();
            text.push_str(&format!(
                "{scope},{frame},{},{},{},{},{},{p},{randomizations}\n",
                statistics.voxels(),
                option(statistics.pearson()),
                option(statistics.overlap()),
                option(statistics.manders()[0]),
                option(statistics.manders()[1])
            ));
        }
        text
    }

    fn marginals(&self) -> (Vec<u64>, Vec<u64>) {
        let mut counts_a = vec![0; self.bins];
        let mut counts_b = vec![0; self.bins];
        for (b, count_b) in counts_b.iter_mut().enumerate() {
            for (a, count_a) in counts_a.iter_mut().enumerate() {
                let count = self.joint[a + self.bins * b];
                *count_a += count;
                *count_b += count;
            }
        }
        (counts_a, counts_b)
    }
}

fn bin(unit: f32, bins: usize) -> usize {
    ((unit.clamp(0.0, 1.0) * bins as f32) as usize).min(bins - 1)
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
