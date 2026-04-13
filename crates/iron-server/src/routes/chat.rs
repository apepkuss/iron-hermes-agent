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

use iron_core::agent::{Agent, AgentConfig};
use iron_core::context_compressor::{AuxiliaryLlmConfig, CompressorConfig};
use iron_core::event::{AgentEvent, EventCallback};
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

/// Build a [`CompressorConfig`] from the current [`RuntimeConfig`].
///
/// Returns `None` when `compression_threshold` is zero (compression disabled).
fn build_compressor_config(rc: &crate::config::RuntimeConfig) -> Option<CompressorConfig> {
    if rc.compression_threshold <= 0.0 {
        return None;
    }
    let context_length = rc.context_length_override.unwrap_or(128_000);
    Some(CompressorConfig {
        context_length,
        threshold: rc.compression_threshold,
        target_ratio: 0.20,
        protect_first_n: 3,
        auxiliary_llm: rc.auxiliary_model.as_ref().map(|m| AuxiliaryLlmConfig {
            base_url: rc.llm_base_url.clone(),
            model: m.clone(),
        }),
    })
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

    let compressor_config = {
        let rc = state.runtime_config.read().await;
        build_compressor_config(&rc)
    };

    let agent_config = AgentConfig {
        model_name: state.config.model_name.clone(),
        compressor_config,
        ..AgentConfig::default()
    };

    let mem = Arc::clone(&state.memory_manager);
    let sm = Arc::clone(&state.skill_manager);

    let mut agent = Agent::new(
        llm_client,
        Arc::clone(&state.tool_registry),
        mem,
        sm,
        agent_config,
    );

    agent.set_session_id(Uuid::new_v4().to_string());

    // Pre-load conversation history (excluding the last user message which agent.chat will add).
    let core_messages = convert_messages(&payload.messages);
    let history = core_messages[..core_messages.len().saturating_sub(1)].to_vec();
    agent.load_history(history);

    match agent.chat(user_msg, None).await {
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

    // Channel for streaming events from the agent callback to the SSE response.
    let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);

    let model_name_clone = model_name.clone();

    let compressor_config = {
        let rc = state.runtime_config.read().await;
        build_compressor_config(&rc)
    };

    // Spawn the agent loop in a background task.
    tokio::spawn(async move {
        let llm_client = make_llm_client(&state, model_override.as_deref());

        let mem = Arc::clone(&state.memory_manager);
        let sm = Arc::clone(&state.skill_manager);

        let agent_config = AgentConfig {
            model_name: model_name_clone,
            compressor_config,
            ..AgentConfig::default()
        };

        let mut agent = Agent::new(
            llm_client,
            Arc::clone(&state.tool_registry),
            mem,
            sm,
            agent_config,
        );

        agent.set_session_id(Uuid::new_v4().to_string());

        let core_messages = convert_messages(&payload.messages);
        let history = core_messages[..core_messages.len().saturating_sub(1)].to_vec();
        agent.load_history(history);

        let tx_cb = tx.clone();
        let callback: EventCallback = Box::new(move |event: AgentEvent| {
            let _ = tx_cb.try_send(event);
        });

        let result = agent.chat(user_msg, Some(callback)).await;

        if let Err(e) = result {
            error!("Streaming agent error: {e}");
        }

        drop(tx);
    });

    // Build the SSE stream from the channel.
    let sse_stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            match event {
                AgentEvent::TextDelta { content } => {
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
                            "delta": { "content": content },
                            "finish_reason": Value::Null,
                        }]
                    });
                    yield Ok::<_, std::convert::Infallible>(
                        Event::default().data(chunk.to_string())
                    );
                }
                AgentEvent::ToolStarted { tool, args_preview, call_id } => {
                    let data = json!({
                        "tool": tool,
                        "args_preview": args_preview,
                        "call_id": call_id,
                    });
                    yield Ok(Event::default().event("tool_started").data(data.to_string()));
                }
                AgentEvent::ToolCompleted { tool, call_id, duration_ms, success, result_preview } => {
                    let data = json!({
                        "tool": tool,
                        "call_id": call_id,
                        "duration_ms": duration_ms,
                        "success": success,
                        "result_preview": result_preview,
                    });
                    yield Ok(Event::default().event("tool_completed").data(data.to_string()));
                }
                AgentEvent::TodoUpdate { todos } => {
                    let data = json!({"todos": todos});
                    yield Ok(Event::default().event("todo_update").data(data.to_string()));
                }
            }
        }

        // Send the final [DONE] sentinel.
        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(sse_stream).into_response()
}
