use std::collections::HashSet;
use std::sync::Arc;

use iron_core::todo::{
    TodoSenders, create_todo_channel, new_todo_senders, new_todo_state, register_todo,
};
use iron_tool_api::registry::ToolRegistry;
use iron_tool_api::types::ToolContext;
use serde_json::json;

/// Helper: build a ToolContext with the given session id.
fn ctx(session_id: &str) -> ToolContext {
    let mut enabled = HashSet::new();
    enabled.insert("todo".to_string());
    ToolContext {
        task_id: session_id.to_string(),
        working_dir: std::env::current_dir().unwrap(),
        enabled_tools: enabled,
    }
}

/// Helper: register the todo tool and return (registry, senders).
fn setup() -> (ToolRegistry, TodoSenders) {
    let state = new_todo_state();
    let senders = new_todo_senders();
    let mut registry = ToolRegistry::new();
    register_todo(&mut registry, state, Arc::clone(&senders));
    (registry, senders)
}

#[test]
fn test_todo_set() {
    let (registry, senders) = setup();
    let session = "sess-1";
    let rx = create_todo_channel(&senders, session);

    let result = registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "set",
                "todos": [
                    { "content": "Task A", "status": "pending" },
                    { "content": "Task B", "status": "in_progress" },
                ]
            }),
            &ctx(session),
        )
        .unwrap();

    assert!(result.success);
    assert_eq!(result.output["total"], 2);
    assert_eq!(result.output["pending"], 2);
    assert_eq!(result.output["completed"], 0);

    // Verify event was sent
    let rx = rx.lock().unwrap();
    let todos = rx.try_recv().unwrap();
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[0].content, "Task A");
    assert_eq!(todos[0].status, "pending");
    assert_eq!(todos[1].content, "Task B");
    assert_eq!(todos[1].status, "in_progress");
}

#[test]
fn test_todo_update() {
    let (registry, senders) = setup();
    let session = "sess-2";
    let rx = create_todo_channel(&senders, session);

    // First set the list
    registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "set",
                "todos": [
                    { "content": "Task A", "status": "pending" },
                    { "content": "Task B", "status": "pending" },
                ]
            }),
            &ctx(session),
        )
        .unwrap();

    // Drain set event
    let rx = rx.lock().unwrap();
    let _ = rx.try_recv();

    // Update item 0 to completed
    let result = registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "update",
                "index": 0,
                "status": "completed"
            }),
            &ctx(session),
        )
        .unwrap();

    assert!(result.success);

    let todos = rx.try_recv().unwrap();
    assert_eq!(todos[0].status, "completed");
    assert_eq!(todos[1].status, "pending");
}

#[test]
fn test_todo_update_out_of_bounds() {
    let (registry, senders) = setup();
    let session = "sess-3";
    let _rx = create_todo_channel(&senders, session);

    // Set a list with 1 item
    registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "set",
                "todos": [{ "content": "Only item", "status": "pending" }]
            }),
            &ctx(session),
        )
        .unwrap();

    // Try to update index 5 (out of bounds)
    let result = registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "update",
                "index": 5,
                "status": "completed"
            }),
            &ctx(session),
        )
        .unwrap();

    assert!(!result.success);
    assert!(result.output.as_str().unwrap().contains("out of bounds"));
}

#[test]
fn test_todo_event_sent() {
    let (registry, _senders) = setup();
    let session = "sess-4";
    let rx = create_todo_channel(&_senders, session);

    // Dispatch set — verify event arrives
    registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "set",
                "todos": [{ "content": "Check event", "status": "pending" }]
            }),
            &ctx(session),
        )
        .unwrap();

    {
        let guard = rx.lock().unwrap();
        let todos = guard
            .try_recv()
            .expect("should receive TodoUpdate after set");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "Check event");
    }

    // Dispatch update — verify a second event arrives
    registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "update",
                "index": 0,
                "status": "completed"
            }),
            &ctx(session),
        )
        .unwrap();

    {
        let guard = rx.lock().unwrap();
        let todos = guard
            .try_recv()
            .expect("should receive TodoUpdate after update");
        assert_eq!(todos[0].status, "completed");
    }
}

#[test]
fn test_todo_state_per_session() {
    let (registry, senders) = setup();

    let session_a = "sess-a";
    let session_b = "sess-b";
    let rx_a = create_todo_channel(&senders, session_a);
    let rx_b = create_todo_channel(&senders, session_b);

    // Set different lists for each session
    registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "set",
                "todos": [{ "content": "Session A task", "status": "pending" }]
            }),
            &ctx(session_a),
        )
        .unwrap();

    registry
        .dispatch_sync(
            "todo",
            json!({
                "action": "set",
                "todos": [
                    { "content": "Session B task 1", "status": "pending" },
                    { "content": "Session B task 2", "status": "pending" },
                ]
            }),
            &ctx(session_b),
        )
        .unwrap();

    // Verify session A got its own event
    let rx_a = rx_a.lock().unwrap();
    let todos_a = rx_a.try_recv().unwrap();
    assert_eq!(todos_a.len(), 1);
    assert_eq!(todos_a[0].content, "Session A task");

    // Session A should NOT have received session B's event
    assert!(rx_a.try_recv().is_err());

    // Verify session B got its own event
    let rx_b = rx_b.lock().unwrap();
    let todos_b = rx_b.try_recv().unwrap();
    assert_eq!(todos_b.len(), 2);
    assert_eq!(todos_b[0].content, "Session B task 1");
}
