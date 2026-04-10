use thiserror::Error;

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Tool unavailable: {0}")]
    Unavailable(String),

    #[error("Invalid arguments for tool '{tool}': {reason}")]
    InvalidArgs { tool: String, reason: String },

    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Tool timeout after {0} seconds")]
    Timeout(u64),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
