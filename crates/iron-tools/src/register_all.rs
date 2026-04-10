//! register_all — Wires all implemented tools into a ToolRegistry.

use std::sync::Arc;

use serde_json::{Value, json};
use tracing::info;

use crate::{
    error::ToolError,
    file::{patch_file, read_file, search_files, write_file},
    registry::ToolRegistry,
    terminal::{TerminalParams, TerminalTool},
    types::{ToolResult, ToolSchema},
    web::TavilyClient,
};

/// Create a [`ToolRegistry`] pre-loaded with all built-in tools.
///
/// Web tools (web_search, web_extract) are only registered when the
/// `TAVILY_API_KEY` environment variable is present.
pub fn register_default_tools() -> ToolRegistry {
    let mut registry = ToolRegistry::new();

    register_terminal_tool(&mut registry);
    register_file_tools(&mut registry);
    register_web_tools(&mut registry);

    registry
}

// ── Terminal ──────────────────────────────────────────────────────────────────

fn register_terminal_tool(registry: &mut ToolRegistry) {
    let schema = ToolSchema {
        name: "terminal".to_string(),
        description: "Execute a shell command and return its output.".to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (optional)."
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command (optional)."
                }
            },
            "required": ["command"]
        }),
    };

    let tool = Arc::new(TerminalTool::new(30));

    registry.register_sync("terminal", "terminal", schema, move |args: Value, _ctx| {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "terminal".to_string(),
                reason: "missing required field: command".to_string(),
            })?
            .to_string();

        let timeout = args["timeout"].as_u64();

        let workdir = args["workdir"].as_str().map(std::path::PathBuf::from);

        let params = TerminalParams {
            command,
            background: false,
            timeout,
            workdir,
        };

        let tool = Arc::clone(&tool);

        let result = match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(tool.execute(params)),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to create tokio runtime: {e}"))
                })?;
                rt.block_on(tool.execute(params))
            }
        };

        let res = result?;

        Ok(ToolResult::ok(json!({
            "stdout": res.stdout,
            "stderr": res.stderr,
            "exit_code": res.exit_code,
            "truncated": res.truncated,
        })))
    });
}

// ── File tools ────────────────────────────────────────────────────────────────

fn register_file_tools(registry: &mut ToolRegistry) {
    // read_file
    registry.register_sync(
        "read_file",
        "file",
        ToolSchema {
            name: "read_file".to_string(),
            description: "Read a file and return its contents with line numbers.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read."
                    },
                    "offset": {
                        "type": "integer",
                        "description": "1-indexed line number to start reading from (default 1)."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to return (default 500, max 2000)."
                    }
                },
                "required": ["path"]
            }),
        },
        |args: Value, _ctx| {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "read_file".to_string(),
                    reason: "missing required field: path".to_string(),
                })?;

            let offset = args["offset"].as_u64().unwrap_or(1) as usize;
            let limit = args["limit"].as_u64().unwrap_or(500) as usize;
            let limit = limit.min(2000);

            let result = read_file(std::path::Path::new(path), offset, limit)?;

            Ok(ToolResult::ok(json!({
                "content": result.content,
                "total_lines": result.total_lines,
                "truncated": result.truncated,
            })))
        },
    );

    // write_file
    registry.register_sync(
        "write_file",
        "file",
        ToolSchema {
            name: "write_file".to_string(),
            description: "Write content to a file, overwriting any existing content.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write."
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file."
                    }
                },
                "required": ["path", "content"]
            }),
        },
        |args: Value, _ctx| {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "write_file".to_string(),
                    reason: "missing required field: path".to_string(),
                })?;

            let content = args["content"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "write_file".to_string(),
                    reason: "missing required field: content".to_string(),
                })?;

            let result = write_file(std::path::Path::new(path), content)?;

            Ok(ToolResult::ok(json!({
                "success": result.success,
                "lines_written": result.lines_written,
            })))
        },
    );

    // patch
    registry.register_sync(
        "patch",
        "file",
        ToolSchema {
            name: "patch".to_string(),
            description: "Replace occurrences of old_string with new_string in a file.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to patch."
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The string to find and replace."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement string."
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences (default false)."
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        |args: Value, _ctx| {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "patch".to_string(),
                    reason: "missing required field: path".to_string(),
                })?;

            let old_string = args["old_string"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "patch".to_string(),
                    reason: "missing required field: old_string".to_string(),
                })?;

            let new_string = args["new_string"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "patch".to_string(),
                    reason: "missing required field: new_string".to_string(),
                })?;

            let replace_all = args["replace_all"].as_bool().unwrap_or(false);

            let result = patch_file(
                std::path::Path::new(path),
                old_string,
                new_string,
                replace_all,
            )?;

            Ok(ToolResult::ok(json!({
                "success": result.success,
                "replacements": result.replacements,
                "diff": result.diff,
            })))
        },
    );

    // search_files
    registry.register_sync(
        "search_files",
        "file",
        ToolSchema {
            name: "search_files".to_string(),
            description: "Search files by content (regex) or filename (glob).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Regex pattern for content search or glob for file search."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["content", "files"],
                        "description": "Search mode: 'content' (default) or 'files' (glob)."
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (default '.')."
                    },
                    "file_glob": {
                        "type": "string",
                        "description": "Optional glob to filter files during content search."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default 50)."
                    }
                },
                "required": ["pattern"]
            }),
        },
        |args: Value, _ctx| {
            let pattern = args["pattern"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "search_files".to_string(),
                    reason: "missing required field: pattern".to_string(),
                })?;

            let target = args["target"].as_str().unwrap_or("content");
            let is_glob = target == "files";

            let dir = args["path"].as_str().unwrap_or(".");
            let file_glob = args["file_glob"].as_str();
            let limit = args["limit"].as_u64().unwrap_or(50) as usize;

            let matches = search_files(
                std::path::Path::new(dir),
                pattern,
                is_glob,
                file_glob,
                limit,
            )?;

            let results: Vec<Value> = matches
                .into_iter()
                .map(|m| {
                    json!({
                        "path": m.path,
                        "line_number": m.line_number,
                        "content": m.content,
                    })
                })
                .collect();

            Ok(ToolResult::ok(json!({ "results": results })))
        },
    );
}

// ── Web tools ─────────────────────────────────────────────────────────────────

fn register_web_tools(registry: &mut ToolRegistry) {
    let client = match TavilyClient::from_env() {
        Ok(c) => Arc::new(c),
        Err(_) => {
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
                description: "Search the web using Tavily and return relevant results.".to_string(),
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

                let results = match tokio::runtime::Handle::try_current() {
                    Ok(handle) => handle.block_on(client.search(&query)),
                    Err(_) => {
                        let rt = tokio::runtime::Runtime::new().map_err(|e| {
                            ToolError::ExecutionFailed(format!(
                                "failed to create tokio runtime: {e}"
                            ))
                        })?;
                        rt.block_on(client.search(&query))
                    }
                }?;

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
                let urls_val = args["urls"]
                    .as_array()
                    .ok_or_else(|| ToolError::InvalidArgs {
                        tool: "web_extract".to_string(),
                        reason: "missing required field: urls (must be an array)".to_string(),
                    })?;

                let urls: Vec<String> = urls_val
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();

                let client = Arc::clone(&client);

                let items = match tokio::runtime::Handle::try_current() {
                    Ok(handle) => handle.block_on(client.extract(&urls)),
                    Err(_) => {
                        let rt = tokio::runtime::Runtime::new().map_err(|e| {
                            ToolError::ExecutionFailed(format!(
                                "failed to create tokio runtime: {e}"
                            ))
                        })?;
                        rt.block_on(client.extract(&urls))
                    }
                }?;

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
