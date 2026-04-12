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
    }))
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

    let updated = json!({
        "llm_base_url": rc.llm_base_url,
        "llm_model": rc.llm_model,
        "auxiliary_model": rc.auxiliary_model,
        "compression_threshold": rc.compression_threshold,
        "context_length_override": rc.context_length_override,
    });

    (StatusCode::OK, Json(updated)).into_response()
}
