use std::sync::Arc;

use grana_geometry::Ellipsoid;

use crate::{IntensityTable, ObjectId, ObjectIds};

pub const RADIUS_PER_SIGMA: f32 = 1.177_410_1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spot {
    center: [f32; 3],
    sigma: [f32; 3],
    quality: f32,
    group: Option<u32>,
}

impl Spot {
    pub fn new(center: [f32; 3], radius: f32, quality: f32) -> Self {
        assert!(
            radius.is_finite() && radius > 0.0,
            "a spot radius must be a positive length"
        );
        let sigma = radius / RADIUS_PER_SIGMA;
        Self::gaussian(center, [sigma; 3], quality)
    }

    pub fn gaussian(center: [f32; 3], sigma: [f32; 3], quality: f32) -> Self {
        Self::grouped(center, sigma, quality, None)
    }

    pub fn grouped(center: [f32; 3], sigma: [f32; 3], quality: f32, group: Option<u32>) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "a spot center must be finite"
        );
        assert!(
            sigma.iter().all(|value| value.is_finite() && *value > 0.0),
            "a spot sigma is a positive length on every axis"
        );
        assert!(
            quality.is_finite(),
            "a spot quality must be a finite contrast"
        );
        Self {
            center,
            sigma,
            quality,
            group,
        }
    }

    pub fn center(self) -> [f32; 3] {
        self.center
    }

    pub fn sigma(self) -> [f32; 3] {
        self.sigma
    }

    pub fn radii(self) -> [f32; 3] {
        self.sigma.map(|value| value * RADIUS_PER_SIGMA)
    }

    pub fn ellipsoid(self) -> Ellipsoid {
        Ellipsoid::new(self.center, self.radii())
    }

    pub fn equivalent_radius(self) -> f32 {
        self.ellipsoid().equivalent_radius()
    }

    pub fn quality(self) -> f32 {
        self.quality
    }

    pub fn group(self) -> Option<u32> {
        self.group
    }

    pub fn moved(self, center: [f32; 3]) -> Self {
        Self::grouped(center, self.sigma, self.quality, self.group)
    }

    pub fn with_group(self, group: Option<u32>) -> Self {
        Self { group, ..self }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpotSet {
    level: usize,
    ids: ObjectIds,
    spots: Arc<[Spot]>,
    quality: Option<f32>,
    intensities: IntensityTable,
}

impl SpotSet {
    pub fn new(
        level: usize,
        ids: ObjectIds,
        spots: Vec<Spot>,
        intensities: IntensityTable,
    ) -> Self {
        assert_eq!(
            intensities.objects(),
            spots.len(),
            "every spot is measured in every channel"
        );
        assert_eq!(ids.len(), spots.len(), "every spot carries one object id");
        let quality = (!spots.is_empty())
            .then(|| spots.iter().map(|spot| spot.quality).sum::<f32>() / spots.len() as f32);
        Self {
            level,
            ids,
            spots: spots.into(),
            quality,
            intensities,
        }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn ids(&self) -> &ObjectIds {
        &self.ids
    }

    pub fn id(&self, object: usize) -> Option<ObjectId> {
        self.ids.get(object)
    }

    pub fn index_of(&self, id: ObjectId) -> Option<usize> {
        self.ids.index_of(id)
    }

    pub fn spots(&self) -> &[Spot] {
        &self.spots
    }

    pub fn shared_spots(&self) -> Arc<[Spot]> {
        Arc::clone(&self.spots)
    }

    pub fn intensities(&self) -> &IntensityTable {
        &self.intensities
    }

    pub fn len(&self) -> usize {
        self.spots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spots.is_empty()
    }

    pub fn mean_quality(&self) -> Option<f32> {
        self.quality
    }

    pub fn without(&self, object: usize) -> Option<Self> {
        if object >= self.spots.len() {
            return None;
        }
        let mut spots = Vec::with_capacity(self.spots.len() - 1);
        spots.extend_from_slice(&self.spots[..object]);
        spots.extend_from_slice(&self.spots[object + 1..]);
        Some(Self::new(
            self.level,
            self.ids.without(object)?,
            spots,
            self.intensities.without(object),
        ))
    }

    pub fn retain(&self, keep: &[bool]) -> Self {
        assert_eq!(
            keep.len(),
            self.spots.len(),
            "the kept objects cover every spot"
        );
        let mut spots = Vec::with_capacity(keep.iter().filter(|kept| **kept).count());
        for (object, kept) in keep.iter().enumerate() {
            if *kept {
                spots.push(self.spots[object]);
            }
        }
        Self::new(
            self.level,
            self.ids.retained(keep),
            spots,
            self.intensities.retain(keep),
        )
    }

    pub fn merge(&self, other: &Self) -> Self {
        assert_eq!(
            self.level, other.level,
            "a merged spot set holds one resolution level"
        );
        let mut spots = Vec::with_capacity(self.spots.len() + other.spots.len());
        spots.extend_from_slice(&self.spots);
        spots.extend_from_slice(&other.spots);
        let ids = self
            .ids
            .appended(&other.ids)
            .unwrap_or_else(|| panic!("two merged spot sets name disjoint objects"));
        Self::new(
            self.level,
            ids,
            spots,
            self.intensities.merge(&other.intensities),
        )
    }

    pub fn reidentified(&self, ids: ObjectIds) -> Self {
        assert_eq!(
            ids.len(),
            self.spots.len(),
            "a reidentified spot set names every spot"
        );
        Self {
            ids,
            ..self.clone()
        }
    }

    pub fn regrouped(&self, groups: &[Option<u32>]) -> Self {
        assert_eq!(
            groups.len(),
            self.spots.len(),
            "the molecule of every spot is named"
        );
        let spots = self
            .spots
            .iter()
            .zip(groups)
            .map(|(spot, group)| spot.with_group(*group))
            .collect::<Vec<_>>();
        Self::new(
            self.level,
            self.ids.clone(),
            spots,
            self.intensities.clone(),
        )
    }
}
