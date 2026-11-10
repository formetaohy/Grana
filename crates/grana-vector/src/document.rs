use grana_text::Contour;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

impl Rgba {
    pub fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        assert!(
            [red, green, blue, alpha]
                .iter()
                .all(|value| (0.0..=1.0).contains(value)),
            "a vector colour lies in the unit cube"
        );
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub fn transparent(self) -> bool {
        self.alpha <= 0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Fill(Rgba),
    Gradient(Rgba, Rgba),
    Stroke { width: f32, color: Rgba, cap: Cap },
}

impl Paint {
    pub fn visible(self) -> bool {
        match self {
            Self::Fill(color) => !color.transparent(),
            Self::Gradient(top, bottom) => !top.transparent() || !bottom.transparent(),
            Self::Stroke { width, color, .. } => width > 0.0 && !color.transparent(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl Rect {
    pub fn new(min: [f32; 2], max: [f32; 2]) -> Self {
        assert!(
            min.iter().chain(&max).all(|value| value.is_finite()),
            "a vector rectangle spans finite points"
        );
        Self { min, max }
    }

    pub fn width(self) -> f32 {
        self.max[0] - self.min[0]
    }

    pub fn height(self) -> f32 {
        self.max[1] - self.min[1]
    }

    pub fn is_empty(self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub contours: Vec<Contour>,
    pub paint: Paint,
    pub clip: Option<Rect>,
}

impl Shape {
    pub fn new(contours: Vec<Contour>, paint: Paint) -> Self {
        Self {
            contours,
            paint,
            clip: None,
        }
    }

    pub fn clipped(mut self, clip: Option<Rect>) -> Self {
        self.clip = clip.filter(|rect| !rect.is_empty());
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    size: [u32; 2],
    pixels: Vec<u8>,
}

impl Image {
    pub fn new(size: [u32; 2], pixels: Vec<u8>) -> Self {
        assert!(
            size[0] > 0 && size[1] > 0,
            "a vector image covers at least one pixel"
        );
        assert_eq!(
            pixels.len(),
            size[0] as usize * size[1] as usize * 4,
            "a vector image holds four bytes per pixel"
        );
        Self { size, pixels }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    width: f32,
    height: f32,
    background: Option<Image>,
    shapes: Vec<Shape>,
}

impl Document {
    pub fn new(width: f32, height: f32) -> Self {
        assert!(
            width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0,
            "a vector page spans a positive size"
        );
        Self {
            width,
            height,
            background: None,
            shapes: Vec::new(),
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn background(&self) -> Option<&Image> {
        self.background.as_ref()
    }

    pub fn set_background(&mut self, image: Image) {
        self.background = Some(image);
    }

    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    pub fn push(&mut self, shape: Shape) {
        if shape.contours.is_empty() || !shape.paint.visible() {
            return;
        }
        self.shapes.push(shape);
    }
}
