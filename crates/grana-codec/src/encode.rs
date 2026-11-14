use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::FAULT_CANCELLED;

enum Sink<'a> {
    Blob(&'a mut Vec<u8>),
    Stream(&'a mut dyn Write),
}

pub struct Encoder<'a> {
    sink: Sink<'a>,
    cancel: Option<&'a AtomicBool>,
    report: Option<&'a mut dyn FnMut(usize)>,
    steps: usize,
    written: usize,
    fault: Option<String>,
}

impl<'a> Encoder<'a> {
    pub fn memory(bytes: &'a mut Vec<u8>) -> Self {
        Self {
            sink: Sink::Blob(bytes),
            cancel: None,
            report: None,
            steps: 0,
            written: 0,
            fault: None,
        }
    }

    pub fn stream(
        sink: &'a mut dyn Write,
        cancel: &'a AtomicBool,
        report: &'a mut dyn FnMut(usize),
    ) -> Self {
        Self {
            sink: Sink::Stream(sink),
            cancel: Some(cancel),
            report: Some(report),
            steps: 0,
            written: 0,
            fault: None,
        }
    }

    pub fn len(&self) -> usize {
        self.written
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    pub fn fail(&mut self, detail: &str) {
        if self.fault.is_none() {
            self.fault = Some(detail.to_string());
        }
    }

    pub fn step(&mut self) {
        if self.fault.is_some() {
            return;
        }
        self.steps += 1;
        if let Some(report) = self.report.as_deref_mut() {
            report(self.steps);
        }
    }

    pub fn u8(&mut self, value: u8) {
        self.push(&[value]);
    }

    pub fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    pub fn u16(&mut self, value: u16) {
        self.push(&value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.push(&value.to_le_bytes());
    }

    pub fn i32(&mut self, value: i32) {
        self.push(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.push(&value.to_le_bytes());
    }

    pub fn f32(&mut self, value: f32) {
        self.push(&value.to_le_bytes());
    }

    pub fn f64(&mut self, value: f64) {
        self.push(&value.to_le_bytes());
    }

    pub fn count(&mut self, value: usize) {
        assert!(
            u32::try_from(value).is_ok(),
            "a binary array holds fewer than four billion entries"
        );
        self.u32(value as u32);
    }

    pub fn string(&mut self, value: &str) {
        self.count(value.len());
        self.push(value.as_bytes());
    }

    pub fn blob(&mut self, bytes: &[u8]) {
        self.count(bytes.len());
        self.push(bytes);
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        self.push(bytes);
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.fault.is_some() {
            return;
        }
        if self
            .cancel
            .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
        {
            self.fault = Some(FAULT_CANCELLED.to_string());
            return;
        }
        let written = match &mut self.sink {
            Sink::Blob(buffer) => {
                buffer.extend_from_slice(bytes);
                Ok(())
            }
            Sink::Stream(sink) => sink.write_all(bytes),
        };
        match written {
            Ok(()) => self.written += bytes.len(),
            Err(error) => self.fault = Some(error.to_string()),
        }
    }
}
