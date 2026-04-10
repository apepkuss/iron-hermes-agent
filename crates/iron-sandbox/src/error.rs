use thiserror::Error;

#[derive(Error, Debug)]
pub enum SandboxError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Sandbox execution failed: {0}")]
    ExecutionFailed(String),
    #[error("Tool call limit exceeded")]
    ToolCallLimitExceeded,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
