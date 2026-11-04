#[derive(Clone, Debug, PartialEq)]
pub struct RelationFrame {
    distance: Vec<f32>,
    inside: Vec<f32>,
    contact: Vec<f32>,
    partner: Vec<u32>,
}

impl RelationFrame {
    pub fn new(distance: Vec<f32>, inside: Vec<f32>, contact: Vec<f32>, partner: Vec<u32>) -> Self {
        assert_eq!(
            distance.len(),
            inside.len(),
            "a relation frame measures every object it counts"
        );
        assert_eq!(
            distance.len(),
            contact.len(),
            "a relation frame measures the contact of every object"
        );
        assert_eq!(
            distance.len(),
            partner.len(),
            "a relation frame names a partner for every object"
        );
        assert!(
            distance.iter().all(|value| *value >= 0.0),
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
            inside,
            contact,
            partner,
        }
    }

    pub fn empty(objects: usize) -> Self {
        Self {
            distance: vec![f32::INFINITY; objects],
            inside: vec![0.0; objects],
            contact: vec![0.0; objects],
            partner: vec![0; objects],
        }
    }

    pub fn objects(&self) -> usize {
        self.distance.len()
    }

    pub fn distance(&self, object: usize) -> f32 {
        self.distance[object]
    }

    pub fn inside(&self, object: usize) -> f32 {
        self.inside[object]
    }

    pub fn contact(&self, object: usize) -> f32 {
        self.contact[object]
    }

    pub fn partner(&self, object: usize) -> u32 {
        self.partner[object]
    }
}
