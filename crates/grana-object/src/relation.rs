#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelationPair {
    source: u32,
    target: u32,
    contact: f32,
    distance: f32,
    inside: f32,
    center: f32,
}

impl RelationPair {
    pub fn new(
        source: u32,
        target: u32,
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

    pub fn source(self) -> u32 {
        self.source
    }

    pub fn target(self) -> u32 {
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

#[derive(Clone, Debug, PartialEq)]
pub struct RelationFrame {
    distance: Vec<f32>,
    center: Vec<f32>,
    inside: Vec<f32>,
    contact: Vec<f32>,
    overlapping: Vec<u32>,
    partner: Vec<u32>,
    pairs: Vec<RelationPair>,
}

impl RelationFrame {
    pub fn new(
        distance: Vec<f32>,
        center: Vec<f32>,
        inside: Vec<f32>,
        contact: Vec<f32>,
        overlapping: Vec<u32>,
        partner: Vec<u32>,
        pairs: Vec<RelationPair>,
    ) -> Self {
        let objects = distance.len();
        assert_eq!(
            center.len(),
            objects,
            "a relation frame centres the objects it measures"
        );
        assert_eq!(
            inside.len(),
            objects,
            "a relation frame measures every object it counts"
        );
        assert_eq!(
            contact.len(),
            objects,
            "a relation frame measures the contact of every object"
        );
        assert_eq!(
            overlapping.len(),
            objects,
            "a relation frame counts the partners of every object"
        );
        assert_eq!(
            partner.len(),
            objects,
            "a relation frame names a partner for every object"
        );
        assert!(
            distance.iter().chain(&center).all(|value| *value >= 0.0),
            "a relation distance is a length"
        );
        assert!(
            inside.iter().all(|value| (0.0..=1.0).contains(value)),
            "a relation containment is a fraction"
        );
        assert!(
            contact
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
            "a relation contact is an area"
        );
        assert!(
            pairs.windows(2).all(|pair| {
                (pair[0].source(), pair[0].target()) < (pair[1].source(), pair[1].target())
            }),
            "a relation frame lists each pair once in source and target order"
        );
        assert!(
            pairs.iter().all(|pair| (pair.source() as usize) < objects),
            "a relation pair names a source object of the frame"
        );
        Self {
            distance,
            center,
            inside,
            contact,
            overlapping,
            partner,
            pairs,
        }
    }

    pub fn empty(objects: usize) -> Self {
        Self {
            distance: vec![f32::INFINITY; objects],
            center: vec![f32::INFINITY; objects],
            inside: vec![0.0; objects],
            contact: vec![0.0; objects],
            overlapping: vec![0; objects],
            partner: vec![0; objects],
            pairs: Vec::new(),
        }
    }

    pub fn objects(&self) -> usize {
        self.distance.len()
    }

    pub fn distance(&self, object: usize) -> f32 {
        self.distance[object]
    }

    pub fn center(&self, object: usize) -> f32 {
        self.center[object]
    }

    pub fn inside(&self, object: usize) -> f32 {
        self.inside[object]
    }

    pub fn contact(&self, object: usize) -> f32 {
        self.contact[object]
    }

    pub fn overlapping(&self, object: usize) -> u32 {
        self.overlapping[object]
    }

    pub fn partner(&self, object: usize) -> u32 {
        self.partner[object]
    }

    pub fn pairs(&self) -> &[RelationPair] {
        &self.pairs
    }
}
