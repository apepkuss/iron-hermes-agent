/// Represents a conversation session.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub parent_session_id: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
    pub message_count: u32,
    pub tool_call_count: u32,
    pub title: Option<String>,
}

/// Represents a single message within a session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionMessage {
    /// Auto-incremented primary key (0 before insertion).
    pub id: i64,
    pub session_id: String,
    /// One of "system", "user", "assistant", "tool".
    pub role: String,
    pub content: Option<String>,
    pub tool_call_id: Option<String>,
    /// JSON string of tool calls.
    pub tool_calls: Option<String>,
    pub tool_name: Option<String>,
    pub timestamp: String,
    pub finish_reason: Option<String>,
}

/// Token usage for a session or a single response.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
