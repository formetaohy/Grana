use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the segmentation model cannot be read: {detail}")]
    Model { detail: String },
    #[error("the segmentation model cannot run: {detail}")]
    Inference { detail: String },
    #[error("the segmentation input is invalid: {detail}")]
    Input { detail: String },
    #[error("the segmentation device is unusable: {detail}")]
    Device { detail: String },
}

impl Error {
    pub(crate) fn model(detail: impl Into<String>) -> Self {
        Self::Model {
            detail: detail.into(),
        }
    }

    pub(crate) fn inference(detail: impl Into<String>) -> Self {
        Self::Inference {
            detail: detail.into(),
        }
    }

    pub(crate) fn input(detail: impl Into<String>) -> Self {
        Self::Input {
            detail: detail.into(),
        }
    }

    pub(crate) fn device(detail: impl Into<String>) -> Self {
        Self::Device {
            detail: detail.into(),
        }
    }
}
