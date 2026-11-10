use grana_object::Algorithm;
use grana_volume::Fingerprint;

use crate::object_set::ObjectSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trace {
    algorithm: Algorithm,
    version: u32,
    input: Option<Fingerprint>,
    objects: usize,
}

impl Trace {
    pub fn of(set: &ObjectSet, input: Option<Fingerprint>) -> Self {
        let algorithm = set.provenance().algorithm();
        let objects = set
            .frames()
            .map(|(_, frame)| frame.object_count())
            .sum::<usize>();
        Self {
            algorithm,
            version: set.version(),
            input,
            objects,
        }
    }

    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn input(&self) -> Option<Fingerprint> {
        self.input
    }

    pub fn objects(&self) -> usize {
        self.objects
    }
}
