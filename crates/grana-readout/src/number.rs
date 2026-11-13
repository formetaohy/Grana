use hdf5_pure::{Datatype, DatatypeByteOrder};

pub(crate) fn size(datatype: &Datatype) -> Option<usize> {
    match datatype {
        Datatype::FloatingPoint { size, .. } | Datatype::FixedPoint { size, .. } => {
            Some(*size as usize)
        }
        _ => None,
    }
}

pub(crate) fn decode(datatype: &Datatype, bytes: &[u8]) -> Option<f64> {
    if bytes.len() > 8 {
        return None;
    }
    let big = matches!(
        datatype,
        Datatype::FloatingPoint {
            byte_order: DatatypeByteOrder::BigEndian,
            ..
        } | Datatype::FixedPoint {
            byte_order: DatatypeByteOrder::BigEndian,
            ..
        }
    );
    let mut buffer = [0u8; 8];
    if big {
        for (slot, byte) in bytes.iter().rev().enumerate() {
            buffer[slot] = *byte;
        }
    } else {
        buffer[..bytes.len()].copy_from_slice(bytes);
    }
    match datatype {
        Datatype::FloatingPoint { size: 8, .. } => Some(f64::from_le_bytes(buffer)),
        Datatype::FloatingPoint { size: 4, .. } => {
            Some(f64::from(f32::from_le_bytes(buffer[..4].try_into().ok()?)))
        }
        Datatype::FixedPoint { size, layout, .. } => {
            let value = u64::from_le_bytes(buffer);
            if layout.signed {
                let shift = 64 - 8 * *size;
                Some(((value << shift) as i64 >> shift) as f64)
            } else {
                Some(value as f64)
            }
        }
        _ => None,
    }
}
