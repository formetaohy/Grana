use grana_measure::{
    ColocCost, ColocPair, ColocSeries, ColocSeriesInfo, ColocSettings, ColocStatistics,
    ColocThresholdRange, ColocThresholds,
};

use crate::mask::MaskFilter;
use crate::object_set::ObjectSetId;
use crate::revision::Revision;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskKey {
    set: ObjectSetId,
    document: u64,
    radius: u32,
    filter: Option<MaskFilter>,
}

impl MaskKey {
    pub fn new(set: ObjectSetId, document: u64, radius: f32, filter: Option<MaskFilter>) -> Self {
        assert!(
            radius.is_finite() && radius >= 0.0,
            "an analysis mask grows by a finite radius"
        );
        Self {
            set,
            document,
            radius: radius.to_bits(),
            filter,
        }
    }

    pub fn set(self) -> ObjectSetId {
        self.set
    }

    pub fn document(self) -> u64 {
        self.document
    }

    pub fn filter(self) -> Option<MaskFilter> {
        self.filter
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

    pub fn stride(&self) -> u32 {
        self.series.stride()
    }

    pub fn coverage(&self) -> f32 {
        self.series.coverage()
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
    pub revision: Revision,
}

pub struct Coloc {
    pair: Option<ColocPair>,
    results: Vec<ColocResult>,
    settings: ColocSettings,
    mask: Option<MaskKey>,
    revision: Revision,
}

impl Coloc {
    pub fn new(channels: usize) -> Self {
        Self {
            pair: (channels >= 2).then(|| ColocPair::new(0, 1)),
            results: Vec::new(),
            settings: ColocSettings::default(),
            mask: None,
            revision: Revision::FIRST,
        }
    }

    pub fn pair(&self) -> Option<ColocPair> {
        self.pair
    }

    pub fn set_pair(&mut self, pair: ColocPair, channels: usize) {
        assert!(
            pair.a() < channels && pair.b() < channels,
            "a colocalization pair names two channels of the dataset"
        );
        if self.pair == Some(pair) {
            return;
        }
        self.pair = Some(pair);
        self.revision = self.revision.next();
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
        self.revision = self.revision.next();
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
        self.results.clear();
        self.revision = self.revision.next();
    }

    pub fn remove_result(&mut self, pair: ColocPair) {
        let length = self.results.len();
        self.results.retain(|result| result.pair() != pair);
        if self.results.len() != length {
            self.revision = self.revision.next();
        }
    }

    pub fn clear(&mut self) {
        if self.results.is_empty() {
            return;
        }
        self.results.clear();
        self.revision = self.revision.next();
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
        self.revision = self.revision.next();
    }

    pub fn set_mask(&mut self, mask: Option<MaskKey>) {
        if self.mask == mask {
            return;
        }
        self.mask = mask;
        self.clear();
    }

    pub fn state(&self) -> ColocState {
        ColocState {
            pair: self.pair,
            results: self.results.clone(),
            settings: self.settings,
            revision: self.revision,
        }
    }

    pub fn restore(
        &mut self,
        state: ColocState,
        channels: usize,
        frames: u32,
        mask: Option<MaskKey>,
    ) {
        let ColocState {
            pair,
            results,
            settings,
            revision,
        } = state;
        assert!(
            pair.is_none_or(|pair| pair.a() < channels && pair.b() < channels),
            "a colocalization state names two channels of the dataset"
        );
        assert!(
            !revision.is_zero(),
            "a colocalization state carries a document"
        );
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
        self.pair = pair;
        self.results = results;
        self.settings = settings;
        self.mask = mask;
        self.revision = revision;
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn grow_channel(&mut self, channels: usize) {
        if self.pair.is_none() && channels >= 2 {
            self.pair = Some(ColocPair::new(0, 1));
            self.revision = self.revision.next();
        }
    }

    pub fn rebuild_channels(&mut self, channels: usize) {
        let pair = self
            .pair
            .filter(|pair| pair.a() < channels && pair.b() < channels);
        self.pair = pair.or_else(|| (channels >= 2).then(|| ColocPair::new(0, 1)));
        self.results.clear();
        self.revision = self.revision.next();
    }

    pub fn remove_channel(&mut self, index: usize) {
        self.results.clear();
        self.pair = match self.pair {
            None => None,
            Some(pair) if pair.a() == index || pair.b() == index => None,
            Some(pair) => {
                let a = if pair.a() > index {
                    pair.a() - 1
                } else {
                    pair.a()
                };
                let b = if pair.b() > index {
                    pair.b() - 1
                } else {
                    pair.b()
                };
                Some(ColocPair::new(a, b))
            }
        };
        self.revision = self.revision.next();
    }

    fn result_mut(&mut self) -> Option<&mut ColocResult> {
        let pair = self.pair?;
        self.results.iter_mut().find(|result| result.pair() == pair)
    }
}
