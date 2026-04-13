use iron_core::event::{AgentEvent, TodoItem};

#[test]
fn test_todo_item_status_values() {
    let items = vec![
        TodoItem {
            content: "Task 1".into(),
            status: "pending".into(),
        },
        TodoItem {
            content: "Task 2".into(),
            status: "in_progress".into(),
        },
        TodoItem {
            content: "Task 3".into(),
            status: "completed".into(),
        },
    ];

    let event = AgentEvent::TodoUpdate { todos: items };
    let json = serde_json::to_value(&event).unwrap();

    assert_eq!(json["todos"].as_array().unwrap().len(), 3);
    assert_eq!(json["todos"][0]["status"], "pending");
    assert_eq!(json["todos"][1]["status"], "in_progress");
    assert_eq!(json["todos"][2]["status"], "completed");
}

#[test]
fn test_tool_started_event_format() {
    let event = AgentEvent::ToolStarted {
        tool: "execute_code".into(),
        args_preview: "code=\"print(42)\", language=\"python\"".into(),
        call_id: "call_abc".into(),
    };
    let json = serde_json::to_value(&event).unwrap();

    assert!(json["tool"].is_string());
    assert!(json["args_preview"].is_string());
    assert!(json["call_id"].is_string());
    assert_eq!(json["type"], "ToolStarted");
}

#[test]
fn test_tool_completed_event_format() {
    let event = AgentEvent::ToolCompleted {
        tool: "read_file".into(),
        call_id: "call_xyz".into(),
        duration_ms: 12,
        success: true,
        result_preview: "42 lines".into(),
    };
    let json = serde_json::to_value(&event).unwrap();

    assert!(json["duration_ms"].is_number());
    assert!(json["success"].is_boolean());
    assert_eq!(json["type"], "ToolCompleted");
}

#[test]
fn test_text_delta_event_format() {
    let event = AgentEvent::TextDelta {
        content: "chunk".into(),
    };
    let json = serde_json::to_value(&event).unwrap();

    assert_eq!(json["type"], "TextDelta");
    assert_eq!(json["content"], "chunk");
}
