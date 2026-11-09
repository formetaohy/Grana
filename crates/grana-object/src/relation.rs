#[derive(Clone, Debug, PartialEq)]
pub struct RelationFrame {
    distance: Vec<f32>,
    center: Vec<f32>,
    inside: Vec<f32>,
    contact: Vec<f32>,
    overlapping: Vec<u32>,
    partner: Vec<u32>,
}

impl RelationFrame {
    pub fn new(
        distance: Vec<f32>,
        center: Vec<f32>,
        inside: Vec<f32>,
        contact: Vec<f32>,
        overlapping: Vec<u32>,
        partner: Vec<u32>,
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
        Self {
            distance,
            center,
            inside,
            contact,
            overlapping,
            partner,
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
}
