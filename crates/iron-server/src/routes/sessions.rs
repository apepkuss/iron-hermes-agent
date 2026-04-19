use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

const DEFAULT_PAGE_SIZE: u32 = 30;
const MAX_PAGE_SIZE: u32 = 100;
const TITLE_FALLBACK_CHARS: usize = 40;

#[derive(Deserialize)]
pub struct ListQuery {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Optional full-text query. When set, the endpoint switches to search
    /// mode: it runs SQLite FTS over message contents and returns one entry
    /// per matched message (not per session), ranked by BM25, so the UI can
    /// render a VS Code-style flat list and jump straight to the matched
    /// message. `offset` is ignored in search mode.
    pub q: Option<String>,
}

/// Build a display title: stored `title` if set, otherwise the first user
/// message trimmed to `TITLE_FALLBACK_CHARS` characters. Falls back to an
/// empty string when no user message exists.
fn build_display_title(
    store: &iron_core::session::store::SessionStore,
    session_id: &str,
    stored_title: &Option<String>,
) -> String {
    if let Some(t) = stored_title.as_ref().filter(|s| !s.trim().is_empty()) {
        return t.clone();
    }
    match store.first_user_message(session_id) {
        Ok(Some(content)) => {
            let trimmed = content.trim();
            let chars: Vec<char> = trimmed.chars().collect();
            if chars.len() <= TITLE_FALLBACK_CHARS {
                trimmed.to_string()
            } else {
                let head: String = chars.iter().take(TITLE_FALLBACK_CHARS).collect();
                format!("{head}…")
            }
        }
        _ => String::new(),
    }
}

/// GET `/api/sessions?limit=&offset=&q=`
pub async fn list_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let offset = query.offset.unwrap_or(0);
    let search_query = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let store = match state.session_store.lock() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("session store poisoned: {e}")})),
            );
        }
    };

    // Search mode: FTS over message content. Every matching message becomes
    // its own entry so the UI can render a VS Code-style flat list and jump
    // directly to the matched message. Session display metadata is cached
    // per session_id to avoid redundant lookups.
    if let Some(q) = search_query {
        let matches = match store.search_messages(q, None, None, limit) {
            Ok(v) => v,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": e.to_string()})),
                );
            }
        };

        let mut session_meta: std::collections::HashMap<String, (String, String)> =
            std::collections::HashMap::new();
        let mut items = Vec::new();
        for m in matches.into_iter() {
            let entry = session_meta.entry(m.session_id.clone());
            let (title, started_at) = match entry {
                std::collections::hash_map::Entry::Occupied(o) => o.get().clone(),
                std::collections::hash_map::Entry::Vacant(v) => {
                    match store.get_session(&m.session_id) {
                        Ok(Some(s)) => {
                            let display_title = build_display_title(&store, &s.id, &s.title);
                            v.insert((display_title.clone(), s.started_at.clone()));
                            (display_title, s.started_at)
                        }
                        _ => continue,
                    }
                }
            };
            items.push(json!({
                "session_id": m.session_id,
                "session_title": title,
                "session_started_at": started_at,
                "message_id": m.message_id,
                "role": m.role,
                "content": m.content,
            }));
        }

        return (
            StatusCode::OK,
            Json(json!({
                "matches": items,
                "has_more": false,
                "limit": limit,
                "offset": 0,
                "mode": "search",
                "query": q,
            })),
        );
    }

    // Browse mode.
    let fetched = match store.list_non_empty_sessions(limit + 1, offset) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            );
        }
    };

    let has_more = fetched.len() as u32 > limit;
    let items: Vec<_> = fetched
        .into_iter()
        .take(limit as usize)
        .map(|s| {
            let display_title = build_display_title(&store, &s.id, &s.title);
            json!({
                "id": s.id,
                "title": s.title,
                "display_title": display_title,
                "started_at": s.started_at,
                "ended_at": s.ended_at,
                "message_count": s.message_count,
            })
        })
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "items": items,
            "has_more": has_more,
            "limit": limit,
            "offset": offset,
            "mode": "browse",
        })),
    )
}

/// GET `/api/sessions/{id}/messages`
pub async fn get_session_messages(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = match state.session_store.lock() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("session store poisoned: {e}")})),
            );
        }
    };

    // Check existence first so we can return 404 for unknown sessions.
    match store.get_session(&id) {
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "session not found"})),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            );
        }
        Ok(Some(_)) => {}
    }

    match store.get_messages(&id) {
        Ok(messages) => {
            let items: Vec<_> = messages
                .into_iter()
                .map(|m| {
                    json!({
                        "id": m.id,
                        "role": m.role,
                        "content": m.content,
                        "tool_call_id": m.tool_call_id,
                        "tool_calls": m.tool_calls,
                        "tool_name": m.tool_name,
                        "timestamp": m.timestamp,
                        "finish_reason": m.finish_reason,
                    })
                })
                .collect();
            (StatusCode::OK, Json(json!({"messages": items})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

#[derive(Deserialize)]
pub struct UpdateSessionBody {
    pub title: Option<String>,
}

/// PATCH `/api/sessions/{id}` — update mutable session fields (currently
/// just `title`). Passing `title: null` clears the stored title and the
/// display title will fall back to the first user message.
pub async fn update_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateSessionBody>,
) -> impl IntoResponse {
    let store = match state.session_store.lock() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("session store poisoned: {e}")})),
            );
        }
    };

    let normalized = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    match store.update_session_title(&id, normalized) {
        Ok(()) => {
            let display_title = build_display_title(&store, &id, &normalized.map(String::from));
            (
                StatusCode::OK,
                Json(json!({
                    "ok": true,
                    "title": normalized,
                    "display_title": display_title,
                })),
            )
        }
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({"error": msg})))
        }
    }
}

/// DELETE `/api/sessions/{id}`
pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = match state.session_store.lock() {
        Ok(g) => g,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("session store poisoned: {e}")})),
            );
        }
    };

    match store.delete_session(&id) {
        Ok(()) => (StatusCode::OK, Json(json!({"ok": true}))),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({"error": msg})))
        }
    }
}
