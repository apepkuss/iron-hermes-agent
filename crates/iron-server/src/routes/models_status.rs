use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use tracing::error;

use crate::state::AppState;

/// GET `/api/models/status` — proxy to the upstream ironmlx `/v1/models/status` endpoint.
///
/// Reads the base URL from the live `runtime_config` (not the static `ServerConfig`)
/// so that runtime updates to `llm_base_url` are reflected immediately.
pub async fn models_status(State(state): State<Arc<AppState>>) -> Response {
    let base_url = {
        let rc = state.runtime_config.read().await;
        rc.llm_base_url.clone()
    };

    // Strip trailing slash, then strip a trailing "/v1" if present, so we can
    // reconstruct the canonical `{base}/v1/models/status` URL.
    let base = base_url.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    let url = format!("{base}/v1/models/status");

    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp.json().await.unwrap_or(json!({"models": []}));
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(resp) => {
            let status = resp.status();
            error!("Upstream /v1/models/status returned HTTP {status}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Upstream returned {status}")})),
            )
                .into_response()
        }
        Err(e) => {
            error!("Failed to reach upstream /v1/models/status: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error": format!("Cannot reach LLM provider: {e}")})),
            )
                .into_response()
        }
    }
}
