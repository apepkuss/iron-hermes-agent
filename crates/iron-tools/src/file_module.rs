//! FileTools — ToolModule implementation for file tools.

use serde_json::{Value, json};

use crate::{
    ToolModule, ToolRegistry, ToolResult, ToolSchema,
    error::ToolError,
    file::{patch_file, read_file, search_files, write_file},
};

/// A `ToolModule` that registers the file tools:
/// `read_file`, `write_file`, `patch`, `search_files`.
pub struct FileTools;

impl ToolModule for FileTools {
    fn register(self: Box<Self>, registry: &mut ToolRegistry) {
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
                description: "Write content to a file, overwriting any existing content."
                    .to_string(),
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
                description: "Replace occurrences of old_string with new_string in a file."
                    .to_string(),
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

                let old_string =
                    args["old_string"]
                        .as_str()
                        .ok_or_else(|| ToolError::InvalidArgs {
                            tool: "patch".to_string(),
                            reason: "missing required field: old_string".to_string(),
                        })?;

                let new_string =
                    args["new_string"]
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
}
