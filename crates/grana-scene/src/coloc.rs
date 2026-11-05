use grana_measure::{
    ColocCost, ColocPair, ColocSeries, ColocSeriesInfo, ColocSettings, ColocStatistics,
    ColocThresholdRange, ColocThresholds,
};
use grana_volume::Region;

use crate::object_set::ObjectSetId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskKey {
    set: ObjectSetId,
    document: u64,
    radius: u32,
}

impl MaskKey {
    pub fn new(set: ObjectSetId, document: u64, radius: f32) -> Self {
        assert!(
            radius.is_finite() && radius >= 0.0,
            "an analysis mask grows by a finite radius"
        );
        Self {
            set,
            document,
            radius: radius.to_bits(),
        }
    }

    pub fn set(self) -> ObjectSetId {
        self.set
    }

    pub fn document(self) -> u64 {
        self.document
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocResult {
    thresholds: ColocThresholds,
    series: ColocSeries,
}

impl ColocResult {
    pub fn new(series: ColocSeries) -> Self {
        let thresholds = series.otsu();
        Self::from_parts(thresholds, series)
    }

    pub fn from_parts(thresholds: ColocThresholds, series: ColocSeries) -> Self {
        Self { thresholds, series }
    }

    pub fn pair(&self) -> ColocPair {
        self.series.pair()
    }

    pub fn thresholds(&self) -> ColocThresholds {
        self.thresholds
    }

    pub fn set_thresholds(&mut self, thresholds: ColocThresholds) {
        self.thresholds = thresholds;
    }

    pub fn series(&self) -> &ColocSeries {
        &self.series
    }

    pub fn info(&self) -> ColocSeriesInfo {
        self.series.info()
    }

    pub fn level(&self) -> usize {
        self.series.level()
    }

    pub fn permutation_level(&self) -> usize {
        self.series.permutation_level()
    }

    pub fn permutation_stride(&self) -> u32 {
        self.series.permutation_stride()
    }

    pub fn mask(&self) -> Option<u64> {
        self.series.mask()
    }

    pub fn significance(&self) -> Option<grana_measure::ColocSignificance> {
        self.series.significance()
    }

    pub fn costes(&self) -> Option<ColocThresholdRange> {
        self.series.costes()
    }

    pub fn cost(&self) -> ColocCost {
        self.series.cost()
    }

    pub fn statistics(&self) -> ColocStatistics {
        self.series.statistics(self.thresholds)
    }

    pub fn frame_statistics(&self, time: u32) -> Option<ColocStatistics> {
        self.series.frame_statistics(time, self.thresholds)
    }
}

#[derive(Debug, PartialEq)]
pub struct ColocState {
    pub pair: Option<ColocPair>,
    pub results: Vec<ColocResult>,
    pub settings: ColocSettings,
    pub document: u64,
}

pub struct Coloc {
    channels: usize,
    pair: Option<ColocPair>,
    results: Vec<ColocResult>,
    settings: ColocSettings,
    region: Region,
    mask: Option<MaskKey>,
    revision: u64,
}

impl Coloc {
    pub fn new(channels: usize, region: Region) -> Self {
        Self {
            channels,
            pair: (channels >= 2).then(|| ColocPair::new(0, 1)),
            results: Vec::new(),
            settings: ColocSettings::default(),
            region,
            mask: None,
            revision: 1,
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn pair(&self) -> Option<ColocPair> {
        self.pair
    }

    pub fn set_pair(&mut self, pair: ColocPair) {
        assert!(
            pair.a() < self.channels && pair.b() < self.channels,
            "a colocalization pair names two channels of the dataset"
        );
        if self.pair == Some(pair) {
            return;
        }
        self.pair = Some(pair);
        self.revision += 1;
    }

    pub fn results(&self) -> &[ColocResult] {
        &self.results
    }

    pub fn result(&self) -> Option<&ColocResult> {
        let pair = self.pair?;
        self.results.iter().find(|result| result.pair() == pair)
    }

    pub fn series(&self) -> Option<&ColocSeries> {
        self.result().map(ColocResult::series)
    }

    pub fn thresholds(&self) -> Option<ColocThresholds> {
        self.result().map(ColocResult::thresholds)
    }

    pub fn set_thresholds(&mut self, thresholds: ColocThresholds) {
        let Some(result) = self.result_mut() else {
            return;
        };
        if result.thresholds() == thresholds {
            return;
        }
        result.set_thresholds(thresholds);
        self.revision += 1;
    }

    pub fn series_statistics(&self) -> Option<ColocStatistics> {
        self.result().map(ColocResult::statistics)
    }

    pub fn statistics(&self, time: u32) -> Option<ColocStatistics> {
        self.result()?.frame_statistics(time)
    }

    pub fn settings(&self) -> ColocSettings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: ColocSettings) {
        if self.settings == settings {
            return;
        }
        self.settings = settings;
        self.clear_results();
        self.revision += 1;
    }

    pub fn remove_result(&mut self, pair: ColocPair) {
        let length = self.results.len();
        self.results.retain(|result| result.pair() != pair);
        if self.results.len() != length {
            self.revision += 1;
        }
    }

    pub fn clear(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.results.clear();
        self.revision += 1;
    }

    pub fn install(&mut self, series: ColocSeries) {
        assert!(
            self.pair == Some(series.pair()),
            "a colocalization result measures the current channel pair"
        );
        assert_eq!(
            series.mask(),
            self.mask.map(|mask| mask.set().raw()),
            "a colocalization result measures the current analysis mask"
        );
        let result = ColocResult::new(series);
        match self
            .results
            .iter_mut()
            .find(|existing| existing.pair() == result.pair())
        {
            Some(existing) => *existing = result,
            None => self.results.push(result),
        }
        self.revision += 1;
    }

    pub fn set_inputs(&mut self, region: Region, mask: Option<MaskKey>) {
        if self.region == region && self.mask == mask {
            return;
        }
        self.region = region;
        self.mask = mask;
        if !self.results.is_empty() {
            self.clear_results();
            self.revision += 1;
        }
    }

    pub fn state(&self) -> ColocState {
        ColocState {
            pair: self.pair,
            results: self.results.clone(),
            settings: self.settings,
            document: self.revision,
        }
    }

    pub fn restore(
        &mut self,
        state: ColocState,
        channels: usize,
        frames: u32,
        region: Region,
        mask: Option<MaskKey>,
    ) {
        let ColocState {
            pair,
            results,
            settings,
            document,
        } = state;
        assert!(
            pair.is_none_or(|pair| pair.a() < channels && pair.b() < channels),
            "a colocalization state names two channels of the dataset"
        );
        assert!(document > 0, "a colocalization state carries a document");
        for (index, result) in results.iter().enumerate() {
            let pair = result.pair();
            assert!(
                pair.a() < channels && pair.b() < channels,
                "a colocalization state names two channels of the dataset"
            );
            assert!(
                result.series().frames().len() == frames as usize,
                "a colocalization state covers every frame of the dataset"
            );
            assert_eq!(
                result.mask(),
                mask.map(|mask| mask.set().raw()),
                "a colocalization state measures the analysis mask of its scene"
            );
            assert!(
                results[index + 1..]
                    .iter()
                    .all(|other| other.pair() != pair),
                "a colocalization state measures a channel pair once"
            );
        }
        self.channels = channels;
        self.pair = pair;
        self.results = results;
        self.settings = settings;
        self.region = region;
        self.mask = mask;
        self.revision = document;
    }

    pub fn document(&self) -> u64 {
        self.revision
    }

    fn result_mut(&mut self) -> Option<&mut ColocResult> {
        let pair = self.pair?;
        self.results.iter_mut().find(|result| result.pair() == pair)
    }

    fn clear_results(&mut self) {
        self.results.clear();
    }
}
