use wgpu::{Device, Error, ErrorFilter};

pub fn scoped<T>(device: &Device, build: impl FnOnce() -> T) -> Result<T, Error> {
    let validation = device.push_error_scope(ErrorFilter::Validation);
    let memory = device.push_error_scope(ErrorFilter::OutOfMemory);
    let value = build();
    let memory = pollster::block_on(memory.pop());
    let validation = pollster::block_on(validation.pop());
    match memory.or(validation) {
        Some(error) => Err(error),
        None => Ok(value),
    }
}
