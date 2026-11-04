#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Extent {
    x: u32,
    y: u32,
    z: u32,
}

impl Extent {
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        assert!(
            x > 0 && y > 0 && z > 0,
            "extent dimensions must be positive"
        );
        Self { x, y, z }
    }

    pub const fn x(self) -> u32 {
        self.x
    }

    pub const fn y(self) -> u32 {
        self.y
    }

    pub const fn z(self) -> u32 {
        self.z
    }

    pub const fn voxel_count(self) -> u64 {
        self.x as u64 * self.y as u64 * self.z as u64
    }

    pub const fn max_dimension(self) -> u32 {
        let mut max = self.x;
        if self.y > max {
            max = self.y;
        }
        if self.z > max {
            max = self.z;
        }
        max
    }

    pub const fn index(self, x: u32, y: u32, z: u32) -> u64 {
        assert!(
            x < self.x && y < self.y && z < self.z,
            "voxel index out of bounds"
        );
        x as u64 + self.x as u64 * (y as u64 + self.y as u64 * z as u64)
    }
}
