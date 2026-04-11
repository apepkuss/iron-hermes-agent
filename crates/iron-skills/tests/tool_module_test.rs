use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use iron_skills::manager::SkillManager;
use iron_skills::tool_module::SkillTools;
use iron_tool_api::{ToolContext, ToolModule, ToolRegistry};
use serde_json::json;

fn make_skill(root: &Path, category: &str, name: &str, desc: &str) {
    let dir = root.join(category).join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {desc}\n---\n# {name}\nBody.\n"),
    )
    .unwrap();
}

fn make_ctx() -> ToolContext {
    ToolContext {
        task_id: "test".to_string(),
        working_dir: std::path::PathBuf::from("."),
        enabled_tools: ["skills_list", "skill_view", "skill_manage"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

fn setup() -> (ToolRegistry, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    make_skill(&root, "research", "arxiv", "Search arXiv papers");
    make_skill(&root, "tools", "my-tool", "A custom tool");

    let manager = Arc::new(SkillManager::new(vec![root], HashSet::new()));
    let mut registry = ToolRegistry::new();
    let module: Box<dyn ToolModule> = Box::new(SkillTools { manager });
    module.register(&mut registry);

    (registry, tmp)
}

#[test]
fn test_skills_list_registered() {
    let (registry, _tmp) = setup();
    assert!(registry.has_tool("skills_list"));
    assert!(registry.has_tool("skill_view"));
    assert!(registry.has_tool("skill_manage"));
}

#[test]
fn test_skills_list_returns_skills() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();
    let result = registry
        .dispatch_sync("skills_list", json!({}), &ctx)
        .unwrap();
    assert!(result.success);
    let count = result.output["count"].as_u64().unwrap();
    assert_eq!(count, 2);
}

#[test]
fn test_skill_view_returns_content() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();
    let result = registry
        .dispatch_sync("skill_view", json!({"name": "arxiv"}), &ctx)
        .unwrap();
    assert!(result.success);
    assert_eq!(result.output["name"], "arxiv");
    assert!(result.output["content"].as_str().unwrap().contains("Body."));
}

#[test]
fn test_skill_view_not_found() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();
    let result = registry
        .dispatch_sync("skill_view", json!({"name": "nonexistent"}), &ctx)
        .unwrap();
    assert!(!result.success);
}

#[test]
fn test_skill_manage_create_and_delete() {
    let (registry, _tmp) = setup();
    let ctx = make_ctx();

    let content = "---\nname: new-skill\ndescription: A new skill\n---\n# New\nBody.\n";
    let result = registry
        .dispatch_sync(
            "skill_manage",
            json!({
                "action": "create",
                "name": "new-skill",
                "content": content,
                "category": "testing"
            }),
            &ctx,
        )
        .unwrap();
    assert!(result.success, "create failed: {:?}", result.output);

    let view = registry
        .dispatch_sync("skill_view", json!({"name": "new-skill"}), &ctx)
        .unwrap();
    assert!(view.success);

    let del = registry
        .dispatch_sync(
            "skill_manage",
            json!({"action": "delete", "name": "new-skill"}),
            &ctx,
        )
        .unwrap();
    assert!(del.success);
}
