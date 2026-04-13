use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
    #[error("LLM API error: {status} - {message}")]
    LlmApi { status: u16, message: String },

    #[error("LLM request failed: {0}")]
    LlmRequest(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Agent is busy processing another request for this session")]
    AgentBusy,

    #[error("Agent execution timed out after {0} seconds")]
    Timeout(u64),

    #[error(transparent)]
    Tool(#[from] iron_tools::error::ToolError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
