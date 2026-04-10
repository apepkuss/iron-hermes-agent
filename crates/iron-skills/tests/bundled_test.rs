#[test]
fn test_extract_bundled_skills() {
    let dir = tempfile::TempDir::new().unwrap();
    let count = iron_skills::bundled::extract_bundled_skills(dir.path()).unwrap();
    assert!(
        count > 0,
        "Should extract at least some skills, got {count}"
    );

    // Verify at least one SKILL.md exists
    let has_skill = walkdir(dir.path());
    assert!(has_skill, "Should have at least one SKILL.md");
}

fn walkdir(dir: &std::path::Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().map(|n| n == "SKILL.md").unwrap_or(false) {
                return true;
            }
            if path.is_dir() && walkdir(&path) {
                return true;
            }
        }
    }
    false
}
