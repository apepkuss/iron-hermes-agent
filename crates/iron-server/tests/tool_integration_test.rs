use std::collections::HashSet;
use std::sync::Arc;

use iron_memory::manager::MemoryManager;
use iron_memory::tool_module::MemoryTools;
use iron_skills::manager::SkillManager;
use iron_skills::tool_module::SkillTools;
use iron_tool_api::{ToolContext, ToolModule, ToolRegistry};
use iron_tools::{file_module::FileTools, terminal_module::TerminalTools, web_module::WebTools};
use serde_json::json;
use tokio::sync::Mutex;

fn build_full_registry() -> (ToolRegistry, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().unwrap();
    let skills_dir = tmp.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let skill_manager = SkillManager::new(vec![skills_dir.clone()], HashSet::new());
    skill_manager.ensure_bundled_skills().unwrap();
    let skill_manager = Arc::new(skill_manager);

    let memory_dir = tmp.path().join("memories");
    std::fs::create_dir_all(&memory_dir).unwrap();
    let mut memory_manager = MemoryManager::new(&memory_dir, None, None);
    memory_manager.initialize().unwrap();
    let memory_manager = Arc::new(Mutex::new(memory_manager));

    let mut registry = ToolRegistry::new();

    let modules: Vec<Box<dyn ToolModule>> = vec![
        Box::new(TerminalTools::new(30)),
        Box::new(FileTools),
        Box::new(WebTools::from_env()),
        Box::new(SkillTools {
            manager: Arc::clone(&skill_manager),
        }),
        Box::new(MemoryTools {
            manager: Arc::clone(&memory_manager),
        }),
    ];

    for module in modules {
        module.register(&mut registry);
    }

    (registry, tmp)
}

fn make_ctx(registry: &ToolRegistry) -> ToolContext {
    ToolContext {
        task_id: "integration-test".to_string(),
        working_dir: std::path::PathBuf::from("."),
        enabled_tools: registry.tool_names(),
    }
}

#[test]
fn test_all_expected_tools_registered() {
    let (registry, _tmp) = build_full_registry();
    let names = registry.tool_names();

    assert!(names.contains("terminal"), "missing: terminal");
    assert!(names.contains("read_file"), "missing: read_file");
    assert!(names.contains("write_file"), "missing: write_file");
    assert!(names.contains("patch"), "missing: patch");
    assert!(names.contains("search_files"), "missing: search_files");
    assert!(names.contains("skills_list"), "missing: skills_list");
    assert!(names.contains("skill_view"), "missing: skill_view");
    assert!(names.contains("skill_manage"), "missing: skill_manage");
    assert!(names.contains("memory"), "missing: memory");
    assert!(
        names.len() >= 9,
        "expected at least 9 tools, got {}",
        names.len()
    );
}

#[test]
fn test_skills_list_returns_bundled_skills() {
    let (registry, _tmp) = build_full_registry();
    let ctx = make_ctx(&registry);

    let result = registry
        .dispatch_sync("skills_list", json!({}), &ctx)
        .unwrap();

    assert!(result.success);
    let count = result.output["count"].as_u64().unwrap();
    assert!(count > 50, "expected >50 bundled skills, got {count}");
}

#[test]
fn test_skills_list_category_filter() {
    let (registry, _tmp) = build_full_registry();
    let ctx = make_ctx(&registry);

    let result = registry
        .dispatch_sync("skills_list", json!({"category": "research"}), &ctx)
        .unwrap();

    assert!(result.success);
    let skills = result.output["skills"].as_array().unwrap();
    for skill in skills {
        assert_eq!(
            skill["category"].as_str().unwrap(),
            "research",
            "skill '{}' has wrong category",
            skill["name"]
        );
    }
    assert!(!skills.is_empty(), "research category should not be empty");
}

#[test]
fn test_skill_view_known_skill() {
    let (registry, _tmp) = build_full_registry();
    let ctx = make_ctx(&registry);

    // Try to view a skill that we know exists in bundled skills.
    // First list skills to find one that exists.
    let list_result = registry
        .dispatch_sync("skills_list", json!({}), &ctx)
        .unwrap();
    let skills = list_result.output["skills"].as_array().unwrap();
    assert!(!skills.is_empty(), "should have bundled skills");

    let first_skill_name = skills[0]["name"].as_str().unwrap();

    let result = registry
        .dispatch_sync("skill_view", json!({"name": first_skill_name}), &ctx)
        .unwrap();

    assert!(result.success, "skill_view failed: {:?}", result.output);
    assert_eq!(result.output["name"], first_skill_name);
    let content = result.output["content"].as_str().unwrap();
    assert!(!content.is_empty(), "skill content should not be empty");
}

#[test]
fn test_skill_view_not_found() {
    let (registry, _tmp) = build_full_registry();
    let ctx = make_ctx(&registry);

    let result = registry
        .dispatch_sync(
            "skill_view",
            json!({"name": "this-skill-does-not-exist-xyz"}),
            &ctx,
        )
        .unwrap();

    assert!(!result.success);
}

#[test]
fn test_skill_manage_create_view_delete() {
    let (registry, _tmp) = build_full_registry();
    let ctx = make_ctx(&registry);

    let skill_content = "---\nname: integration-test-skill\ndescription: Created by integration test\n---\n# Test\nBody.\n";

    let create = registry
        .dispatch_sync(
            "skill_manage",
            json!({
                "action": "create",
                "name": "integration-test-skill",
                "content": skill_content,
                "category": "testing"
            }),
            &ctx,
        )
        .unwrap();
    assert!(create.success, "create failed: {:?}", create.output);

    let view = registry
        .dispatch_sync(
            "skill_view",
            json!({"name": "integration-test-skill"}),
            &ctx,
        )
        .unwrap();
    assert!(view.success);
    assert!(view.output["content"].as_str().unwrap().contains("Body."));

    let delete = registry
        .dispatch_sync(
            "skill_manage",
            json!({"action": "delete", "name": "integration-test-skill"}),
            &ctx,
        )
        .unwrap();
    assert!(delete.success);

    let view_after = registry
        .dispatch_sync(
            "skill_view",
            json!({"name": "integration-test-skill"}),
            &ctx,
        )
        .unwrap();
    assert!(!view_after.success);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_full_lifecycle() {
    let (registry, _tmp) = build_full_registry();
    let ctx = make_ctx(&registry);

    let add = registry
        .dispatch_sync(
            "memory",
            json!({"action": "add", "target": "user", "content": "Boss prefers Rust over Python"}),
            &ctx,
        )
        .unwrap();
    assert!(add.success, "add failed: {:?}", add.output);
    assert_eq!(add.output["entry_count"], 1);

    let replace = registry
        .dispatch_sync(
            "memory",
            json!({
                "action": "replace",
                "target": "user",
                "old_text": "Python",
                "content": "Boss prefers Rust over Go"
            }),
            &ctx,
        )
        .unwrap();
    assert!(replace.success, "replace failed: {:?}", replace.output);

    let remove = registry
        .dispatch_sync(
            "memory",
            json!({"action": "remove", "target": "user", "old_text": "Rust over Go"}),
            &ctx,
        )
        .unwrap();
    assert!(remove.success);
    assert_eq!(remove.output["entry_count"], 0);
}

#[test]
fn test_system_prompt_contains_skill_index() {
    let tmp = tempfile::TempDir::new().unwrap();
    let skills_dir = tmp.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    let skill_manager = SkillManager::new(vec![skills_dir], HashSet::new());
    skill_manager.ensure_bundled_skills().unwrap();

    let available_tools: HashSet<String> = [
        "terminal",
        "read_file",
        "write_file",
        "patch",
        "search_files",
        "skills_list",
        "skill_view",
        "skill_manage",
        "memory",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let index = skill_manager.build_system_prompt_index(&available_tools);

    assert!(!index.is_empty(), "skill index should not be empty");
    assert!(
        index.contains("skill_view"),
        "index should reference skill_view"
    );
}
