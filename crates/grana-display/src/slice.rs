use glam::Vec3;

const UNIT_TOLERANCE: f32 = 1.0e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slice {
    axis: u8,
    right: Vec3,
    up: Vec3,
    step: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    normal: Vec3,
    offset: f32,
    thickness: f32,
}

impl Slice {
    pub fn new(axis: usize, right: [f32; 3], up: [f32; 3], step: f32) -> Self {
        Self::restore(axis, right, up, step).expect("a slice holds an orthonormal plane frame")
    }

    pub fn restore(axis: usize, right: [f32; 3], up: [f32; 3], step: f32) -> Option<Self> {
        let right = Vec3::from(right);
        let up = Vec3::from(up);
        if axis >= 3
            || !right.is_finite()
            || !up.is_finite()
            || (right.length() - 1.0).abs() > UNIT_TOLERANCE
            || (up.length() - 1.0).abs() > UNIT_TOLERANCE
            || right.dot(up).abs() > UNIT_TOLERANCE
            || !step.is_finite()
            || step <= 0.0
        {
            return None;
        }
        Some(Self {
            axis: axis as u8,
            right,
            up,
            step,
        })
    }

    pub fn axis(self) -> usize {
        usize::from(self.axis)
    }

    pub fn right(self) -> Vec3 {
        self.right
    }

    pub fn up(self) -> Vec3 {
        self.up
    }

    pub fn normal(self) -> Vec3 {
        self.right.cross(self.up)
    }

    pub fn forward(self) -> Vec3 {
        -self.normal()
    }

    pub fn step(self) -> f32 {
        self.step
    }

    pub fn plane(self, pivot: Vec3) -> Plane {
        let normal = self.normal();
        Plane {
            normal,
            offset: normal.dot(pivot),
            thickness: self.step,
        }
    }
}

impl Plane {
    pub fn normal(self) -> Vec3 {
        self.normal
    }

    pub fn offset(self) -> f32 {
        self.offset
    }

    pub fn thickness(self) -> f32 {
        self.thickness
    }
}
