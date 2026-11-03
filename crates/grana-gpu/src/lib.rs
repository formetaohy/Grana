mod buffer;
mod command;
mod device;
pub mod layout;
mod readback;
mod table;
mod target;

pub const REQUIRED_FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

pub use buffer::storage as storage_buffer;
pub use command::{IndexedCommands, InstanceCommands};
pub use device::{Gpu, HEADLESS_FORMAT, Headless};
pub use readback::Readback;
pub use table::Table;
pub use target::Target;
