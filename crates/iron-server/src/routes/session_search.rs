use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::json;

use iron_core::session::search::SearchParams;

use crate::state::AppState;

#[derive(Deserialize)]
pub struct SearchQuery {
    /// Search keywords (optional; omit to browse recent sessions).
    pub q: Option<String>,
    /// Filter by message role, comma-separated (optional).
    pub role: Option<String>,
    /// Max sessions to return (optional, default 3, max 5).
    pub limit: Option<u32>,
}

/// GET `/api/sessions/search`
pub async fn search_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let params = SearchParams {
        query: query.q,
        role_filter: query.role,
        limit: query.limit.unwrap_or(3).min(5),
        current_session_id: None,
    };

    match state.searcher.search(params).await {
        Ok(response) => Json(serde_json::to_value(response).unwrap_or_default()),
        Err(e) => Json(json!({
            "success": false,
            "error": e.to_string()
        })),
    }
}
