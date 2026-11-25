use grana_measure::{ColocPair, ColocSettings};
use grana_object::{SurfaceCut, TrackParams};
use grana_pipeline::Layout;
use grana_scene::{Drift, MaskKey, ObjectParams, ObjectSetId, Revision, Scene};
use grana_volume::Region;

#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Coloc(ColocInput),
    ObjectColoc(ObjectColocInput),
    Detect(DetectInput),
    Import(ImportInput),
    Recut(RecutInput),
    Relation(RelationInput),
    Track(TrackInput),
    Drift(DriftInput),
    Register(RegisterInput),
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

    pub fn import(
        set: ObjectSetId,
        channel: usize,
        source: ImportSource,
        drifts: u64,
        registration: u64,
    ) -> Self {
        Self::Import(ImportInput {
            set,
            channel,
            source,
            drifts,
            registration,
        })
    }

    pub fn recut(set: ObjectSetId, frames: Vec<Option<Revision>>, cut: SurfaceCut) -> Self {
        Self::Recut(RecutInput { set, frames, cut })
    }

    pub fn relation(
        source: ObjectSetId,
        target: ObjectSetId,
        shift: i32,
        gate: f32,
        frames: Vec<(Option<Revision>, Option<Revision>)>,
    ) -> Self {
        Self::Relation(RelationInput {
            source,
            target,
            shift,
            gate,
            frames,
        })
    }

    pub fn track(frames: Vec<Option<Revision>>, params: TrackParams) -> Self {
        Self::Track(TrackInput { frames, params })
    }

    pub fn drift(input: DriftInput) -> Self {
        Self::Drift(input)
    }

    pub fn register(input: RegisterInput) -> Self {
        Self::Register(input)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportSource {
    Label(String),
    File(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportInput {
    pub set: ObjectSetId,
    pub channel: usize,
    pub source: ImportSource,
    pub drifts: u64,
    pub registration: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecutInput {
    pub set: ObjectSetId,
    pub frames: Vec<Option<Revision>>,
    pub cut: SurfaceCut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelationInput {
    pub source: ObjectSetId,
    pub target: ObjectSetId,
    pub shift: i32,
    pub gate: f32,
    pub frames: Vec<(Option<Revision>, Option<Revision>)>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DriftInput {
    channel: usize,
    reference: u32,
    source: Option<(ObjectSetId, Revision)>,
    base: Option<u64>,
}

impl DriftInput {
    pub fn channels(channel: usize, reference: u32) -> Self {
        Self {
            channel,
            reference,
            source: None,
            base: None,
        }
    }

    pub fn localizations(
        channel: usize,
        reference: u32,
        source: ObjectSetId,
        revision: Revision,
        base: u64,
    ) -> Self {
        Self {
            channel,
            reference,
            source: Some((source, revision)),
            base: Some(base),
        }
    }

    pub fn current(&self, scene: &Scene) -> Option<Self> {
        let source = match self.source {
            Some((set, _)) => Some((set, scene.objects.get(set)?.revision())),
            None => None,
        };
        let base = self.base.map(|_| drift_base(scene, self.channel).digest());
        Some(Self {
            channel: self.channel,
            reference: self.reference,
            source,
            base,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RegisterInput {
    reference: usize,
    sources: Vec<(ObjectSetId, Revision)>,
}

impl RegisterInput {
    pub fn channels(reference: usize) -> Self {
        Self {
            reference,
            sources: Vec::new(),
        }
    }

    pub fn localizations(reference: usize, sources: Vec<(ObjectSetId, Revision)>) -> Self {
        Self { reference, sources }
    }

    pub fn current(&self, scene: &Scene) -> Option<Self> {
        let sources = self
            .sources
            .iter()
            .map(|(set, _)| Some((*set, scene.objects.get(*set)?.revision())))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            reference: self.reference,
            sources,
        })
    }
}

pub fn drift_base(scene: &Scene, channel: usize) -> Drift {
    scene.drift(channel).cloned().unwrap_or_default()
}
