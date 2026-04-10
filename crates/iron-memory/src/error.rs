use thiserror::Error;

#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid target: {0}. Must be 'memory' or 'user'.")]
    InvalidTarget(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
