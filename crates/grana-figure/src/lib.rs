mod annotation;
mod figure;
mod labels;
mod overlay;

pub use annotation::{
    Item as AnnotationItem, Part as AnnotationPart, Placed as PlacedAnnotation,
    collect as annotations, pick as pick_annotation, volume as volume_annotations,
};
pub use figure::{Contents, instances, marks};
pub use labels::{Depth, Label, Placed as PlacedLabel, labels, volume_labels};
pub use overlay::{Caption, Handle, Legend, Viewport, caption, handles, legends, paint};

pub fn value(value: f32, unit: grana_measure::Unit, decimals: u8) -> String {
    let suffix = unit.suffix();
    if suffix.is_empty() {
        format!("{:.*}", decimals as usize, value)
    } else {
        format!("{:.*} {}", decimals as usize, value, suffix)
    }
}
