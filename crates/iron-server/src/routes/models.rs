use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use tracing::error;

use crate::state::AppState;

/// GET `/v1/models` — return an OpenAI-compatible model list.
pub async fn list_models(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "object": "list",
        "data": [
            {
                "id": state.config.model_name,
                "object": "model",
                "created": 1700000000_u64,
                "owned_by": "iron-hermes"
            }
        ]
    }))
}

/// GET `/v1/provider/models` — proxy to the upstream LLM provider's model list.
pub async fn list_provider_models(State(state): State<Arc<AppState>>) -> Response {
    let base_url = state.config.llm_base_url.trim_end_matches('/');
    let url = if base_url.ends_with("/v1") {
        format!("{base_url}/models")
    } else {
        format!("{base_url}/v1/models")
    };

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp
                .json()
                .await
                .unwrap_or(json!({"object": "list", "data": []}));
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(resp) => {
            let status = resp.status();
            error!("Upstream /v1/models returned HTTP {status}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Upstream returned {status}")})),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to reach upstream /v1/models: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Cannot reach LLM provider: {e}")})),
            )
                .into_response()
        }
    }
}
