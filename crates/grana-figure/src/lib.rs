mod figure;
mod labels;
mod overlay;

pub use figure::{Contents, instances};
pub use labels::{Label, Placed, labels, section_labels, volume_labels};
pub use overlay::{Caption, Handle, Legend, Overlay, caption, handles, legends, paint};

pub fn value(value: f32, unit: grana_measure::Unit, decimals: u8) -> String {
    let suffix = unit.suffix();
    if suffix.is_empty() {
        format!("{:.*}", decimals as usize, value)
    } else {
        format!("{:.*} {}", decimals as usize, value, suffix)
    }
}
