use std::sync::Arc;

use serde_json::json;

use iron_tool_api::registry::ToolRegistry;
use iron_tool_api::types::{ToolResult, ToolSchema};

use super::search::{SearchParams, SessionSearcher};

/// Register the `session_search` tool into the given [`ToolRegistry`].
pub fn register_session_search(registry: &mut ToolRegistry, searcher: Arc<SessionSearcher>) {
    let schema = ToolSchema {
        name: "session_search".to_string(),
        description: "Search past conversation sessions by keywords, or browse recent sessions. \
             Use when the user references past discussions, asks about previous work, \
             or when cross-session context would help. Supports Chinese and English."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keywords or phrases. Omit to browse recent sessions."
                },
                "role_filter": {
                    "type": "string",
                    "description": "Filter by message role (comma-separated): user, assistant, tool."
                },
                "limit": {
                    "type": "integer",
                    "description": "Max sessions to return (1-5, default: 3)."
                }
            }
        }),
    };

    registry.register_sync(
        "session_search",
        "session_search",
        schema,
        move |args, ctx| {
            let query = args["query"].as_str().map(|s| s.to_string());
            let role_filter = args["role_filter"].as_str().map(|s| s.to_string());
            let limit = args["limit"].as_u64().unwrap_or(3).min(5) as u32;

            let params = SearchParams {
                query,
                role_filter,
                limit,
                current_session_id: Some(ctx.task_id.clone()),
            };

            let searcher = Arc::clone(&searcher);
            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(searcher.search(params))
            })
            .map_err(|e| iron_tool_api::error::ToolError::ExecutionFailed(e.to_string()))?;

            Ok(ToolResult::ok(
                serde_json::to_value(result).unwrap_or_default(),
            ))
        },
    );
}
