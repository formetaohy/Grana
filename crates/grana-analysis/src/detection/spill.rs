use std::mem::size_of;

use grana_store::Scratch;

pub(super) enum Storage {
    Memory(Vec<f32>),
    Spill(Scratch),
}

impl Storage {
    pub(super) fn floats(&self, offset: usize, length: usize) -> Result<&[f32], String> {
        match self {
            Self::Memory(values) => Ok(&values[offset..offset + length]),
            Self::Spill(scratch) => {
                let start = offset * size_of::<f32>();
                let end = start + length * size_of::<f32>();
                let bytes = scratch
                    .bytes()
                    .and_then(|bytes| bytes.get(start..end))
                    .ok_or_else(|| {
                        format!("the detection spill does not hold the bytes {start}..{end}")
                    })?;
                Ok(bytemuck::cast_slice(bytes))
            }
        }
    }
}

pub(super) fn write(scratch: &mut Scratch, offset: usize, values: &[f32]) -> Result<(), String> {
    let start = offset * size_of::<f32>();
    let end = start + size_of_val(values);
    let bytes = scratch
        .bytes_mut()
        .map_err(|error| error.to_string())?
        .get_mut(start..end)
        .ok_or_else(|| format!("the detection spill does not hold the bytes {start}..{end}"))?;
    bytes.copy_from_slice(bytemuck::cast_slice(values));
    Ok(())
}

pub(super) fn acquire(bytes: u64) -> Result<Storage, String> {
    let scratch = Scratch::acquire(bytes)
        .map_err(|error| format!("the detection volume cannot spill its image: {error}"))?;
    Ok(Storage::Spill(scratch))
}
