use glam::{Mat3, Vec3};

use crate::{Extent, Spacing};

const ALIGNMENT_TOLERANCE: f32 = 1.0e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    matrix: Mat3,
    inverse: Mat3,
}

impl Basis {
    pub fn new(columns: [[f32; 3]; 3]) -> Self {
        let [x, y, z] = columns.map(Vec3::from);
        let matrix = Mat3::from_cols(x, y, z);
        assert!(matrix.is_finite(), "a basis holds finite voxel steps");
        let determinant = matrix.determinant();
        let volume = matrix.col(0).length() * matrix.col(1).length() * matrix.col(2).length();
        assert!(
            determinant.is_finite() && determinant.abs() > 1.0e-6 * volume && volume > 0.0,
            "a basis maps array coordinates bijectively"
        );
        Self {
            matrix,
            inverse: matrix.inverse(),
        }
    }

    pub fn diagonal(spacing: Spacing) -> Self {
        Self::new([
            [spacing.x(), 0.0, 0.0],
            [0.0, spacing.y(), 0.0],
            [0.0, 0.0, spacing.z()],
        ])
    }

    pub fn matrix(self) -> Mat3 {
        self.matrix
    }

    pub fn inverse(self) -> Mat3 {
        self.inverse
    }

    pub fn column(self, axis: usize) -> [f32; 3] {
        self.matrix.col(axis).to_array()
    }

    pub fn step(self, axis: usize) -> f32 {
        self.matrix.col(axis).length()
    }

    pub fn spacing(self) -> Spacing {
        Spacing::new(self.step(0), self.step(1), self.step(2))
    }

    pub fn transform(self, vector: [f32; 3]) -> [f32; 3] {
        (self.matrix * Vec3::from(vector)).to_array()
    }

    pub fn untransform(self, vector: [f32; 3]) -> [f32; 3] {
        (self.inverse * Vec3::from(vector)).to_array()
    }

    pub fn diagonal_spacing(self) -> Option<Spacing> {
        let matrix = self.matrix;
        let columns = [matrix.col(0), matrix.col(1), matrix.col(2)];
        let aligned = (0..3).all(|left| {
            (0..3).all(|right| {
                left == right
                    || columns[left][right].abs()
                        <= ALIGNMENT_TOLERANCE * columns[right][right].abs()
            })
        });
        (aligned && (0..3).all(|axis| columns[axis][axis] > 0.0)).then(|| self.spacing())
    }

    pub fn orthogonal(self) -> bool {
        let matrix = self.matrix;
        let columns = [matrix.col(0), matrix.col(1), matrix.col(2)];
        (0..3).all(|left| {
            (0..3).all(|right| {
                left == right
                    || columns[left].dot(columns[right]).abs()
                        <= ALIGNMENT_TOLERANCE * columns[left].length() * columns[right].length()
            })
        })
    }

    pub fn extent(self, extent: Extent) -> (Vec3, Vec3) {
        let reach = Vec3::new(extent.x() as f32, extent.y() as f32, extent.z() as f32);
        let columns = [self.matrix.col(0), self.matrix.col(1), self.matrix.col(2)];
        let mut low = Vec3::ZERO;
        let mut high = Vec3::ZERO;
        for (axis, column) in columns.into_iter().enumerate() {
            let offset = column * reach[axis];
            low += offset.min(Vec3::ZERO);
            high += offset.max(Vec3::ZERO);
        }
        (low, high)
    }
}
