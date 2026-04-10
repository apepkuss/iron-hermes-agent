use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

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
