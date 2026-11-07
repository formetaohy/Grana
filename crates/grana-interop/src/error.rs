#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the track file cannot be read: {detail}")]
    Read { detail: String },
    #[error("the track file cannot be written: {detail}")]
    Write { detail: String },
    #[error("the track file does not describe a lineage: {detail}")]
    Format { detail: String },
    #[error("the lineage graph cannot be stored: {detail}")]
    Graph { detail: String },
}
