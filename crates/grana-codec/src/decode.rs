use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    FAULT_CANCELLED, FAULT_LENGTH, FAULT_TAG, FAULT_TRAILING, FAULT_TRUNCATED, FAULT_UTF8, Fault,
    STRING_LIMIT,
};

enum Source<'a> {
    Slice(&'a [u8]),
    Stream(&'a mut dyn Read),
}

pub struct Decoder<'a> {
    source: Source<'a>,
    remaining: usize,
    cancel: Option<&'a AtomicBool>,
    report: Option<&'a mut dyn FnMut(usize)>,
    steps: usize,
}

impl<'a> Decoder<'a> {
    pub fn memory(bytes: &'a [u8]) -> Self {
        Self {
            source: Source::Slice(bytes),
            remaining: bytes.len(),
            cancel: None,
            report: None,
            steps: 0,
        }
    }

    pub fn stream(
        source: &'a mut dyn Read,
        length: usize,
        cancel: &'a AtomicBool,
        report: &'a mut dyn FnMut(usize),
    ) -> Self {
        Self {
            source: Source::Stream(source),
            remaining: length,
            cancel: Some(cancel),
            report: Some(report),
            steps: 0,
        }
    }

    pub fn step(&mut self) {
        self.steps += 1;
        if let Some(report) = self.report.as_deref_mut() {
            report(self.steps);
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }

    pub fn done(&mut self) -> Result<(), Fault> {
        if self.remaining != 0 {
            return Err(FAULT_TRAILING);
        }
        match &mut self.source {
            Source::Slice(_) => Ok(()),
            Source::Stream(source) => {
                let mut byte = [0u8; 1];
                match source.read(&mut byte) {
                    Ok(0) => Ok(()),
                    Ok(_) => Err(FAULT_TRAILING),
                    Err(_) => Err(FAULT_TRUNCATED),
                }
            }
        }
    }

    pub fn u8(&mut self) -> Result<u8, Fault> {
        let mut bytes = [0u8; 1];
        self.fill(&mut bytes)?;
        Ok(bytes[0])
    }

    pub fn bool(&mut self) -> Result<bool, Fault> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(FAULT_TAG),
        }
    }

    pub fn u16(&mut self) -> Result<u16, Fault> {
        let mut bytes = [0u8; 2];
        self.fill(&mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    pub fn u32(&mut self) -> Result<u32, Fault> {
        let mut bytes = [0u8; 4];
        self.fill(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn i32(&mut self) -> Result<i32, Fault> {
        self.u32().map(|value| value as i32)
    }

    pub fn u64(&mut self) -> Result<u64, Fault> {
        let mut bytes = [0u8; 8];
        self.fill(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn f32(&mut self) -> Result<f32, Fault> {
        self.u32().map(f32::from_bits)
    }

    pub fn f64(&mut self) -> Result<f64, Fault> {
        self.u64().map(f64::from_bits)
    }

    pub fn count(&mut self, stride: usize) -> Result<usize, Fault> {
        let count = self.u32()? as usize;
        match count.checked_mul(stride.max(1)) {
            Some(length) if length <= self.remaining => Ok(count),
            _ => Err(FAULT_LENGTH),
        }
    }

    pub fn string(&mut self) -> Result<String, Fault> {
        let length = self.count(1)?;
        if length > STRING_LIMIT {
            return Err(FAULT_LENGTH);
        }
        let mut bytes = vec![0u8; length];
        self.fill(&mut bytes)?;
        String::from_utf8(bytes).map_err(|_| FAULT_UTF8)
    }

    pub fn blob(&mut self) -> Result<Vec<u8>, Fault> {
        let length = self.count(1)?;
        let mut bytes = vec![0u8; length];
        self.fill(&mut bytes)?;
        Ok(bytes)
    }

    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), Fault> {
        if bytes.len() > self.remaining {
            return Err(FAULT_TRUNCATED);
        }
        if self
            .cancel
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
        {
            return Err(FAULT_CANCELLED);
        }
        match &mut self.source {
            Source::Slice(slice) => {
                let at = slice.len() - self.remaining;
                bytes.copy_from_slice(&slice[at..at + bytes.len()]);
            }
            Source::Stream(source) => source.read_exact(bytes).map_err(|_| FAULT_TRUNCATED)?,
        }
        self.remaining -= bytes.len();
        Ok(())
    }
}
