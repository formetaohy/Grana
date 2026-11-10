mod annotation;
mod figure;
mod labels;
mod overlay;

pub use annotation::{
    Item as AnnotationItem, Part as AnnotationPart, Placed as PlacedAnnotation,
    collect as annotations, paint as paint_annotations, pending as pending_annotation,
    pick as pick_annotation, section as section_annotations, volume as volume_annotations,
};
pub use figure::{Contents, instances};
pub use labels::{Depth, Label, Placed, labels, section_labels, volume_labels};
pub use overlay::{Caption, Handle, Legend, Pane, caption, handles, legends, paint};

pub fn value(value: f32, unit: grana_measure::Unit, decimals: u8) -> String {
    let suffix = unit.suffix();
    if suffix.is_empty() {
        format!("{:.*}", decimals as usize, value)
    } else {
        format!("{:.*} {}", decimals as usize, value, suffix)
    }
}
