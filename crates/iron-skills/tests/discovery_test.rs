use std::collections::HashSet;
use std::fs;
use std::path::Path;

use iron_skills::{SkillDiscovery, SkillError};
use tempfile::TempDir;

/// Helper: write a SKILL.md file under `skills_root/category/skill_name/SKILL.md`.
fn write_skill(skills_root: &Path, category: &str, skill_name: &str, extra_frontmatter: &str) {
    let skill_dir = skills_root.join(category).join(skill_name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    let content = format!(
        "---\nname: {skill_name}\ndescription: Description of {skill_name}\n{extra_frontmatter}---\n# {skill_name}\n\nSkill body.\n"
    );
    fs::write(skill_dir.join("SKILL.md"), content).expect("write SKILL.md");
}

#[test]
fn test_discover_all_skills() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_skill(root, "tools", "skill-alpha", "");
    write_skill(root, "tools", "skill-beta", "");

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], HashSet::new());
    let skills = discovery.discover_all();

    assert_eq!(skills.len(), 2, "expected 2 skills, got {}", skills.len());
    let names: HashSet<String> = skills.iter().map(|s| s.meta.name.clone()).collect();
    assert!(names.contains("skill-alpha"));
    assert!(names.contains("skill-beta"));
}

#[test]
fn test_discover_filters_disabled() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_skill(root, "tools", "skill-one", "");
    write_skill(root, "tools", "skill-two", "");

    let mut disabled = HashSet::new();
    disabled.insert("skill-two".to_string());

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], disabled);
    let available: HashSet<String> = HashSet::new();
    let summaries = discovery.discover_summaries(&available);

    assert_eq!(
        summaries.len(),
        1,
        "expected 1 summary after filtering disabled"
    );
    assert_eq!(summaries[0].name, "skill-one");
}

#[test]
fn test_load_skill_by_name() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_skill(root, "utils", "my-target-skill", "version: \"2.0\"\n");

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], HashSet::new());
    let skill = discovery
        .load_skill("my-target-skill")
        .expect("should load skill by dir name");

    assert_eq!(skill.meta.name, "my-target-skill");
    assert_eq!(skill.meta.description, "Description of my-target-skill");
}

#[test]
fn test_load_skill_not_found() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_skill(root, "utils", "existing-skill", "");

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], HashSet::new());
    let result = discovery.load_skill("non-existent-skill");

    assert!(result.is_err());
    match result.unwrap_err() {
        SkillError::NotFound(name) => {
            assert_eq!(name, "non-existent-skill");
        }
        other => panic!("expected NotFound error, got: {other:?}"),
    }
}

#[test]
fn test_discover_filters_by_requires_tools() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_skill(
        root,
        "tools",
        "git-skill",
        "metadata:\n  hermes:\n    requires_tools:\n      - git\n",
    );
    write_skill(
        root,
        "tools",
        "docker-skill",
        "metadata:\n  hermes:\n    requires_tools:\n      - docker\n",
    );

    let mut available = HashSet::new();
    available.insert("git".to_string());

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], HashSet::new());
    let summaries = discovery.discover_summaries(&available);

    assert_eq!(summaries.len(), 1, "only git-skill should pass filter");
    assert_eq!(summaries[0].name, "git-skill");
}

#[test]
fn test_discover_filters_by_fallback_for_tools() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // fallback-skill should appear only when "special-tool" is NOT available
    write_skill(
        root,
        "tools",
        "fallback-skill",
        "metadata:\n  hermes:\n    fallback_for_tools:\n      - special-tool\n",
    );
    write_skill(root, "tools", "normal-skill", "");

    // When special-tool is available: fallback-skill should be hidden
    let mut available = HashSet::new();
    available.insert("special-tool".to_string());

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], HashSet::new());
    let summaries = discovery.discover_summaries(&available);
    let names: Vec<&str> = summaries.iter().map(|s| s.name.as_str()).collect();
    assert!(
        !names.contains(&"fallback-skill"),
        "fallback-skill should be hidden when special-tool is available"
    );
    assert!(names.contains(&"normal-skill"));

    // When special-tool is NOT available: fallback-skill should appear
    let empty_tools: HashSet<String> = HashSet::new();
    let summaries2 = discovery.discover_summaries(&empty_tools);
    let names2: Vec<&str> = summaries2.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names2.contains(&"fallback-skill"),
        "fallback-skill should appear when special-tool is missing"
    );
}

#[test]
fn test_category_extraction() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_skill(root, "my-category", "categorized-skill", "");

    let discovery = SkillDiscovery::new(vec![root.to_path_buf()], HashSet::new());
    let available: HashSet<String> = HashSet::new();
    let summaries = discovery.discover_summaries(&available);

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].category, "my-category");
}
