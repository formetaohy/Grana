use std::io::{Read, Write};
use std::sync::Arc;

const FAULT_TRUNCATED: &str = "the project data ends before its structure closes";
const FAULT_LENGTH: &str = "the project data declares a length it cannot hold";
const FAULT_TAG: &str = "the project data carries an unknown variant tag";
const FAULT_UTF8: &str = "the project data carries text that is not utf-8";
const FAULT_TRAILING: &str = "the project data carries trailing bytes";
const STRING_LIMIT: usize = 1 << 20;

pub(crate) trait Encode {
    fn encode(&self, writer: &mut Writer<'_>);
}

pub(crate) trait Decode: Sized {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str>;
}

impl Encode for bool {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.bool(*self);
    }
}

impl Encode for u8 {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.u8(*self);
    }
}

impl Encode for u16 {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.u16(*self);
    }
}

impl Encode for u32 {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.u32(*self);
    }
}

impl Encode for u64 {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.u64(*self);
    }
}

impl Encode for f32 {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.f32(*self);
    }
}

impl Encode for f64 {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.f64(*self);
    }
}

impl Encode for String {
    fn encode(&self, writer: &mut Writer<'_>) {
        writer.string(self);
    }
}

impl<T: Encode + ?Sized> Encode for Arc<T> {
    fn encode(&self, writer: &mut Writer<'_>) {
        (**self).encode(writer);
    }
}

impl Decode for bool {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.bool()
    }
}

impl Decode for u8 {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.u8()
    }
}

impl Decode for u16 {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.u16()
    }
}

impl Decode for u32 {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.u32()
    }
}

impl Decode for u64 {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.u64()
    }
}

impl Decode for f32 {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.f32()
    }
}

impl Decode for f64 {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.f64()
    }
}

impl Decode for String {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        reader.string()
    }
}

impl<T: Decode> Decode for Arc<T> {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str> {
        Ok(Arc::new(T::decode(reader)?))
    }
}

pub(crate) struct Writer<'a> {
    sink: &'a mut dyn Write,
    raw: usize,
    fault: Option<std::io::Error>,
}

impl<'a> Writer<'a> {
    pub fn new(sink: &'a mut dyn Write) -> Self {
        Self {
            sink,
            raw: 0,
            fault: None,
        }
    }

    pub fn raw(&self) -> usize {
        self.raw
    }

    pub fn fault(&self) -> Option<&std::io::Error> {
        self.fault.as_ref()
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    pub fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    pub fn f32(&mut self, value: f32) {
        self.bytes(&value.to_le_bytes());
    }

    pub fn f64(&mut self, value: f64) {
        self.bytes(&value.to_le_bytes());
    }

    pub fn count(&mut self, value: usize) {
        assert!(
            u32::try_from(value).is_ok(),
            "a project holds fewer than four billion entries per array"
        );
        self.u32(value as u32);
    }

    pub fn string(&mut self, value: &str) {
        self.count(value.len());
        self.bytes(value.as_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        if self.fault.is_some() {
            return;
        }
        match self.sink.write_all(bytes) {
            Ok(()) => self.raw += bytes.len(),
            Err(error) => self.fault = Some(error),
        }
    }
}

pub(crate) struct Reader<'a> {
    source: &'a mut dyn Read,
    remaining: usize,
}

impl<'a> Reader<'a> {
    pub fn new(source: &'a mut dyn Read, length: usize) -> Self {
        Self {
            source,
            remaining: length,
        }
    }

    pub fn u8(&mut self) -> Result<u8, &'static str> {
        let mut bytes = [0u8; 1];
        self.fill(&mut bytes)?;
        Ok(bytes[0])
    }

    pub fn bool(&mut self) -> Result<bool, &'static str> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(FAULT_TAG),
        }
    }

    pub fn u16(&mut self) -> Result<u16, &'static str> {
        let mut bytes = [0u8; 2];
        self.fill(&mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    pub fn u32(&mut self) -> Result<u32, &'static str> {
        let mut bytes = [0u8; 4];
        self.fill(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn u64(&mut self) -> Result<u64, &'static str> {
        let mut bytes = [0u8; 8];
        self.fill(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn f32(&mut self) -> Result<f32, &'static str> {
        self.u32().map(f32::from_bits)
    }

    pub fn f64(&mut self) -> Result<f64, &'static str> {
        self.u64().map(f64::from_bits)
    }

    pub fn count(&mut self, stride: usize) -> Result<usize, &'static str> {
        let count = self.u32()? as usize;
        match count.checked_mul(stride.max(1)) {
            Some(length) if length <= self.remaining => Ok(count),
            _ => Err(FAULT_LENGTH),
        }
    }

    pub fn string(&mut self) -> Result<String, &'static str> {
        let length = self.count(1)?;
        if length > STRING_LIMIT {
            return Err(FAULT_LENGTH);
        }
        let mut bytes = vec![0u8; length];
        self.fill(&mut bytes)?;
        String::from_utf8(bytes).map_err(|_| FAULT_UTF8)
    }

    pub fn done(&mut self) -> Result<(), &'static str> {
        if self.remaining != 0 {
            return Err(FAULT_TRAILING);
        }
        let mut byte = [0u8; 1];
        match self.source.read(&mut byte) {
            Ok(0) => Ok(()),
            Ok(_) => Err(FAULT_TRAILING),
            Err(_) => Err(FAULT_TRUNCATED),
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }

    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), &'static str> {
        if bytes.len() > self.remaining {
            return Err(FAULT_TRUNCATED);
        }
        self.source.read_exact(bytes).map_err(|_| FAULT_TRUNCATED)?;
        self.remaining -= bytes.len();
        Ok(())
    }
}
