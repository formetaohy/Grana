use grana_object::{ColocPair, ColocSeries, ColocStatistics, ColocThresholds};
use grana_volume::Region;

#[derive(Debug, PartialEq)]
pub struct ColocState {
    pub pair: Option<ColocPair>,
    pub thresholds: ColocThresholds,
    pub region: Option<Region>,
    pub series: Option<ColocSeries>,
}

pub struct Coloc {
    channels: usize,
    pair: Option<ColocPair>,
    thresholds: ColocThresholds,
    region: Option<Region>,
    series: Option<ColocSeries>,
    revision: u64,
}

impl Coloc {
    pub fn new(channels: usize) -> Self {
        Self {
            channels,
            pair: (channels >= 2).then(|| ColocPair::new(0, 1)),
            thresholds: ColocThresholds::new(0.5, 0.5),
            region: None,
            series: None,
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
        self.region = None;
        self.series = None;
        self.revision += 1;
    }

    pub fn thresholds(&self) -> ColocThresholds {
        self.thresholds
    }

    pub fn set_thresholds(&mut self, thresholds: ColocThresholds) {
        if self.thresholds == thresholds {
            return;
        }
        self.thresholds = thresholds;
        self.revision += 1;
    }

    pub fn series(&self) -> Option<&ColocSeries> {
        self.series.as_ref()
    }

    pub fn region(&self) -> Option<Region> {
        self.region
    }

    pub fn statistics(&self, time: u32) -> Option<ColocStatistics> {
        self.series
            .as_ref()?
            .frame_statistics(time, self.thresholds)
    }

    pub fn series_statistics(&self) -> Option<ColocStatistics> {
        self.series
            .as_ref()
            .map(|series| series.statistics(self.thresholds))
    }

    pub fn install(&mut self, series: ColocSeries, region: Region, frames: u32) {
        assert!(
            self.pair == Some(series.pair()),
            "a colocalization result measures its channel pair"
        );
        assert!(
            series.frames().len() == frames as usize,
            "a colocalization result covers every frame of the dataset"
        );
        self.thresholds = series.otsu();
        self.region = Some(region);
        self.series = Some(series);
        self.revision += 1;
    }

    pub fn clear(&mut self) {
        if self.series.take().is_some() {
            self.region = None;
            self.revision += 1;
        }
    }

    pub fn state(&self) -> ColocState {
        ColocState {
            pair: self.pair,
            thresholds: self.thresholds,
            region: self.region,
            series: self.series.clone(),
        }
    }

    pub fn restore(&mut self, state: ColocState, channels: usize, frames: u32) {
        let ColocState {
            pair,
            thresholds,
            region,
            series,
        } = state;
        assert!(
            pair.is_none_or(|pair| pair.a() < channels && pair.b() < channels),
            "a colocalization state names two channels of the dataset"
        );
        assert!(
            series
                .as_ref()
                .is_none_or(|series| series.frames().len() == frames as usize),
            "a colocalization state covers every frame of the dataset"
        );
        self.pair = pair;
        self.thresholds = thresholds;
        self.region = region;
        self.series = series;
        self.revision += 1;
    }

    pub fn document(&self) -> u64 {
        self.revision
    }
}
