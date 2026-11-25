#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TriangleBudget(usize);

impl TriangleBudget {
    pub fn new(triangles: usize) -> Self {
        Self(triangles.max(1))
    }

    pub fn triangles(self) -> usize {
        self.0
    }

    pub fn raw(self) -> u64 {
        self.0 as u64
    }
}
