use grana_gpu::Pipelines;
use wgpu::{Device, Queue, TextureFormat};

pub(crate) struct Surface<'a> {
    pub(crate) device: &'a Device,
    pub(crate) queue: &'a Queue,
    pub(crate) format: TextureFormat,
    pub(crate) pipelines: &'a Pipelines,
}
