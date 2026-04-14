//! ToDo tool — per-session task list managed by the LLM.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

use iron_tool_api::error::ToolError;
use iron_tool_api::registry::ToolRegistry;
use iron_tool_api::types::{ToolResult, ToolSchema};
use serde_json::{Value, json};

use crate::event::TodoItem;

/// Per-session todo state, keyed by session_id (task_id).
pub type TodoState = Arc<Mutex<HashMap<String, Vec<TodoItem>>>>;

/// Per-session event senders, keyed by session_id.
pub type TodoSenders = Arc<Mutex<HashMap<String, mpsc::Sender<Vec<TodoItem>>>>>;

/// Receiver half for todo update events, wrapped in `Mutex` to be `Sync`.
pub type TodoEventReceiver = Mutex<mpsc::Receiver<Vec<TodoItem>>>;

/// Create a new empty `TodoState`.
pub fn new_todo_state() -> TodoState {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Create a new empty `TodoSenders`.
pub fn new_todo_senders() -> TodoSenders {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Create a todo event channel for the given session and register its sender.
///
/// Returns the receiver to be passed to `Agent::new()`.
pub fn create_todo_channel(senders: &TodoSenders, session_id: &str) -> TodoEventReceiver {
    let (tx, rx) = mpsc::channel();
    senders.lock().unwrap().insert(session_id.to_string(), tx);
    Mutex::new(rx)
}

/// Register the `todo` tool into the given registry.
///
/// The handler captures `state` and `senders` via closure.  After every
/// mutation it looks up the sender for the current session (via
/// `ToolContext.task_id`) and sends the updated todo list so that the Agent
/// dispatch loop can relay a `TodoUpdate` event to the frontend.
pub fn register_todo(registry: &mut ToolRegistry, state: TodoState, senders: TodoSenders) {
    registry.register_sync(
        "todo",
        "agent",
        ToolSchema {
            name: "todo".to_string(),
            description: "Track progress on multi-step work. Rules: \
                1) Call 'set' ONCE at the start to create the full task list, then NEVER use 'set' again. \
                2) Execute a task using the appropriate tool (e.g. execute_code, skills_list, memory). \
                3) Call 'update' to mark that task's status (keep all items, only change status). \
                4) Repeat steps 2-3 for each remaining task. \
                IMPORTANT: Never call this tool twice in a row. Always call another tool between todo calls."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["set", "update"],
                        "description": "'set' replaces the entire list, 'update' changes one item's status"
                    },
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string", "description": "Task description" },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "completed"] }
                            },
                            "required": ["content", "status"]
                        },
                        "description": "Full todo list. Required for 'set'."
                    },
                    "index": {
                        "type": "integer",
                        "description": "0-based index of the item to update. Required for 'update'."
                    },
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed"],
                        "description": "New status for the item. Required for 'update'."
                    }
                },
                "required": ["action"]
            }),
        },
        move |args: Value, ctx| {
            let action = args["action"].as_str().ok_or_else(|| ToolError::InvalidArgs {
                tool: "todo".to_string(),
                reason: "missing required field: action".to_string(),
            })?;

            let session_id = &ctx.task_id;
            let mut map = state.lock().unwrap();

            match action {
                "set" => {
                    let raw = args["todos"]
                        .as_array()
                        .ok_or_else(|| ToolError::InvalidArgs {
                            tool: "todo".to_string(),
                            reason: "missing required field: todos for 'set' action".to_string(),
                        })?;

                    let todos: Vec<TodoItem> = raw
                        .iter()
                        .map(|item| TodoItem {
                            content: item["content"].as_str().unwrap_or("").to_string(),
                            status: item["status"].as_str().unwrap_or("pending").to_string(),
                        })
                        .collect();

                    map.insert(session_id.clone(), todos.clone());

                    // Send event to the session's Agent via its channel.
                    let senders_map = senders.lock().unwrap();
                    if let Some(tx) = senders_map.get(session_id) {
                        let _ = tx.send(todos.clone());
                    }

                    Ok(ToolResult::ok(build_result_summary(&todos)))
                }
                "update" => {
                    let index = args["index"]
                        .as_u64()
                        .ok_or_else(|| ToolError::InvalidArgs {
                            tool: "todo".to_string(),
                            reason: "missing required field: index for 'update' action".to_string(),
                        })? as usize;

                    let new_status =
                        args["status"]
                            .as_str()
                            .ok_or_else(|| ToolError::InvalidArgs {
                                tool: "todo".to_string(),
                                reason: "missing required field: status for 'update' action"
                                    .to_string(),
                            })?;

                    let list = map.entry(session_id.clone()).or_default();

                    if index >= list.len() {
                        return Ok(ToolResult::err(&format!(
                            "index {} out of bounds (list has {} items)",
                            index,
                            list.len()
                        )));
                    }

                    list[index].status = new_status.to_string();

                    let snapshot = list.clone();

                    // Send event to the session's Agent via its channel.
                    let senders_map = senders.lock().unwrap();
                    if let Some(tx) = senders_map.get(session_id) {
                        let _ = tx.send(snapshot.clone());
                    }

                    Ok(ToolResult::ok(build_result_summary(&snapshot)))
                }
                other => Ok(ToolResult::err(&format!("unknown action: {}", other))),
            }
        },
    );
}

/// Build a result summary with pending/completed counts and next pending tasks.
fn build_result_summary(todos: &[TodoItem]) -> Value {
    let total = todos.len();
    let completed = todos.iter().filter(|t| t.status == "completed").count();
    let pending: Vec<&str> = todos
        .iter()
        .filter(|t| t.status != "completed")
        .map(|t| t.content.as_str())
        .collect();

    let mut result = json!({
        "success": true,
        "total": total,
        "completed": completed,
        "pending": pending.len(),
    });

    if !pending.is_empty() {
        result["next_steps"] = json!(pending);
        result["hint"] = json!("Now execute the pending tasks using the appropriate tools.");
    }

    result
}
