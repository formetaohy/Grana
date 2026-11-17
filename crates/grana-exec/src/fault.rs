use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Debug)]
pub struct Fault {
    detail: String,
}

impl Fault {
    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn from_payload(payload: Box<dyn Any + Send>) -> Self {
        let detail = match payload.downcast::<String>() {
            Ok(text) => *text,
            Err(payload) => match payload.downcast::<&'static str>() {
                Ok(text) => (*text).to_string(),
                Err(_) => "a panic carried no message".to_string(),
            },
        };
        Self { detail }
    }

    pub fn describe(&self, subject: &str) -> String {
        format!("{subject} failed after an internal fault: {}", self.detail)
    }
}

pub fn contain<R>(work: impl FnOnce() -> R) -> Result<R, Fault> {
    catch_unwind(AssertUnwindSafe(work)).map_err(Fault::from_payload)
}
