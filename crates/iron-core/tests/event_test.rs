use iron_core::event::{AgentEvent, TodoItem, build_args_preview, truncate_preview};

#[test]
fn text_delta_serialization() {
    let event = AgentEvent::TextDelta {
        content: "Hello".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "TextDelta");
    assert_eq!(json["content"], "Hello");
}

#[test]
fn tool_started_serialization() {
    let event = AgentEvent::ToolStarted {
        tool: "bash".to_string(),
        args_preview: "cmd=\"ls\"".to_string(),
        call_id: "call-123".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "ToolStarted");
    assert_eq!(json["tool"], "bash");
    assert_eq!(json["args_preview"], "cmd=\"ls\"");
    assert_eq!(json["call_id"], "call-123");
}

#[test]
fn tool_completed_serialization() {
    let event = AgentEvent::ToolCompleted {
        tool: "bash".to_string(),
        call_id: "call-456".to_string(),
        duration_ms: 150,
        success: true,
        result_preview: "ok".to_string(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "ToolCompleted");
    assert_eq!(json["duration_ms"], 150);
    assert_eq!(json["success"], true);
    assert_eq!(json["result_preview"], "ok");
}

#[test]
fn todo_update_serialization() {
    let event = AgentEvent::TodoUpdate {
        todos: vec![
            TodoItem {
                content: "Task 1".to_string(),
                status: "pending".to_string(),
            },
            TodoItem {
                content: "Task 2".to_string(),
                status: "completed".to_string(),
            },
        ],
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "TodoUpdate");
    let todos = json["todos"].as_array().unwrap();
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[0]["content"], "Task 1");
    assert_eq!(todos[1]["status"], "completed");
}

#[test]
fn build_args_preview_basic() {
    let json = r#"{"name": "arxiv", "category": "research"}"#;
    let preview = build_args_preview(json);
    assert!(preview.contains("name="), "should contain 'name='");
    assert!(preview.contains("arxiv"), "should contain 'arxiv'");
}

#[test]
fn build_args_preview_truncates_long_values() {
    let long_val = "a".repeat(50);
    let json = format!(r#"{{"key": "{long_val}"}}"#);
    let preview = build_args_preview(&json);
    assert!(preview.contains("..."), "should truncate with '...'");
}

#[test]
fn build_args_preview_empty() {
    let preview = build_args_preview("{}");
    assert_eq!(preview, "");
}

#[test]
fn build_args_preview_invalid_json() {
    let preview = build_args_preview("not json");
    assert_eq!(preview, "");
}

#[test]
fn truncate_preview_short() {
    let result = truncate_preview("hello", 100);
    assert_eq!(result, "hello");
}

#[test]
fn truncate_preview_long() {
    let long_str = "x".repeat(200);
    let result = truncate_preview(&long_str, 50);
    assert!(result.ends_with("..."), "should end with '...'");
    assert_eq!(result.len(), 53); // 50 chars + "..."
}

#[test]
fn truncate_preview_exact_boundary() {
    let s = "a".repeat(50);
    let result = truncate_preview(&s, 50);
    // exactly at boundary, no truncation
    assert_eq!(result, s);
    assert!(!result.ends_with("..."));
}
