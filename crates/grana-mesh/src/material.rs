#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    color: [f32; 4],
}

impl Material {
    pub fn new(color: [f32; 3], opacity: f32) -> Self {
        assert!(
            color.iter().all(|value| (0.0..=1.0).contains(value)),
            "a material color is a linear triple"
        );
        assert!(
            (0.0..=1.0).contains(&opacity),
            "a material opacity lies in the unit interval"
        );
        Self {
            color: [color[0], color[1], color[2], opacity],
        }
    }

    pub fn color(self) -> [f32; 3] {
        [self.color[0], self.color[1], self.color[2]]
    }

    pub fn opacity(self) -> f32 {
        self.color[3]
    }
}
