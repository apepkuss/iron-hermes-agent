use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use iron_tool_api::{ToolRegistry, ToolResult, ToolSchema};
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::bridge::SANDBOX_TOOL_WHITELIST;
use crate::sandbox::{Sandbox, SandboxConfig, SandboxStatus};

/// Register the `execute_code` tool into a [`ToolRegistry`].
///
/// The handler captures `registry_holder` — a [`OnceLock`] that will be
/// populated with the final [`Arc<ToolRegistry>`] immediately after this
/// function returns.  This breaks the chicken-and-egg cycle: the registry
/// must exist before the handler is registered, but the handler needs the
/// registry for sandbox RPC dispatch.
pub fn register_execute_code(
    registry: &mut ToolRegistry,
    registry_holder: Arc<OnceLock<Arc<ToolRegistry>>>,
) {
    let schema = ToolSchema {
        name: "execute_code".to_string(),
        description: "Execute Python or Shell code in an isolated sandbox. \
Code can call whitelisted tools (read_file, write_file, search_files, patch, \
terminal, web_search, web_extract) via built-in functions."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "The code to execute."
                },
                "language": {
                    "type": "string",
                    "enum": ["python", "shell"],
                    "description": "The language of the code. Default: python."
                }
            },
            "required": ["code"]
        }),
    };

    registry.register_sync(
        "execute_code",
        "sandbox",
        schema,
        move |args: Value, _ctx| {
            let code = match args["code"].as_str() {
                Some(c) => c.to_string(),
                None => return Ok(ToolResult::err("missing required field: code")),
            };
            let language = args["language"].as_str().unwrap_or("python").to_string();

            let dispatch_registry = match registry_holder.get() {
                Some(r) => Arc::clone(r),
                None => return Ok(ToolResult::err("sandbox registry not yet initialized")),
            };

            let enabled_tools: HashSet<String> = SANDBOX_TOOL_WHITELIST
                .iter()
                .map(|s| s.to_string())
                .collect();

            let sandbox = Sandbox::new(SandboxConfig::default(), dispatch_registry, enabled_tools);

            let result = tokio::task::block_in_place(|| {
                Handle::current().block_on(async {
                    match language.as_str() {
                        "shell" => sandbox.execute_shell(&code).await,
                        _ => sandbox.execute_python(&code).await,
                    }
                })
            });

            match result {
                Ok(r) => {
                    let status_str = match r.status {
                        SandboxStatus::Success => "success",
                        SandboxStatus::Error => "error",
                        SandboxStatus::Timeout => "timeout",
                        SandboxStatus::Interrupted => "interrupted",
                    };
                    Ok(ToolResult::ok(json!({
                        "stdout": r.stdout,
                        "stderr": r.stderr,
                        "status": status_str,
                        "duration_ms": r.duration.as_millis(),
                        "tool_calls_made": r.tool_calls_made,
                    })))
                }
                Err(e) => Ok(ToolResult::err(&e.to_string())),
            }
        },
    );
}
