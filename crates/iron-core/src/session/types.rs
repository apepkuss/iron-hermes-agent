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

impl SessionMessage {
    /// Convert an LLM Message to a SessionMessage for persistence.
    pub fn from_llm_message(msg: &crate::llm::types::Message, session_id: &str) -> Self {
        Self {
            id: 0,
            session_id: session_id.to_string(),
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_call_id: msg.tool_call_id.clone(),
            tool_calls: msg
                .tool_calls
                .as_ref()
                .map(|tc| serde_json::to_string(tc).unwrap_or_default()),
            tool_name: msg.name.clone(),
            timestamp: super::store::chrono_now(),
            finish_reason: None,
        }
    }
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

/// FTS5 search match record.
#[derive(Debug, Clone)]
pub struct MessageMatch {
    pub session_id: String,
    /// Row id of the matched message (stable per database, used to anchor
    /// the browser UI when jumping back to a search hit).
    pub message_id: i64,
    pub content: String,
    pub role: String,
    pub rank: f64,
}

/// Token usage for a session or a single response.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}
