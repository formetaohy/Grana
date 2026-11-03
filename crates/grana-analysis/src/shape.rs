use std::sync::Arc;

use grana_geometry::PackedMesh;
use grana_object::Spot;

const UNIT: f32 = 65535.0;

#[derive(Clone)]
pub enum Shape {
    Mesh(Arc<PackedMesh>),
    Spots(Arc<[Spot]>),
}

impl Shape {
    pub fn mesh(mesh: Arc<PackedMesh>) -> Self {
        Self::Mesh(mesh)
    }

    pub fn spots(spots: Arc<[Spot]>) -> Self {
        Self::Spots(spots)
    }

    pub fn objects(&self) -> usize {
        match self {
            Self::Mesh(mesh) => mesh.object_count(),
            Self::Spots(spots) => spots.len(),
        }
    }

    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        let mut take = |minimum: [f32; 3], maximum: [f32; 3]| {
            for axis in 0..3 {
                low[axis] = low[axis].min(minimum[axis]);
                high[axis] = high[axis].max(maximum[axis]);
            }
        };
        match self {
            Self::Spots(spots) => {
                for spot in spots.iter() {
                    let center = spot.center();
                    let radius = spot.radius();
                    take(
                        [0, 1, 2].map(|axis| center[axis] - radius),
                        [0, 1, 2].map(|axis| center[axis] + radius),
                    );
                }
            }
            Self::Mesh(mesh) => {
                for group in mesh.groups() {
                    let minimum = group.min();
                    let maximum = [0, 1, 2].map(|axis| minimum[axis] + UNIT * group.scale()[axis]);
                    take(minimum, maximum);
                }
            }
        }
        (low[0] <= high[0]).then_some((low, high))
    }
}
