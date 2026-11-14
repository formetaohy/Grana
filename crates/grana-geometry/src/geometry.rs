use std::sync::Arc;

use grana_store::{Decode, Error, SpillStore, Stored};

use crate::packed::PackedMesh;

pub struct Geometry<T> {
    stored: Stored<T>,
}

impl<T: Decode> Geometry<T> {
    pub fn resident(payload: T) -> Self {
        Self {
            stored: Stored::resident(payload),
        }
    }

    pub fn paged(store: &Arc<SpillStore>, payload: T) -> Result<Self, Error> {
        Ok(Self {
            stored: Stored::paged(store, payload)?,
        })
    }

    pub fn load(&self) -> Result<Arc<T>, Error> {
        self.stored.load()
    }

    pub fn bytes(&self) -> u64 {
        self.stored.bytes()
    }

    pub fn replace(&self, payload: T) -> Result<Self, Error> {
        Ok(Self {
            stored: self.stored.replace(payload)?,
        })
    }
}

impl Geometry<PackedMesh> {
    pub fn without(&self, object: usize) -> Result<Option<Self>, Error> {
        let mesh = self.load()?;
        if object >= mesh.object_count() {
            return Ok(None);
        }
        self.replace(mesh.without(object)).map(Some)
    }

    pub fn retain(&self, keep: &[bool]) -> Result<Self, Error> {
        let mesh = self.load()?;
        assert_eq!(
            keep.len(),
            mesh.object_count(),
            "the kept objects cover every packed object"
        );
        self.replace(mesh.retain(keep))
    }

    pub fn merge(&self, other: &Self) -> Result<Self, Error> {
        let mesh = self.load()?;
        let absorbed = other.load()?;
        self.replace(mesh.merge(&absorbed))
    }
}

impl<T> Clone for Geometry<T> {
    fn clone(&self) -> Self {
        Self {
            stored: self.stored.clone(),
        }
    }
}

impl<T: Decode + std::fmt::Debug> std::fmt::Debug for Geometry<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.stored.fmt(formatter)
    }
}

impl<T: Decode + PartialEq> PartialEq for Geometry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.stored == other.stored
    }
}
