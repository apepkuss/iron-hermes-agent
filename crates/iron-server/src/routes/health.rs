use axum::Json;
use serde_json::{Value, json};

/// GET `/health` — simple liveness probe.
pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}
