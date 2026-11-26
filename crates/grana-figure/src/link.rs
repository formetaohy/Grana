use grana_object::LinkConfidence;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkStroke {
    pub width: f32,
    pub alpha: f32,
}

pub fn link_stroke(confidence: LinkConfidence) -> LinkStroke {
    let value = confidence.value().map(|value| value as f32);
    LinkStroke {
        width: value.map_or(1.1, |value| 0.6 + 0.7 * value),
        alpha: value.map_or(0.45, |value| 0.18 + 0.5 * value),
    }
}
