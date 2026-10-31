use std::sync::mpsc;

use bytemuck::Pod;
use wgpu::{Buffer, BufferDescriptor, BufferUsages, Device, MapMode, PollType, Queue};

pub fn download<T: Pod>(device: &Device, queue: &Queue, buffer: &Buffer, len: usize) -> Vec<T> {
    assert!(len > 0, "a download copies at least one row");
    let size = (len * size_of::<T>()) as u64;
    let staging = device.create_buffer(&BufferDescriptor {
        label: Some("grana.download"),
        size,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    queue.submit([encoder.finish()]);
    let slice = staging.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(PollType::wait_indefinitely())
        .expect("the graphics device failed while downloading a buffer");
    receiver
        .recv()
        .expect("the download callback fires once")
        .expect("the download staging buffer is mappable");
    let mapped = slice
        .get_mapped_range()
        .expect("the downloaded staging buffer stays mapped");
    let values = (0..len)
        .map(|index| {
            let start = index * size_of::<T>();
            bytemuck::pod_read_unaligned(&mapped[start..start + size_of::<T>()])
        })
        .collect();
    drop(mapped);
    staging.unmap();
    values
}
