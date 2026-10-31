const FAULT_TRUNCATED: &str = "the project data ends before its structure closes";
const FAULT_LENGTH: &str = "the project data declares a length it cannot hold";
const FAULT_TAG: &str = "the project data carries an unknown variant tag";
const FAULT_UTF8: &str = "the project data carries text that is not utf-8";
const FAULT_TRAILING: &str = "the project data carries trailing bytes";

pub(crate) trait Encode {
    fn encode(&self, writer: &mut Writer);
}

pub(crate) trait Decode: Sized {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, &'static str>;
}

impl Encode for bool {
    fn encode(&self, writer: &mut Writer) {
        writer.bool(*self);
    }
}

impl Encode for u8 {
    fn encode(&self, writer: &mut Writer) {
        writer.u8(*self);
    }
}

impl Encode for u32 {
    fn encode(&self, writer: &mut Writer) {
        writer.u32(*self);
    }
}

impl Encode for u64 {
    fn encode(&self, writer: &mut Writer) {
        writer.u64(*self);
    }
}

impl Encode for f32 {
    fn encode(&self, writer: &mut Writer) {
        writer.f32(*self);
    }
}

impl Encode for f64 {
    fn encode(&self, writer: &mut Writer) {
        writer.f64(*self);
    }
}

impl Encode for String {
    fn encode(&self, writer: &mut Writer) {
        writer.string(self);
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

pub(crate) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
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
        self.bytes.extend_from_slice(value.as_bytes());
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn u8(&mut self) -> Result<u8, &'static str> {
        self.take(1).map(|bytes| bytes[0])
    }

    pub fn bool(&mut self) -> Result<bool, &'static str> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(FAULT_TAG),
        }
    }

    pub fn u32(&mut self) -> Result<u32, &'static str> {
        self.take(4).map(|bytes| {
            let mut array = [0; 4];
            array.copy_from_slice(bytes);
            u32::from_le_bytes(array)
        })
    }

    pub fn u64(&mut self) -> Result<u64, &'static str> {
        self.take(8).map(|bytes| {
            let mut array = [0; 8];
            array.copy_from_slice(bytes);
            u64::from_le_bytes(array)
        })
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
            Some(length) if length <= self.bytes.len() - self.at => Ok(count),
            _ => Err(FAULT_LENGTH),
        }
    }

    pub fn string(&mut self) -> Result<String, &'static str> {
        let length = self.count(1)?;
        let bytes = self.take(length)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| FAULT_UTF8)
    }

    pub fn done(&self) -> Result<(), &'static str> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(FAULT_TRAILING)
        }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], &'static str> {
        let end = match self.at.checked_add(length) {
            Some(end) if end <= self.bytes.len() => end,
            _ => return Err(FAULT_TRUNCATED),
        };
        let bytes = &self.bytes[self.at..end];
        self.at = end;
        Ok(bytes)
    }
}
