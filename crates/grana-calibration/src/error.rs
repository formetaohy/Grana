use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Image(#[from] grana_image::Error),
    #[error("the point spread function cannot be calibrated: {detail}")]
    Calibration { detail: String },
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}

pub(crate) fn calibration(detail: impl Into<String>) -> Error {
    Error::Calibration {
        detail: detail.into(),
    }
}
