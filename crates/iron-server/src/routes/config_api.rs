use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::state::AppState;

/// GET `/api/config` — return the current runtime configuration.
pub async fn get_config(State(state): State<Arc<AppState>>) -> Json<Value> {
    let rc = state.runtime_config.read().await;
    Json(json!({
        "llm_base_url": rc.llm_base_url,
        "llm_model": rc.llm_model,
        "auxiliary_model": rc.auxiliary_model,
        "compression_threshold": rc.compression_threshold,
        "context_length_override": rc.context_length_override,
        "disabled_toolsets": rc.disabled_toolsets,
    }))
}

/// GET `/api/toolsets` — list all registered toolsets with tool counts.
pub async fn list_toolsets(State(state): State<Arc<AppState>>) -> Json<Value> {
    let toolsets = state.tool_registry.toolsets();
    let rc = state.runtime_config.read().await;
    let disabled: &[String] = &rc.disabled_toolsets;

    let list: Vec<Value> = toolsets
        .iter()
        .map(|(name, count)| {
            json!({
                "name": name,
                "tool_count": count,
                "enabled": !disabled.contains(name),
            })
        })
        .collect();

    Json(json!({ "toolsets": list }))
}

/// POST `/api/config` — partially update the runtime configuration.
///
/// Only fields present in the JSON payload are updated.
/// `compression_threshold` is clamped to `[0.50, 0.95]`.
pub async fn update_config(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Response {
    let mut rc = state.runtime_config.write().await;

    if let Some(v) = payload.get("llm_base_url").and_then(|v| v.as_str()) {
        rc.llm_base_url = v.to_string();
    }

    if let Some(v) = payload.get("llm_model").and_then(|v| v.as_str()) {
        rc.llm_model = v.to_string();
    }

    if payload.get("auxiliary_model").is_some() {
        rc.auxiliary_model = payload["auxiliary_model"].as_str().map(|s| s.to_string());
    }

    if let Some(v) = payload
        .get("compression_threshold")
        .and_then(|v| v.as_f64())
    {
        rc.compression_threshold = v.clamp(0.50, 0.95);
    }

    if payload.get("context_length_override").is_some() {
        rc.context_length_override = payload["context_length_override"].as_u64();
    }

    if let Some(arr) = payload.get("disabled_toolsets").and_then(|v| v.as_array()) {
        rc.disabled_toolsets = arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    let updated = json!({
        "llm_base_url": rc.llm_base_url,
        "llm_model": rc.llm_model,
        "auxiliary_model": rc.auxiliary_model,
        "compression_threshold": rc.compression_threshold,
        "context_length_override": rc.context_length_override,
        "disabled_toolsets": rc.disabled_toolsets,
    });

    (StatusCode::OK, Json(updated)).into_response()
}
