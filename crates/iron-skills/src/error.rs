use thiserror::Error;

#[derive(Error, Debug)]
pub enum SkillError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Skill not found: {0}")]
    NotFound(String),
    #[error("Invalid skill name: {0}")]
    InvalidName(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
