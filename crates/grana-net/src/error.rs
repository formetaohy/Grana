use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the location `{location}` is not an http or https url")]
    Scheme { location: String },
    #[error("the url `{url}` is malformed: {detail}")]
    Url { url: String, detail: String },
    #[error("the request to `{url}` failed: {detail}")]
    Transport { url: String, detail: String },
    #[error("the host answered the status {status} for `{url}`")]
    Status { url: String, status: u16 },
    #[error("the response of `{url}` exceeds the size limit")]
    Size { url: String },
}

impl From<Error> for String {
    fn from(error: Error) -> Self {
        error.to_string()
    }
}
