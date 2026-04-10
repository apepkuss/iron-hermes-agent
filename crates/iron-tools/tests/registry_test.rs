use iron_tools::{
    error::ToolError,
    registry::ToolRegistry,
    types::{ToolContext, ToolResult, ToolSchema},
};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::PathBuf;

fn make_schema(name: &str) -> ToolSchema {
    ToolSchema {
        name: name.to_string(),
        description: format!("Description for {}", name),
        parameters: json!({
            "type": "object",
            "properties": {
                "input": { "type": "string" }
            },
            "required": ["input"]
        }),
    }
}

fn make_ctx(enabled: &[&str]) -> ToolContext {
    ToolContext {
        task_id: "test-task".to_string(),
        working_dir: PathBuf::from("/tmp"),
        enabled_tools: enabled.iter().map(|s| s.to_string()).collect(),
    }
}

// Test 1: Register a tool and verify it appears in schemas
#[test]
fn test_register_tool_appears_in_schemas() {
    let mut registry = ToolRegistry::new();
    let schema = make_schema("echo_tool");

    registry.register_sync("echo_tool", "test_toolset", schema, |args, _ctx| {
        Ok(ToolResult::ok(args))
    });

    assert!(registry.has_tool("echo_tool"));
    assert!(registry.tool_names().contains("echo_tool"));

    let ctx = make_ctx(&["echo_tool"]);
    let schemas = registry.get_schemas(&ctx);
    assert_eq!(schemas.len(), 1);

    let schema_val = &schemas[0];
    assert_eq!(schema_val["type"], "function");
    assert_eq!(schema_val["function"]["name"], "echo_tool");
    assert_eq!(
        schema_val["function"]["description"],
        "Description for echo_tool"
    );
}

// Test 2: Dispatch an unknown tool returns error
#[test]
fn test_dispatch_unknown_tool_returns_error() {
    let registry = ToolRegistry::new();
    let ctx = make_ctx(&[]);

    let result = registry.dispatch_sync("nonexistent", json!({}), &ctx);
    assert!(result.is_err());

    match result.unwrap_err() {
        ToolError::NotFound(name) => assert_eq!(name, "nonexistent"),
        other => panic!("Expected NotFound, got {:?}", other),
    }
}

// Test 3: Dispatch a registered tool returns correct result
#[test]
fn test_dispatch_registered_tool_returns_correct_result() {
    let mut registry = ToolRegistry::new();
    let schema = make_schema("add_tool");

    registry.register_sync("add_tool", "math", schema, |args, _ctx| {
        let a = args["a"].as_i64().unwrap_or(0);
        let b = args["b"].as_i64().unwrap_or(0);
        Ok(ToolResult::ok(json!({ "sum": a + b })))
    });

    let ctx = make_ctx(&["add_tool"]);
    let result = registry
        .dispatch_sync("add_tool", json!({"a": 3, "b": 4}), &ctx)
        .expect("dispatch should succeed");

    assert!(result.success);
    assert_eq!(result.output["sum"], 7);
}

// Test 4: get_schemas() filters by enabled set
#[test]
fn test_get_schemas_filters_by_enabled_set() {
    let mut registry = ToolRegistry::new();

    for name in &["tool_a", "tool_b", "tool_c"] {
        let schema = make_schema(name);
        registry.register_sync(name, "toolset", schema, |_, _| {
            Ok(ToolResult::ok(Value::Null))
        });
    }

    assert_eq!(registry.tool_names().len(), 3);

    // Enable only tool_a and tool_c
    let ctx = make_ctx(&["tool_a", "tool_c"]);
    let schemas = registry.get_schemas(&ctx);
    assert_eq!(schemas.len(), 2);

    let names: HashSet<String> = schemas
        .iter()
        .map(|s| s["function"]["name"].as_str().unwrap().to_string())
        .collect();

    assert!(names.contains("tool_a"));
    assert!(names.contains("tool_c"));
    assert!(!names.contains("tool_b"));

    // Empty enabled set returns no schemas
    let ctx_empty = make_ctx(&[]);
    assert_eq!(registry.get_schemas(&ctx_empty).len(), 0);

    // All enabled
    let ctx_all = make_ctx(&["tool_a", "tool_b", "tool_c"]);
    assert_eq!(registry.get_schemas(&ctx_all).len(), 3);
}

// Test 5: ToolResult helpers
#[test]
fn test_tool_result_helpers() {
    let ok = ToolResult::ok(json!({"key": "value"}));
    assert!(ok.success);
    assert_eq!(ok.output["key"], "value");

    let err = ToolResult::err("something went wrong");
    assert!(!err.success);
    assert_eq!(
        err.output,
        Value::String("something went wrong".to_string())
    );
}

// Test 6: Handler returns ToolError properly
#[test]
fn test_handler_can_return_tool_error() {
    let mut registry = ToolRegistry::new();
    let schema = make_schema("failing_tool");

    registry.register_sync("failing_tool", "test", schema, |_args, _ctx| {
        Err(ToolError::ExecutionFailed(
            "intentional failure".to_string(),
        ))
    });

    let ctx = make_ctx(&["failing_tool"]);
    let result = registry.dispatch_sync("failing_tool", json!({}), &ctx);
    assert!(result.is_err());

    match result.unwrap_err() {
        ToolError::ExecutionFailed(msg) => assert_eq!(msg, "intentional failure"),
        other => panic!("Expected ExecutionFailed, got {:?}", other),
    }
}
