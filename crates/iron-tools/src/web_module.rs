//! WebTools — ToolModule implementation for web tools.

use std::sync::Arc;

use serde_json::{Value, json};
use tracing::info;

use crate::{
    ToolModule, ToolRegistry, ToolResult, ToolSchema, error::ToolError, web::TavilyClient,
};

/// A `ToolModule` that registers `web_search` and `web_extract` tools.
///
/// If `client` is `None` (e.g. `TAVILY_API_KEY` not set), registration is
/// skipped entirely.
pub struct WebTools {
    client: Option<Arc<TavilyClient>>,
}

impl WebTools {
    /// Construct from the `TAVILY_API_KEY` environment variable.
    /// Returns a `WebTools` with `client = None` when the variable is absent.
    pub fn from_env() -> Self {
        match TavilyClient::from_env() {
            Ok(c) => Self {
                client: Some(Arc::new(c)),
            },
            Err(_) => Self { client: None },
        }
    }
}

impl ToolModule for WebTools {
    fn register(self: Box<Self>, registry: &mut ToolRegistry) {
        let client = match self.client {
            Some(c) => c,
            None => {
                info!("TAVILY_API_KEY not set — skipping web tool registration");
                return;
            }
        };

        // web_search
        {
            let client = Arc::clone(&client);
            registry.register_sync(
                "web_search",
                "web",
                ToolSchema {
                    name: "web_search".to_string(),
                    description: "Search the web using Tavily and return relevant results."
                        .to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "The search query."
                            }
                        },
                        "required": ["query"]
                    }),
                },
                move |args: Value, _ctx| {
                    let query = args["query"]
                        .as_str()
                        .ok_or_else(|| ToolError::InvalidArgs {
                            tool: "web_search".to_string(),
                            reason: "missing required field: query".to_string(),
                        })?
                        .to_string();

                    let client = Arc::clone(&client);

                    let results = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(client.search(&query))
                    })?;

                    Ok(ToolResult::ok(crate::web::format_search_results(&results)))
                },
            );
        }

        // web_extract
        {
            let client = Arc::clone(&client);
            registry.register_sync(
                "web_extract",
                "web",
                ToolSchema {
                    name: "web_extract".to_string(),
                    description: "Extract content from up to 5 URLs using Tavily.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "urls": {
                                "type": "array",
                                "items": { "type": "string" },
                                "maxItems": 5,
                                "description": "List of URLs to extract content from (max 5)."
                            }
                        },
                        "required": ["urls"]
                    }),
                },
                move |args: Value, _ctx| {
                    let urls_val =
                        args["urls"]
                            .as_array()
                            .ok_or_else(|| ToolError::InvalidArgs {
                                tool: "web_extract".to_string(),
                                reason: "missing required field: urls (must be an array)"
                                    .to_string(),
                            })?;

                    let urls: Vec<String> = urls_val
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();

                    let client = Arc::clone(&client);

                    let items = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(client.extract(&urls))
                    })?;

                    let results: Vec<Value> = items
                        .into_iter()
                        .map(|item| {
                            json!({
                                "url": item.url,
                                "content": item.content.or(item.raw_content).unwrap_or_default(),
                                "error": item.error,
                            })
                        })
                        .collect();

                    Ok(ToolResult::ok(json!({ "results": results })))
                },
            );
        }
    }
}
