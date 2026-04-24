use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use iron_core::cron_tool::register_cronjob;
use iron_core::session::store::SessionStore;
use iron_tool_api::{ToolContext, ToolRegistry};
use serde_json::json;

fn make_ctx() -> ToolContext {
    ToolContext {
        task_id: "test".to_string(),
        working_dir: PathBuf::from("."),
        enabled_tools: HashSet::from(["cronjob".to_string()]),
        env_vars: HashMap::new(),
    }
}

fn setup() -> ToolRegistry {
    let store = Arc::new(Mutex::new(SessionStore::new_in_memory().unwrap()));
    let mut registry = ToolRegistry::new();
    register_cronjob(&mut registry, store);
    registry
}

#[test]
fn test_cronjob_tool_create_and_list() {
    let registry = setup();
    let ctx = make_ctx();

    let created = registry
        .dispatch_sync(
            "cronjob",
            json!({
                "action": "create",
                "name": "Daily report",
                "prompt": "Summarize project status",
                "schedule": "every 2h"
            }),
            &ctx,
        )
        .unwrap();
    assert!(created.success);
    assert_eq!(created.output["job"]["name"], "Daily report");
    assert_eq!(created.output["job"]["enabled"], true);

    let listed = registry
        .dispatch_sync("cronjob", json!({ "action": "list" }), &ctx)
        .unwrap();
    assert!(listed.success);
    assert_eq!(listed.output["jobs"].as_array().unwrap().len(), 1);
}

#[test]
fn test_cronjob_tool_pause_resume_and_run() {
    let registry = setup();
    let ctx = make_ctx();

    let created = registry
        .dispatch_sync(
            "cronjob",
            json!({
                "action": "create",
                "prompt": "Run health check",
                "schedule": "daily 09:00"
            }),
            &ctx,
        )
        .unwrap();
    let id = created.output["job"]["id"].as_str().unwrap();

    let paused = registry
        .dispatch_sync("cronjob", json!({ "action": "pause", "id": id }), &ctx)
        .unwrap();
    assert_eq!(paused.output["job"]["enabled"], false);

    let resumed = registry
        .dispatch_sync("cronjob", json!({ "action": "resume", "id": id }), &ctx)
        .unwrap();
    assert_eq!(resumed.output["job"]["enabled"], true);

    let scheduled = registry
        .dispatch_sync("cronjob", json!({ "action": "run", "id": id }), &ctx)
        .unwrap();
    assert_eq!(scheduled.output["status"], "scheduled");
    assert!(
        scheduled.output["job"]["next_run_at_epoch"]
            .as_i64()
            .is_some()
    );
}
