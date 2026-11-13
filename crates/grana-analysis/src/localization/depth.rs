pub const MAXIMUM_LAYERS: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalizationDepth {
    origin: f32,
    step: f32,
    layers: u32,
}

impl LocalizationDepth {
    pub fn projection(origin: f32) -> Self {
        assert!(
            origin.is_finite(),
            "a localization projection starts at a finite height"
        );
        Self {
            origin,
            step: 0.0,
            layers: 1,
        }
    }

    pub fn sliced(low: f32, high: f32, thickness: f32) -> Result<Self, String> {
        assert!(
            low.is_finite() && high.is_finite() && high >= low,
            "a localization stack spans a finite height"
        );
        assert!(
            thickness.is_finite() && thickness > 0.0,
            "a localization stack slices a positive thickness"
        );
        let span = high - low;
        let count = (f64::from(span) / f64::from(thickness)).ceil();
        if !count.is_finite() || count > f64::from(MAXIMUM_LAYERS) {
            return Err(format!(
                "the localizations span {:.3} µm in slices of {} nm; raise the slice depth",
                span,
                (thickness * 1000.0).round()
            ));
        }
        Ok(Self {
            origin: low,
            step: thickness,
            layers: (count as u32).max(1),
        })
    }

    pub fn origin(&self) -> f32 {
        self.origin
    }

    pub fn step(&self) -> f32 {
        self.step
    }

    pub fn layers(&self) -> u32 {
        self.layers
    }

    pub fn layer_of(&self, height: f32) -> u32 {
        if self.layers == 1 {
            return 0;
        }
        let index = (height - self.origin) / self.step;
        if !index.is_finite() {
            return 0;
        }
        (index.floor() as i64).clamp(0, i64::from(self.layers) - 1) as u32
    }

    pub fn lower(&self, layer: u32) -> f32 {
        self.origin + self.step * layer as f32
    }

    pub fn upper(&self, layer: u32) -> f32 {
        self.lower(layer) + self.step
    }
}
