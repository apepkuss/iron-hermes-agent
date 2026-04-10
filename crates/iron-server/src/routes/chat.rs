use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::error;
use uuid::Uuid;

use iron_core::agent::{Agent, AgentConfig, SessionState, StreamCallback};
use iron_core::llm::client::{LlmClient, LlmConfig};
use iron_core::llm::types::Message;

use crate::state::AppState;

/// Incoming OpenAI-compatible chat completion request.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<String>,
}

/// POST `/v1/chat/completions`
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Response {
    if payload.messages.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"message": "messages is required and must not be empty", "type": "invalid_request_error"}})),
        )
            .into_response();
    }

    let is_stream = payload.stream.unwrap_or(false);

    if is_stream {
        handle_streaming(state, payload).await
    } else {
        handle_non_streaming(state, payload).await
    }
}

/// Build an `LlmClient` from the server config, optionally overriding the model.
fn make_llm_client(state: &AppState, model_override: Option<&str>) -> LlmClient {
    LlmClient::new(LlmConfig {
        base_url: state.config.llm_base_url.clone(),
        api_key: state.config.llm_api_key.clone(),
        model: model_override
            .unwrap_or(&state.config.llm_model)
            .to_string(),
        temperature: None,
        max_tokens: None,
    })
}

/// Convert incoming messages to `iron_core::llm::types::Message`.
fn convert_messages(msgs: &[ChatMessage]) -> Vec<Message> {
    msgs.iter()
        .map(|m| Message {
            role: m.role.clone(),
            content: m.content.clone(),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        })
        .collect()
}

/// Extract the last user message text from the request.
fn last_user_message(msgs: &[ChatMessage]) -> Option<String> {
    msgs.iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.clone())
}

/// Non-streaming handler.
async fn handle_non_streaming(state: Arc<AppState>, payload: ChatRequest) -> Response {
    let user_msg = match last_user_message(&payload.messages) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "no user message found", "type": "invalid_request_error"}})),
            )
                .into_response();
        }
    };

    let llm_client = make_llm_client(&state, payload.model.as_deref());
    let memory_manager = state.memory_manager.lock().await;
    let agent_config = AgentConfig {
        model_name: state.config.model_name.clone(),
        ..AgentConfig::default()
    };

    // Agent requires owned MemoryManager and SkillManager — clone/recreate them.
    // For Phase 1, create lightweight copies.
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let base = home.join(".iron-hermes");
    let mut mem = iron_memory::manager::MemoryManager::new(base.join("memories"), None, None);
    mem.initialize().ok();
    drop(memory_manager);

    let skill_dirs: Vec<std::path::PathBuf> = vec![base.join("skills")];
    let sm = iron_skills::manager::SkillManager::new(skill_dirs, std::collections::HashSet::new());

    let mut agent = Agent::new(
        llm_client,
        Arc::clone(&state.tool_registry),
        mem,
        sm,
        agent_config,
    );

    let mut session = SessionState::new(Uuid::new_v4().to_string());

    // Pre-load conversation history (excluding the last user message which agent.chat will add).
    let core_messages = convert_messages(&payload.messages);
    for msg in &core_messages[..core_messages.len().saturating_sub(1)] {
        session.messages.push(msg.clone());
    }

    match agent.chat(&mut session, user_msg, None).await {
        Ok(response) => {
            let resp_id = format!("chatcmpl-{}", Uuid::new_v4());
            let reply = json!({
                "id": resp_id,
                "object": "chat.completion",
                "created": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                "model": state.config.model_name,
                "choices": [{
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": response.content,
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": response.usage.prompt_tokens,
                    "completion_tokens": response.usage.completion_tokens,
                    "total_tokens": response.usage.total_tokens,
                }
            });
            (StatusCode::OK, Json(reply)).into_response()
        }
        Err(e) => {
            error!("Agent chat error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": e.to_string(), "type": "server_error"}})),
            )
                .into_response()
        }
    }
}

/// Streaming handler — returns an SSE stream.
async fn handle_streaming(state: Arc<AppState>, payload: ChatRequest) -> Response {
    let user_msg = match last_user_message(&payload.messages) {
        Some(m) => m,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": {"message": "no user message found", "type": "invalid_request_error"}})),
            )
                .into_response();
        }
    };

    let resp_id = format!("chatcmpl-{}", Uuid::new_v4());
    let model_name = state.config.model_name.clone();
    let model_override = payload.model.clone();

    // Channel for streaming deltas from the agent callback to the SSE response.
    let (tx, mut rx) = mpsc::channel::<String>(256);

    let model_name_clone = model_name.clone();

    // Spawn the agent loop in a background task.
    tokio::spawn(async move {
        let llm_client = make_llm_client(&state, model_override.as_deref());

        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let base = home.join(".iron-hermes");
        let mut mem = iron_memory::manager::MemoryManager::new(base.join("memories"), None, None);
        mem.initialize().ok();

        let skill_dirs: Vec<std::path::PathBuf> = vec![base.join("skills")];
        let sm =
            iron_skills::manager::SkillManager::new(skill_dirs, std::collections::HashSet::new());

        let agent_config = AgentConfig {
            model_name: model_name_clone,
            ..AgentConfig::default()
        };

        let mut agent = Agent::new(
            llm_client,
            Arc::clone(&state.tool_registry),
            mem,
            sm,
            agent_config,
        );

        let mut session = SessionState::new(Uuid::new_v4().to_string());

        let core_messages = convert_messages(&payload.messages);
        for msg in &core_messages[..core_messages.len().saturating_sub(1)] {
            session.messages.push(msg.clone());
        }

        let tx_cb = tx.clone();
        let callback: StreamCallback = Box::new(move |delta: &str| {
            let _ = tx_cb.try_send(delta.to_string());
        });

        let result = agent.chat(&mut session, user_msg, Some(callback)).await;

        if let Err(e) = result {
            error!("Streaming agent error: {e}");
        }

        drop(tx);
    });

    // Build the SSE stream from the channel.
    let sse_stream = async_stream::stream! {
        while let Some(delta) = rx.recv().await {
            let chunk = json!({
                "id": resp_id,
                "object": "chat.completion.chunk",
                "created": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                "model": model_name,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": delta,
                    },
                    "finish_reason": Value::Null,
                }]
            });
            yield Ok::<_, std::convert::Infallible>(Event::default().data(chunk.to_string()));
        }

        // Send the final [DONE] sentinel.
        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(sse_stream).into_response()
}
