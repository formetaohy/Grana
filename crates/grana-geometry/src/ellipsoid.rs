use glam::Vec3;

const PARALLEL: f32 = 1.0e-12;
const BISECTIONS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipsoid {
    center: [f32; 3],
    radii: [f32; 3],
}

impl Ellipsoid {
    pub fn new(center: [f32; 3], radii: [f32; 3]) -> Self {
        assert!(
            center.iter().all(|value| value.is_finite()),
            "an ellipsoid lies at a finite center"
        );
        assert!(
            radii.iter().all(|value| value.is_finite() && *value > 0.0),
            "an ellipsoid carries a positive semi axis on every axis"
        );
        Self { center, radii }
    }

    pub fn sphere(center: [f32; 3], radius: f32) -> Self {
        Self::new(center, [radius; 3])
    }

    pub fn center(self) -> [f32; 3] {
        self.center
    }

    pub fn radii(self) -> [f32; 3] {
        self.radii
    }

    pub fn bounds(self) -> ([f32; 3], [f32; 3]) {
        let reach = self.radii;
        (
            [0, 1, 2].map(|axis| self.center[axis] - reach[axis]),
            [0, 1, 2].map(|axis| self.center[axis] + reach[axis]),
        )
    }

    pub fn equivalent_radius(self) -> f32 {
        (self.radii[0] * self.radii[1] * self.radii[2]).cbrt()
    }

    pub fn eccentricity(self) -> f32 {
        let low = self.radii[0].min(self.radii[1]).min(self.radii[2]);
        let high = self.radii[0].max(self.radii[1]).max(self.radii[2]);
        high / low
    }

    pub fn contains(self, point: [f32; 3]) -> bool {
        let mut total = 0.0f32;
        for axis in 0..3 {
            let offset = (point[axis] - self.center[axis]) / self.radii[axis];
            total += offset * offset;
        }
        total <= 1.0
    }

    pub fn distance(self, point: [f32; 3]) -> f32 {
        let offset = Vec3::from(point) - Vec3::from(self.center);
        if self.contains(point) {
            return 0.0;
        }
        let radii = Vec3::from(self.radii);
        if self.eccentricity() <= 1.0 + 1.0e-4 {
            return (offset.length() - self.equivalent_radius()).max(0.0);
        }
        let square = radii * radii;
        let scaled = square * offset;
        let mut low = 0.0f32;
        let mut high = PARALLEL.max(2.0 * scaled.length());
        for _ in 0..BISECTIONS {
            let middle = 0.5 * (low + high);
            let reach = Vec3::splat(middle) + square;
            if (scaled / reach).length_squared() > 1.0 {
                low = middle;
            } else {
                high = middle;
            }
        }
        let reach = Vec3::splat(0.5 * (low + high)) + square;
        let nearest = radii * (square * offset / reach);
        offset.distance(nearest)
    }

    pub fn span(self, origin: Vec3, direction: Vec3) -> Option<(f32, f32)> {
        let scaled_origin = (origin - Vec3::from(self.center)) / Vec3::from(self.radii);
        let scaled_direction = direction / Vec3::from(self.radii);
        let squared = scaled_direction.length_squared();
        if squared <= PARALLEL {
            return None;
        }
        let projection = scaled_origin.dot(scaled_direction);
        let constant = scaled_origin.length_squared() - 1.0;
        let discriminant = projection * projection - squared * constant;
        if discriminant <= 0.0 {
            return None;
        }
        let root = discriminant.sqrt();
        Some((
            (-projection - root) / squared,
            (-projection + root) / squared,
        ))
    }
}
