use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tracing::warn;

use crate::auxiliary_client::AuxiliaryClient;
use crate::error::CoreError;

use super::store::SessionStore;

/// Core search engine for session history.
pub struct SessionSearcher {
    store: Arc<std::sync::Mutex<SessionStore>>,
    auxiliary_client: Option<AuxiliaryClient>,
}

/// Search parameters.
pub struct SearchParams {
    /// FTS5 query string. If empty/None, browse recent sessions instead.
    pub query: Option<String>,
    /// Filter by message role (comma-separated): user, assistant, tool.
    pub role_filter: Option<String>,
    /// Max sessions to return (capped at 5).
    pub limit: u32,
    /// Exclude this session from results (typically the current session).
    pub current_session_id: Option<String>,
}

/// A single search result with LLM summary or text preview.
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub session_id: String,
    pub title: Option<String>,
    pub model: String,
    pub started_at: String,
    pub message_count: u32,
    pub summary: String,
}

/// A recent session entry (browse mode).
#[derive(Debug, Clone, Serialize)]
pub struct RecentResult {
    pub session_id: String,
    pub title: Option<String>,
    pub model: String,
    pub started_at: String,
    pub message_count: u32,
    pub preview: String,
}

/// Search response — either keyword search results or recent session list.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "mode")]
pub enum SearchResponse {
    #[serde(rename = "search")]
    Search {
        query: String,
        results: Vec<SearchResult>,
        count: usize,
    },
    #[serde(rename = "recent")]
    Recent {
        results: Vec<RecentResult>,
        count: usize,
    },
}

/// Max characters of conversation text to send to the auxiliary LLM.
const MAX_CONVERSATION_CHARS: usize = 100_000;
/// Max characters for the fallback preview (no auxiliary LLM).
const MAX_PREVIEW_CHARS: usize = 500;
/// Max messages to fetch from FTS5 for session deduplication.
const FTS_MATCH_LIMIT: u32 = 50;

impl SessionSearcher {
    pub fn new(
        store: Arc<std::sync::Mutex<SessionStore>>,
        auxiliary_client: Option<AuxiliaryClient>,
    ) -> Self {
        Self {
            store,
            auxiliary_client,
        }
    }

    /// Main search entry point.
    pub async fn search(&self, params: SearchParams) -> Result<SearchResponse, CoreError> {
        match &params.query {
            Some(q) if !q.trim().is_empty() => self.keyword_search(params).await,
            _ => self.browse_recent(params).await,
        }
    }

    /// Keyword search: FTS5 query → session dedup → summarize.
    async fn keyword_search(&self, params: SearchParams) -> Result<SearchResponse, CoreError> {
        let query = params.query.as_deref().unwrap_or("");
        let limit = params.limit.min(5);

        // 1. FTS5 query — get matching messages.
        let matches = {
            let store = self
                .store
                .lock()
                .map_err(|e| CoreError::Session(format!("Store lock failed: {e}")))?;
            store.search_messages(
                query,
                params.current_session_id.as_deref(),
                params.role_filter.as_deref(),
                FTS_MATCH_LIMIT,
            )?
        };

        // 2. Deduplicate by session_id, preserving order (best rank first).
        let mut seen = HashMap::new();
        let mut top_sessions = Vec::new();
        for m in &matches {
            if !seen.contains_key(&m.session_id) {
                seen.insert(m.session_id.clone(), m.rank);
                top_sessions.push(m.session_id.clone());
                if top_sessions.len() >= limit as usize {
                    break;
                }
            }
        }

        // 3. For each session, load conversation and generate summary.
        let mut results = Vec::new();
        for sid in &top_sessions {
            let (session_meta, conversation_text) = {
                let store = self
                    .store
                    .lock()
                    .map_err(|e| CoreError::Session(format!("Store lock failed: {e}")))?;

                let session = store.get_session(sid)?;
                let messages = store.get_messages(sid)?;

                let text = messages
                    .iter()
                    .filter_map(|m| m.content.as_ref().map(|c| format!("[{}]: {}", m.role, c)))
                    .collect::<Vec<_>>()
                    .join("\n");

                (session, text)
            };

            let meta = session_meta.unwrap_or_else(|| super::types::Session {
                id: sid.clone(),
                model: String::new(),
                system_prompt: None,
                parent_session_id: None,
                started_at: String::new(),
                ended_at: None,
                end_reason: None,
                message_count: 0,
                tool_call_count: 0,
                title: None,
            });

            let summary = self.summarize_session(&conversation_text, query).await;

            results.push(SearchResult {
                session_id: sid.to_string(),
                title: meta.title,
                model: meta.model,
                started_at: meta.started_at,
                message_count: meta.message_count,
                summary,
            });
        }

        let count = results.len();
        Ok(SearchResponse::Search {
            query: query.to_string(),
            results,
            count,
        })
    }

    /// Browse mode: return recent sessions with a message preview.
    async fn browse_recent(&self, params: SearchParams) -> Result<SearchResponse, CoreError> {
        let limit = params.limit.min(5);

        let store = self
            .store
            .lock()
            .map_err(|e| CoreError::Session(format!("Store lock failed: {e}")))?;

        let sessions = store.list_sessions(limit, 0)?;

        let mut results = Vec::new();
        for session in &sessions {
            // Get first user message as preview.
            let messages = store.get_messages(&session.id)?;
            let preview = messages
                .iter()
                .find(|m| m.role == "user")
                .and_then(|m| m.content.as_ref())
                .map(|c| truncate_str(c, MAX_PREVIEW_CHARS))
                .unwrap_or_default();

            results.push(RecentResult {
                session_id: session.id.clone(),
                title: session.title.clone(),
                model: session.model.clone(),
                started_at: session.started_at.clone(),
                message_count: session.message_count,
                preview,
            });
        }

        let count = results.len();
        Ok(SearchResponse::Recent { results, count })
    }

    /// Generate a summary for a session using the auxiliary LLM.
    /// Falls back to a text preview if no auxiliary client is available.
    async fn summarize_session(&self, conversation_text: &str, query: &str) -> String {
        if conversation_text.is_empty() {
            return String::new();
        }

        let truncated = truncate_str(conversation_text, MAX_CONVERSATION_CHARS);

        let Some(client) = &self.auxiliary_client else {
            return truncate_str(conversation_text, MAX_PREVIEW_CHARS);
        };

        let prompt = format!(
            "Summarize the following conversation focused on the search topic: \"{query}\".\n\n\
             Keep the summary under 200 words. Focus on:\n\
             - What was discussed related to the search topic\n\
             - Key decisions or solutions found\n\
             - Relevant files or tools mentioned\n\n\
             Conversation:\n{truncated}"
        );

        match client.generate_summary(&prompt, 1000, None).await {
            Ok(summary) => summary,
            Err(e) => {
                warn!("Auxiliary LLM summarization failed: {e}");
                truncate_str(conversation_text, MAX_PREVIEW_CHARS)
            }
        }
    }
}

/// Truncate a string to at most `max_chars` characters, appending "..." if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    }
}
