use grana_measure::{ColocPair, ColocSettings};
use grana_object::TrackParams;
use grana_pipeline::Layout;
use grana_scene::{MaskKey, ObjectParams, ObjectSetId, Revision};
use grana_volume::Region;

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Coloc(ColocInput),
    ObjectColoc(ObjectColocInput),
    Detect(DetectInput),
    Track(TrackInput),
}

impl Input {
    pub fn coloc(
        layout: Layout,
        drifts: u64,
        pair: ColocPair,
        settings: ColocSettings,
        region: Region,
        mask: Option<MaskKey>,
    ) -> Self {
        Self::Coloc(ColocInput {
            layout,
            drifts,
            pair,
            settings,
            region,
            mask,
        })
    }

    pub fn object_coloc(
        layout: Layout,
        drifts: u64,
        pair: ColocPair,
        region: Region,
        set: ObjectSetId,
        frames: Vec<Option<Revision>>,
    ) -> Self {
        Self::ObjectColoc(ObjectColocInput {
            layout,
            drifts,
            pair,
            region,
            set,
            frames,
        })
    }

    pub fn detect(
        layout: Layout,
        drifts: u64,
        channel: usize,
        params: ObjectParams,
        region: Region,
        mask: Option<MaskKey>,
        seeds: u64,
    ) -> Self {
        Self::Detect(DetectInput {
            layout,
            drifts,
            channel,
            params,
            region,
            mask,
            seeds,
        })
    }

    pub fn track(frames: Vec<Option<Revision>>, params: TrackParams) -> Self {
        Self::Track(TrackInput { frames, params })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColocInput {
    pub layout: Layout,
    pub drifts: u64,
    pub pair: ColocPair,
    pub settings: ColocSettings,
    pub region: Region,
    pub mask: Option<MaskKey>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectColocInput {
    pub layout: Layout,
    pub drifts: u64,
    pub pair: ColocPair,
    pub region: Region,
    pub set: ObjectSetId,
    pub frames: Vec<Option<Revision>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectInput {
    pub layout: Layout,
    pub drifts: u64,
    pub channel: usize,
    pub params: ObjectParams,
    pub region: Region,
    pub mask: Option<MaskKey>,
    pub seeds: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackInput {
    pub frames: Vec<Option<Revision>>,
    pub params: TrackParams,
}
