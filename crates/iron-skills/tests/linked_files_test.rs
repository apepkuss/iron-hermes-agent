use std::collections::HashSet;
use std::fs;
use std::path::Path;

use iron_skills::manager::SkillManager;

fn make_skill_content(name: &str, description: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nSkill body.\n"
    )
}

fn write_skill(root: &Path, category: &str, name: &str) {
    let skill_dir = root.join(category).join(name);
    fs::create_dir_all(&skill_dir).unwrap();
    let content = make_skill_content(name, "test skill");
    fs::write(skill_dir.join("SKILL.md"), content).unwrap();
}

#[test]
fn test_write_linked_file_creates_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    write_skill(&root, "tools", "my-skill");

    let manager = SkillManager::new(vec![root.clone()], HashSet::new());
    let result = manager
        .write_linked_file("my-skill", "references/api.md", "# API Reference\n")
        .unwrap();

    assert!(result.exists());
    let content = fs::read_to_string(&result).unwrap();
    assert_eq!(content, "# API Reference\n");
}

#[test]
fn test_write_linked_file_rejects_disallowed_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    write_skill(&root, "tools", "my-skill");

    let manager = SkillManager::new(vec![root], HashSet::new());
    let result = manager.write_linked_file("my-skill", "forbidden/file.txt", "content");
    assert!(result.is_err());
}

#[test]
fn test_write_linked_file_rejects_path_traversal() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    write_skill(&root, "tools", "my-skill");

    let manager = SkillManager::new(vec![root], HashSet::new());
    let result = manager.write_linked_file("my-skill", "references/../../etc/passwd", "evil");
    assert!(result.is_err());
}

#[test]
fn test_remove_linked_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    write_skill(&root, "tools", "my-skill");

    let manager = SkillManager::new(vec![root.clone()], HashSet::new());
    manager
        .write_linked_file("my-skill", "scripts/run.sh", "#!/bin/bash\necho hi")
        .unwrap();

    manager.remove_linked_file("my-skill", "scripts/run.sh").unwrap();

    let file_path = root.join("tools/my-skill/scripts/run.sh");
    assert!(!file_path.exists());
}

#[test]
fn test_remove_linked_file_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    write_skill(&root, "tools", "my-skill");

    let manager = SkillManager::new(vec![root], HashSet::new());
    let result = manager.remove_linked_file("my-skill", "scripts/nonexistent.sh");
    assert!(result.is_err());
}
