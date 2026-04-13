use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use serde_json::json;

use crate::state::AppState;

/// POST `/api/session/reset`
///
/// Resets the session identified by the `X-Platform`, `X-Chat-Id`, `X-User-Id`
/// and (optionally) `X-Thread-Id` request headers.  The next request with the
/// same headers will start a fresh session.
pub async fn reset_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let source = super::chat::extract_session_source(&headers);
    state.runtime.reset_session(&source).await;
    (StatusCode::OK, Json(json!({"status": "reset"})))
}
