mod figure;
mod overlay;

pub use figure::instances;
pub use overlay::{Legend, Overlay, legends, paint};

pub fn value(value: f32, unit: grana_measure::Unit, decimals: u8) -> String {
    let suffix = unit.suffix();
    if suffix.is_empty() {
        format!("{:.*}", decimals as usize, value)
    } else {
        format!("{:.*} {}", decimals as usize, value, suffix)
    }
}
