use crate::identity::{ObjectId, ObjectIds};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelationPair {
    source: ObjectId,
    target: ObjectId,
    contact: f32,
    distance: f32,
    inside: f32,
    center: f32,
}

impl RelationPair {
    pub fn new(
        source: ObjectId,
        target: ObjectId,
        contact: f32,
        distance: f32,
        inside: f32,
        center: f32,
    ) -> Self {
        assert!(
            contact.is_finite() && contact >= 0.0,
            "a pair contact is an area"
        );
        assert!(
            distance.is_finite() && distance >= 0.0,
            "a pair distance is a length"
        );
        assert!(
            (0.0..=1.0).contains(&inside),
            "a pair containment is a fraction"
        );
        assert!(
            center.is_finite() && center >= 0.0,
            "a pair centre distance is a length"
        );
        Self {
            source,
            target,
            contact,
            distance,
            inside,
            center,
        }
    }

    pub fn source(self) -> ObjectId {
        self.source
    }

    pub fn target(self) -> ObjectId {
        self.target
    }

    pub fn contact(self) -> f32 {
        self.contact
    }

    pub fn distance(self) -> f32 {
        self.distance
    }

    pub fn inside(self) -> f32 {
        self.inside
    }

    pub fn center(self) -> f32 {
        self.center
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelationReading {
    distance: f32,
    center: f32,
    inside: f32,
    contact: f32,
    overlapping: u32,
    partner: Option<ObjectId>,
}

impl RelationReading {
    pub fn new(
        distance: f32,
        center: f32,
        inside: f32,
        contact: f32,
        overlapping: u32,
        partner: Option<ObjectId>,
    ) -> Self {
        assert!(
            distance >= 0.0 && center >= 0.0,
            "a relation distance is a length"
        );
        assert!(
            (0.0..=1.0).contains(&inside),
            "a relation containment is a fraction"
        );
        assert!(
            contact.is_finite() && contact >= 0.0,
            "a relation contact is an area"
        );
        Self {
            distance,
            center,
            inside,
            contact,
            overlapping,
            partner,
        }
    }

    pub fn unreached() -> Self {
        Self {
            distance: f32::INFINITY,
            center: f32::INFINITY,
            inside: 0.0,
            contact: 0.0,
            overlapping: 0,
            partner: None,
        }
    }

    pub fn reached(self) -> bool {
        self.distance.is_finite()
    }

    pub fn distance(self) -> f32 {
        self.distance
    }

    pub fn center(self) -> f32 {
        self.center
    }

    pub fn inside(self) -> f32 {
        self.inside
    }

    pub fn contact(self) -> f32 {
        self.contact
    }

    pub fn overlapping(self) -> u32 {
        self.overlapping
    }

    pub fn partner(self) -> Option<ObjectId> {
        self.partner
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelationFrame {
    sources: ObjectIds,
    targets: ObjectIds,
    readings: Vec<RelationReading>,
    pairs: Vec<RelationPair>,
}

impl RelationFrame {
    pub fn new(
        sources: ObjectIds,
        targets: ObjectIds,
        readings: Vec<RelationReading>,
        pairs: Vec<RelationPair>,
    ) -> Self {
        assert_eq!(
            readings.len(),
            sources.len(),
            "a relation frame measures every object it names"
        );
        assert!(
            readings
                .iter()
                .all(|reading| reading.partner.is_none_or(|id| targets.contains(id))),
            "a relation partner belongs to the measured target frame"
        );
        assert!(
            pairs
                .windows(2)
                .all(|pair| (pair[0].source(), pair[0].target())
                    < (pair[1].source(), pair[1].target())),
            "a relation frame lists each pair once in source and target order"
        );
        assert!(
            pairs
                .iter()
                .all(|pair| sources.contains(pair.source()) && targets.contains(pair.target())),
            "a relation pair names objects of the measured frames"
        );
        Self {
            sources,
            targets,
            readings,
            pairs,
        }
    }

    pub fn empty(sources: ObjectIds, targets: ObjectIds) -> Self {
        let readings = vec![RelationReading::unreached(); sources.len()];
        Self {
            sources,
            targets,
            readings,
            pairs: Vec::new(),
        }
    }

    pub fn sources(&self) -> &ObjectIds {
        &self.sources
    }

    pub fn targets(&self) -> &ObjectIds {
        &self.targets
    }

    pub fn reading(&self, source: ObjectId) -> Option<RelationReading> {
        self.sources
            .index_of(source)
            .map(|slot| self.readings[slot])
    }

    pub fn pairs(&self) -> &[RelationPair] {
        &self.pairs
    }
}
