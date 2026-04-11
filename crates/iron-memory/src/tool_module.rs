use std::sync::Arc;

use serde_json::{Value, json};
use tokio::sync::Mutex;

use iron_tool_api::{ToolError, ToolModule, ToolRegistry, ToolResult, ToolSchema};

use crate::manager::MemoryManager;

pub struct MemoryTools {
    pub manager: Arc<Mutex<MemoryManager>>,
}

impl ToolModule for MemoryTools {
    fn register(self: Box<Self>, registry: &mut ToolRegistry) {
        let manager = self.manager;

        registry.register_sync(
            "memory",
            "memory",
            ToolSchema {
                name: "memory".to_string(),
                description: "Save durable information to persistent memory that survives across sessions. Targets: 'memory' (agent notes) or 'user' (user profile). Actions: add, replace, remove.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["add", "replace", "remove"],
                            "description": "The action to perform."
                        },
                        "target": {
                            "type": "string",
                            "enum": ["memory", "user"],
                            "description": "Which memory store to operate on."
                        },
                        "content": {
                            "type": "string",
                            "description": "The entry content. Required for 'add' and 'replace'."
                        },
                        "old_text": {
                            "type": "string",
                            "description": "Unique substring identifying the entry to replace or remove."
                        }
                    },
                    "required": ["action", "target"]
                }),
            },
            move |args: Value, _ctx| {
                let action = args["action"].as_str().ok_or_else(|| ToolError::InvalidArgs {
                    tool: "memory".into(),
                    reason: "missing required field: action".into(),
                })?;
                let target = args["target"].as_str().ok_or_else(|| ToolError::InvalidArgs {
                    tool: "memory".into(),
                    reason: "missing required field: target".into(),
                })?;
                let content = args["content"].as_str();
                let old_text = args["old_text"].as_str();

                let manager = Arc::clone(&manager);
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        let mut mgr = manager.lock().await;
                        mgr.handle_tool_call(action, target, content, old_text)
                    })
                });

                match result {
                    Ok(value) => Ok(ToolResult::ok(value)),
                    Err(e) => Ok(ToolResult::err(&e.to_string())),
                }
            },
        );
    }
}
