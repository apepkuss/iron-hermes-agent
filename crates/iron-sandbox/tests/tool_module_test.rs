use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use iron_sandbox::tool_module::register_execute_code;
use iron_tool_api::{ToolContext, ToolRegistry};
use serde_json::json;

fn build_registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    let holder: Arc<OnceLock<Arc<ToolRegistry>>> = Arc::new(OnceLock::new());
    register_execute_code(&mut registry, Arc::clone(&holder));
    let registry = Arc::new(registry);
    holder
        .set(Arc::clone(&registry))
        .unwrap_or_else(|_| panic!("holder already set"));
    registry
}

fn make_ctx() -> ToolContext {
    ToolContext {
        task_id: "test".to_string(),
        working_dir: std::env::temp_dir(),
        enabled_tools: HashSet::from(["execute_code".to_string()]),
    }
}

#[test]
fn test_execute_code_registered() {
    let registry = build_registry();
    assert!(registry.has_tool("execute_code"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_execute_python() {
    let registry = build_registry();
    let ctx = make_ctx();
    let args = json!({ "code": "print('hello from python')", "language": "python" });
    let result = registry
        .dispatch_sync("execute_code", args, &ctx)
        .expect("dispatch failed");
    assert!(result.success, "expected success, got: {:?}", result.output);
    let stdout = result.output["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("hello from python"), "stdout was: {stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_execute_shell() {
    let registry = build_registry();
    let ctx = make_ctx();
    let args = json!({ "code": "echo 'hello from shell'", "language": "shell" });
    let result = registry
        .dispatch_sync("execute_code", args, &ctx)
        .expect("dispatch failed");
    assert!(result.success, "expected success, got: {:?}", result.output);
    let stdout = result.output["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("hello from shell"), "stdout was: {stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_execute_code_default_language_python() {
    let registry = build_registry();
    let ctx = make_ctx();
    // No "language" field — should default to python
    let args = json!({ "code": "print('default lang')" });
    let result = registry
        .dispatch_sync("execute_code", args, &ctx)
        .expect("dispatch failed");
    assert!(result.success, "expected success, got: {:?}", result.output);
    let stdout = result.output["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("default lang"), "stdout was: {stdout}");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_execute_code_missing_code_field() {
    let registry = build_registry();
    let ctx = make_ctx();
    let args = json!({ "language": "python" });
    let result = registry
        .dispatch_sync("execute_code", args, &ctx)
        .expect("dispatch failed");
    assert!(!result.success, "expected failure for missing 'code' field");
}
