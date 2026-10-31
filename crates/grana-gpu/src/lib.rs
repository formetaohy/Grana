mod command;
mod download;
mod table;

pub const REQUIRED_FEATURES: wgpu::Features = wgpu::Features::TEXTURE_BINDING_ARRAY
    .union(wgpu::Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING)
    .union(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM);

pub use command::{IndexedCommands, InstanceCommands};
pub use download::download;
pub use table::Table;
