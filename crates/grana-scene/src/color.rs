use grana_render::Palette;

pub type Extent = (f32, f32);

#[derive(Clone, Debug, PartialEq)]
pub struct Coloring<M> {
    metric: Option<M>,
    palette: Palette,
    reversed: bool,
    manual: Option<Extent>,
}

impl<M> Default for Coloring<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M> Coloring<M> {
    pub fn new() -> Self {
        Self {
            metric: None,
            palette: Palette::Viridis,
            reversed: false,
            manual: None,
        }
    }

    pub fn palette(&self) -> Palette {
        self.palette
    }

    pub fn reversed(&self) -> bool {
        self.reversed
    }

    pub fn manual(&self) -> Option<Extent> {
        self.manual
    }

    pub fn set_palette(&mut self, palette: Palette) -> bool {
        if self.palette == palette {
            return false;
        }
        self.palette = palette;
        true
    }

    pub fn set_reversed(&mut self, reversed: bool) -> bool {
        if self.reversed == reversed {
            return false;
        }
        self.reversed = reversed;
        true
    }

    pub fn set_manual(&mut self, manual: Option<Extent>) -> bool {
        let manual = manual.map(|(first, second)| {
            assert!(
                first.is_finite() && second.is_finite(),
                "a color range is finite"
            );
            if first <= second {
                (first, second)
            } else {
                (second, first)
            }
        });
        if self.manual == manual {
            return false;
        }
        self.manual = manual;
        true
    }

    pub fn color(&self, extent: Extent, value: f32) -> [f32; 3] {
        let (minimum, maximum) = extent;
        let position = if value.is_finite() && maximum > minimum {
            ((value - minimum) / (maximum - minimum)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.palette.sample(if self.reversed {
            1.0 - position
        } else {
            position
        })
    }

    pub fn state(&self) -> ColorState<M>
    where
        M: Copy,
    {
        ColorState {
            metric: self.metric,
            palette: self.palette,
            reversed: self.reversed,
            manual: self.manual,
        }
    }

    pub fn restore(state: ColorState<M>) -> Self {
        Self {
            metric: state.metric,
            palette: state.palette,
            reversed: state.reversed,
            manual: state.manual,
        }
    }
}

impl<M: Copy> Coloring<M> {
    pub fn metric(&self) -> Option<M> {
        self.metric
    }
}

impl<M: PartialEq> Coloring<M> {
    pub fn set_metric(&mut self, metric: Option<M>) -> bool {
        if self.metric == metric {
            return false;
        }
        self.metric = metric;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorState<M> {
    pub metric: Option<M>,
    pub palette: Palette,
    pub reversed: bool,
    pub manual: Option<Extent>,
}

pub fn merged(bounds: impl Iterator<Item = Extent>) -> Option<Extent> {
    bounds.fold(None, |accumulated, (minimum, maximum)| {
        Some(match accumulated {
            None => (minimum, maximum),
            Some((low, high)) => (low.min(minimum), high.max(maximum)),
        })
    })
}
