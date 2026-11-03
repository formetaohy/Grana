mod decode;
mod encode;

pub use decode::Decoder;
pub use encode::Encoder;

pub type Fault = &'static str;

pub const FAULT_TRUNCATED: Fault = "the binary data ends before its structure closes";
pub const FAULT_LENGTH: Fault = "the binary data declares a length it cannot hold";
pub const FAULT_TAG: Fault = "the binary data carries an unknown variant tag";
pub const FAULT_UTF8: Fault = "the binary data carries text that is not utf-8";
pub const FAULT_TRAILING: Fault = "the binary data carries trailing bytes";
pub const FAULT_CANCELLED: Fault = "the binary transfer was cancelled";
pub const FAULT_VALUE: Fault = "the binary data carries a value outside its domain";

pub(crate) const STRING_LIMIT: usize = 1 << 20;
