use std::sync::Arc;

use iron_memory::manager::MemoryManager;
use iron_memory::tool_module::MemoryTools;
use iron_tool_api::{ToolContext, ToolModule, ToolRegistry};
use serde_json::json;
use tokio::sync::Mutex;

fn make_ctx() -> ToolContext {
    ToolContext {
        task_id: "test".to_string(),
        working_dir: std::path::PathBuf::from("."),
        enabled_tools: ["memory"].iter().map(|s| s.to_string()).collect(),
        env_vars: std::collections::HashMap::new(),
    }
}

fn setup() -> (ToolRegistry, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut mgr = MemoryManager::new(tmp.path(), None, None);
    mgr.initialize().unwrap();

    let manager = Arc::new(Mutex::new(mgr));
    let mut registry = ToolRegistry::new();
    let module: Box<dyn ToolModule> = Box::new(MemoryTools { manager });
    module.register(&mut registry);

    (registry, tmp)
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_tool_registered() {
    let (registry, _tmp) = setup();
    assert!(registry.has_tool("memory"));
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_add() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();

    let result = registry
        .dispatch_sync(
            "memory",
            json!({"action": "add", "target": "memory", "content": "Boss prefers Rust"}),
            &ctx,
        )
        .unwrap();

    assert!(result.success, "add failed: {:?}", result.output);
    assert_eq!(result.output["entry_count"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_replace() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();

    registry
        .dispatch_sync(
            "memory",
            json!({"action": "add", "target": "user", "content": "Name: Alice"}),
            &ctx,
        )
        .unwrap();

    let result = registry
        .dispatch_sync(
            "memory",
            json!({
                "action": "replace",
                "target": "user",
                "old_text": "Alice",
                "content": "Name: Bob"
            }),
            &ctx,
        )
        .unwrap();

    assert!(result.success, "replace failed: {:?}", result.output);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_remove() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();

    registry
        .dispatch_sync(
            "memory",
            json!({"action": "add", "target": "memory", "content": "temp note"}),
            &ctx,
        )
        .unwrap();

    let result = registry
        .dispatch_sync(
            "memory",
            json!({"action": "remove", "target": "memory", "old_text": "temp"}),
            &ctx,
        )
        .unwrap();

    assert!(result.success);
    assert_eq!(result.output["entry_count"], 0);
}
