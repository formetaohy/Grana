use crate::column::{Column, Unit};
use crate::correlation::Pearson;

mod threshold;

pub const COLOC_BINS_MIN: usize = 32;
pub const COLOC_BINS_MAX: usize = 512;
pub const COLOC_PERMUTATIONS_DEFAULT: u32 = 200;
pub const COLOC_PERMUTATIONS_MAX: u32 = 2000;
pub const COLOC_BLOCK_DEFAULT: u32 = 3;
pub const COLOC_BLOCK_MAX: u32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColocPrecision {
    #[default]
    Full,
    Balanced,
    Fast,
}

impl ColocPrecision {
    pub const ALL: [Self; 3] = [Self::Full, Self::Balanced, Self::Fast];

    pub fn index(self) -> u8 {
        match self {
            Self::Full => 0,
            Self::Balanced => 1,
            Self::Fast => 2,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full resolution",
            Self::Balanced => "Balanced",
            Self::Fast => "Fast",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.name() == key)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocSettings {
    permutations: u32,
    block: u32,
    precision: ColocPrecision,
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
            precision: ColocPrecision::default(),
        }
    }

    pub fn with_precision(self, precision: ColocPrecision) -> Self {
        Self { precision, ..self }
    }

    pub fn permutations(self) -> u32 {
        self.permutations
    }

    pub fn block(self) -> u32 {
        self.block
    }

    pub fn precision(self) -> ColocPrecision {
        self.precision
    }
}

impl Default for ColocSettings {
    fn default() -> Self {
        Self::new(COLOC_PERMUTATIONS_DEFAULT, COLOC_BLOCK_DEFAULT)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColocSeriesInfo {
    level: usize,
    stride: u32,
    mask: Option<u64>,
    coverage: f32,
}

impl ColocSeriesInfo {
    pub fn new(level: usize, stride: u32, mask: Option<u64>, coverage: f32) -> Self {
        assert!(
            stride > 0,
            "a colocalization sample visits a positive stride"
        );
        assert!(
            coverage.is_finite() && (0.0..=1.0).contains(&coverage),
            "a colocalization scan covers a fraction of its region"
        );
        Self {
            level,
            stride,
            mask,
            coverage,
        }
    }

    pub fn level(self) -> usize {
        self.level
    }

    pub fn stride(self) -> u32 {
        self.stride
    }

    pub fn mask(self) -> Option<u64> {
        self.mask
    }

    pub fn coverage(self) -> f32 {
        self.coverage
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColocSignificance {
    p: f64,
    randomizations: u32,
}

impl ColocSignificance {
    pub fn new(p: f64, randomizations: u32) -> Self {
        assert!(
            p.is_finite() && (0.0..=1.0).contains(&p),
            "a permutation p value is a probability"
        );
        assert!(
            randomizations > 0,
            "a permutation test runs at least one randomization"
        );
        Self { p, randomizations }
    }

    pub fn p(self) -> f64 {
        self.p
    }

    pub fn randomizations(self) -> u32 {
        self.randomizations
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocCost {
    voxels: u64,
    randomizations: u32,
    blocks: u64,
}

impl ColocCost {
    pub fn new(voxels: u64, randomizations: u32, blocks: u64) -> Self {
        assert!(
            voxels > 0,
            "a colocalization scan visits at least one voxel"
        );
        assert!(
            blocks > 0,
            "a colocalization randomization shuffles blocks of voxels"
        );
        assert!(
            (1..=COLOC_PERMUTATIONS_MAX).contains(&randomizations),
            "a colocalization significance test runs a supported number of permutations"
        );
        Self {
            voxels,
            randomizations,
            blocks,
        }
    }

    pub fn voxels(self) -> u64 {
        self.voxels
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColocCell {
    cell: u32,
    count: u64,
}

impl ColocCell {
    pub fn new(cell: u32, count: u64) -> Self {
        assert!(count > 0, "a joint histogram cell counts a voxel");
        Self { cell, count }
    }

    pub fn cell(self) -> u32 {
        self.cell
    }

    pub fn count(self) -> u64 {
        self.count
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocJoint {
    bins: usize,
    frames: Vec<Vec<ColocCell>>,
    total: Vec<u64>,
}

impl ColocJoint {
    pub fn new(bins: usize, frames: Vec<Vec<ColocCell>>) -> Self {
        Self::checked(bins, frames).expect("a joint histogram scatters every sampled voxel")
    }

    pub fn checked(bins: usize, frames: Vec<Vec<ColocCell>>) -> Option<Self> {
        if !(COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&bins) || frames.is_empty() {
            return None;
        }
        let cells = bins * bins;
        let mut total = vec![0u64; cells];
        for frame in &frames {
            let mut previous = None;
            for cell in frame {
                let index = cell.cell() as usize;
                if index >= cells || previous.is_some_and(|previous| index <= previous) {
                    return None;
                }
                previous = Some(index);
                total[index] += cell.count();
            }
        }
        Some(Self {
            bins,
            frames,
            total,
        })
    }

    pub fn bins(&self) -> usize {
        self.bins
    }

    pub fn frames(&self) -> &[Vec<ColocCell>] {
        &self.frames
    }

    pub fn frame(&self, time: u32) -> Option<&[ColocCell]> {
        self.frames.get(time as usize).map(Vec::as_slice)
    }

    pub fn total(&self) -> &[u64] {
        &self.total
    }

    pub fn cells(&self) -> Vec<ColocCell> {
        self.total
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(cell, count)| {
                ColocCell::new(
                    u32::try_from(cell).expect("a joint histogram grid fits the u32 range"),
                    *count,
                )
            })
            .collect()
    }

    pub fn count(&self, a: usize, b: usize) -> u64 {
        assert!(
            a < self.bins && b < self.bins,
            "the scatter cell lies on the grid"
        );
        self.total[a + self.bins * b]
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
    joint: ColocJoint,
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
        joint: ColocJoint,
        significance: Option<ColocSignificance>,
        cost: ColocCost,
    ) -> Self {
        assert!(!frames.is_empty(), "a colocalization series covers a frame");
        assert!(
            (COLOC_BINS_MIN..=COLOC_BINS_MAX).contains(&bins),
            "a colocalization series bins intensities within the supported range"
        );
        assert!(
            joint.bins() == bins,
            "a colocalization series scatters the square scatter grid"
        );
        assert!(
            joint.frames().len() == frames.len(),
            "a colocalization series scatters every frame"
        );
        assert!(
            frames.iter().all(|frame| frame.bins() == bins),
            "a colocalization series bins every frame at one resolution"
        );
        for (frame, cells) in frames.iter().zip(joint.frames()) {
            assert_eq!(
                cells.iter().map(|cell| cell.count()).sum::<u64>(),
                frame.moments().voxels(),
                "a joint frame scatters every sampled voxel of its frame"
            );
        }
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
            cost.voxels() == voxels,
            "a colocalization cost counts the voxels of its series"
        );
        if let Some(significance) = significance {
            assert!(
                significance.randomizations() == cost.randomizations(),
                "a colocalization cost counts the randomizations of its significance test"
            );
        }
        let costes = threshold::costes(bins, joint.total(), total.moments());
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

    pub fn stride(&self) -> u32 {
        self.info.stride()
    }

    pub fn coverage(&self) -> f32 {
        self.info.coverage()
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

    pub fn joint(&self) -> &ColocJoint {
        &self.joint
    }

    pub fn joint_at(&self, a: usize, b: usize) -> u64 {
        self.joint.count(a, b)
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
        let mut text = crate::csv::line(&[
            "scope",
            "frame",
            "voxels",
            "pearson",
            "overlap",
            "manders_a",
            "manders_b",
            "p",
            "randomizations",
        ]);
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
            let voxels = statistics.voxels().to_string();
            let pearson = option(statistics.pearson());
            let overlap = option(statistics.overlap());
            let manders = statistics.manders();
            let first = option(manders[0]);
            let second = option(manders[1]);
            text.push_str(&crate::csv::line(&[
                &scope,
                &frame,
                &voxels,
                &pearson,
                &overlap,
                &first,
                &second,
                &p,
                &randomizations,
            ]));
        }
        text
    }

    fn marginals(&self) -> (Vec<u64>, Vec<u64>) {
        let joint = self.joint.total();
        let mut counts_a = vec![0; self.bins];
        let mut counts_b = vec![0; self.bins];
        for (b, count_b) in counts_b.iter_mut().enumerate() {
            for (a, count_a) in counts_a.iter_mut().enumerate() {
                let count = joint[a + self.bins * b];
                *count_a += count;
                *count_b += count;
            }
        }
        (counts_a, counts_b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColocMetric {
    Pearson,
    Overlap,
}

impl ColocMetric {
    pub const ALL: [Self; 2] = [Self::Pearson, Self::Overlap];

    pub fn index(self) -> u8 {
        match self {
            Self::Pearson => 0,
            Self::Overlap => 1,
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        Self::ALL.get(index as usize).copied()
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pearson => "pearson",
            Self::Overlap => "overlap",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pearson => "Pearson",
            Self::Overlap => "Overlap",
        }
    }

    pub fn column(self, pair: ColocPair, channels: &[String], keys: &[String]) -> Column {
        let a = channels
            .get(pair.a())
            .map_or_else(|| pair.a().to_string(), Clone::clone);
        let b = channels
            .get(pair.b())
            .map_or_else(|| pair.b().to_string(), Clone::clone);
        let keys = (keys.get(pair.a()), keys.get(pair.b()));
        let key = format!(
            "{}_{}_{}",
            self.name(),
            keys.0.map_or("a", String::as_str),
            keys.1.map_or("b", String::as_str)
        );
        Column::new(format!("{} {a}/{b}", self.label()), key, Unit::Count, 3)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjectColocValue {
    moments: ColocMoments,
}

impl ObjectColocValue {
    pub fn new(voxels: u64, sums: [f64; 5]) -> Self {
        Self {
            moments: ColocMoments::new(voxels, sums),
        }
    }

    pub fn voxels(self) -> u64 {
        self.moments.voxels()
    }

    pub fn sums(self) -> [f64; 5] {
        self.moments.sums()
    }

    pub fn push(&mut self, a: f64, b: f64) {
        self.moments.push(a, b);
    }

    pub fn merge(&mut self, other: &Self) {
        self.moments.merge(&other.moments);
    }

    pub fn metric(self, metric: ColocMetric) -> Option<f64> {
        match metric {
            ColocMetric::Pearson => self.moments.pearson(),
            ColocMetric::Overlap => self.moments.overlap(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectColoc {
    pair: ColocPair,
    level: usize,
    frames: Vec<Vec<ObjectColocValue>>,
}

impl ObjectColoc {
    pub fn new(pair: ColocPair, level: usize, frames: Vec<Vec<ObjectColocValue>>) -> Self {
        assert!(
            !frames.is_empty(),
            "an object colocalization covers at least one frame"
        );
        assert!(
            frames
                .iter()
                .all(|values| values.len() <= usize::from(u16::MAX)),
            "an object colocalization addresses its objects as labels"
        );
        Self {
            pair,
            level,
            frames,
        }
    }

    pub fn pair(&self) -> ColocPair {
        self.pair
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn frames(&self) -> &[Vec<ObjectColocValue>] {
        &self.frames
    }

    pub fn objects(&self, time: u32) -> Option<&[ObjectColocValue]> {
        self.frames.get(time as usize).map(Vec::as_slice)
    }

    pub fn value(&self, time: u32, object: usize) -> Option<ObjectColocValue> {
        self.objects(time)?.get(object).copied()
    }

    pub fn metric(&self, time: u32, object: usize, metric: ColocMetric) -> Option<f64> {
        self.value(time, object)?.metric(metric)
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
