//! TerminalTools — ToolModule implementation for the terminal tool.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    ToolModule, ToolRegistry, ToolResult, ToolSchema,
    error::ToolError,
    terminal::{TerminalParams, TerminalTool},
};

/// A `ToolModule` that registers the `terminal` tool.
pub struct TerminalTools {
    pub default_timeout: u64,
}

impl TerminalTools {
    pub fn new(default_timeout: u64) -> Self {
        Self { default_timeout }
    }
}

impl ToolModule for TerminalTools {
    fn register(self: Box<Self>, registry: &mut ToolRegistry) {
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

        let tool = Arc::new(TerminalTool::new(self.default_timeout));

        registry.register_sync("terminal", "terminal", schema, move |args: Value, ctx| {
            let command = args["command"]
                .as_str()
                .ok_or_else(|| ToolError::InvalidArgs {
                    tool: "terminal".to_string(),
                    reason: "missing required field: command".to_string(),
                })?
                .to_string();

            let timeout = args["timeout"].as_u64();

            // Working directory: explicit arg > session default from ToolContext
            let workdir = args["workdir"]
                .as_str()
                .map(std::path::PathBuf::from)
                .or_else(|| Some(ctx.working_dir.clone()));

            let params = TerminalParams {
                command,
                background: false,
                timeout,
                workdir,
                env_vars: Some(ctx.env_vars.clone()),
            };

            let tool = Arc::clone(&tool);

            let res = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(tool.execute(params))
            })?;

            Ok(ToolResult::ok(json!({
                "stdout": res.stdout,
                "stderr": res.stderr,
                "exit_code": res.exit_code,
                "truncated": res.truncated,
            })))
        });
    }
}
