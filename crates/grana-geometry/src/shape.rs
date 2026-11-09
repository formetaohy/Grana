use glam::DVec3;

use crate::hull::Hull;

const TETRAHEDRON_COVARIANCE: f64 = 1.0 / 20.0;
const ELLIPSOID_MOMENT: f64 = 5.0;
const ROTATIONS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceShape {
    axes: [f64; 3],
    diameter: f64,
    hull_volume: f64,
}

impl SurfaceShape {
    pub fn new(axes: [f64; 3], diameter: f64, hull_volume: f64) -> Self {
        assert!(
            axes.iter().all(|axis| axis.is_finite() && *axis >= 0.0),
            "a surface ellipsoid carries finite non negative semi axes"
        );
        assert!(
            axes[0] >= axes[1] && axes[1] >= axes[2],
            "a surface ellipsoid orders its semi axes"
        );
        assert!(
            diameter.is_finite() && diameter >= 0.0,
            "a surface diameter is a finite distance"
        );
        assert!(
            hull_volume.is_finite() && hull_volume >= 0.0,
            "a surface hull volume is a finite volume"
        );
        Self {
            axes,
            diameter,
            hull_volume,
        }
    }

    pub fn of(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> Option<Self> {
        if positions.len() < 4 || triangles.len() < 4 {
            return None;
        }
        let (centroid, moment) = moments(positions, triangles)?;
        let hull = Hull::of(positions)?;
        let mut covariance = [[0.0f64; 3]; 3];
        for row in 0..3 {
            for column in 0..3 {
                covariance[row][column] = moment[row][column] - centroid[row] * centroid[column];
            }
        }
        let values = eigenvalues(covariance);
        let axes = values.map(|value| (ELLIPSOID_MOMENT * value).max(0.0).sqrt());
        if axes.iter().any(|axis| !axis.is_finite()) {
            return None;
        }
        Some(Self::new(axes, hull.diameter(), hull.volume()))
    }

    pub fn axes(&self) -> [f64; 3] {
        self.axes
    }

    pub fn diameter(&self) -> f64 {
        self.diameter
    }

    pub fn hull_volume(&self) -> f64 {
        self.hull_volume
    }
}

fn position(positions: &[[f32; 3]], index: u32) -> Option<DVec3> {
    let point = positions.get(index as usize)?;
    Some(DVec3::new(
        f64::from(point[0]),
        f64::from(point[1]),
        f64::from(point[2]),
    ))
}

fn moments(positions: &[[f32; 3]], triangles: &[[u32; 3]]) -> Option<([f64; 3], [[f64; 3]; 3])> {
    let mut volume = 0.0f64;
    let mut first = DVec3::ZERO;
    let mut second = [[0.0f64; 3]; 3];
    for triangle in triangles {
        let a = position(positions, triangle[0])?;
        let b = position(positions, triangle[1])?;
        let c = position(positions, triangle[2])?;
        let signed = a.dot(b.cross(c)) / 6.0;
        if !signed.is_finite() {
            return None;
        }
        volume += signed;
        let centre = (a + b + c) / 4.0;
        first += signed * centre;
        let mut covariance = [[0.0f64; 3]; 3];
        for point in [DVec3::ZERO, a, b, c] {
            let offset = point - centre;
            for row in 0..3 {
                for column in 0..3 {
                    covariance[row][column] += offset[row] * offset[column];
                }
            }
        }
        for row in 0..3 {
            for column in 0..3 {
                second[row][column] += signed
                    * (TETRAHEDRON_COVARIANCE * covariance[row][column]
                        + centre[row] * centre[column]);
            }
        }
    }
    if !volume.is_finite() || volume.abs() <= f64::MIN_POSITIVE {
        return None;
    }
    let centroid = first / volume;
    let mut moment = [[0.0f64; 3]; 3];
    for row in 0..3 {
        for column in 0..3 {
            moment[row][column] = second[row][column] / volume;
        }
    }
    Some((centroid.to_array(), moment))
}

fn eigenvalues(matrix: [[f64; 3]; 3]) -> [f64; 3] {
    let mut a = matrix;
    for _ in 0..ROTATIONS {
        let off = a[0][1] * a[0][1] + a[0][2] * a[0][2] + a[1][2] * a[1][2];
        let diagonal = a[0][0] * a[0][0] + a[1][1] * a[1][1] + a[2][2] * a[2][2];
        if off <= 1.0e-24 * diagonal.max(f64::MIN_POSITIVE) {
            break;
        }
        rotate(&mut a, 0, 1);
        rotate(&mut a, 0, 2);
        rotate(&mut a, 1, 2);
    }
    let mut values = [a[0][0], a[1][1], a[2][2]];
    values.sort_by(|left, right| right.total_cmp(left));
    values
}

fn rotate(matrix: &mut [[f64; 3]; 3], p: usize, q: usize) {
    let off = matrix[p][q];
    if off.abs() <= f64::MIN_POSITIVE {
        return;
    }
    let theta = (matrix[q][q] - matrix[p][p]) / (2.0 * off);
    let sign = if theta >= 0.0 { 1.0 } else { -1.0 };
    let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
    let cosine = 1.0 / (t * t + 1.0).sqrt();
    let sine = t * cosine;
    let (pp, qq) = (matrix[p][p], matrix[q][q]);
    matrix[p][p] = pp - t * off;
    matrix[q][q] = qq + t * off;
    matrix[p][q] = 0.0;
    matrix[q][p] = 0.0;
    let axis = 3 - p - q;
    let (ap, aq) = (matrix[axis][p], matrix[axis][q]);
    let across = cosine * ap - sine * aq;
    let along = sine * ap + cosine * aq;
    matrix[axis][p] = across;
    matrix[p][axis] = across;
    matrix[axis][q] = along;
    matrix[q][axis] = along;
}
