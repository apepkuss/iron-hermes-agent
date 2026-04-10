use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::discovery::SkillDiscovery;
use crate::error::SkillError;
use crate::parser::parse_skill_file;
use crate::security::{check_path_traversal, scan_skill_content, validate_skill_name};
use crate::types::{Skill, SkillSummary};

/// Manages skill lifecycle: create, edit, patch, delete, and system prompt injection.
pub struct SkillManager {
    discovery: SkillDiscovery,
    skills_dirs: Vec<PathBuf>,
}

impl SkillManager {
    /// Create a new `SkillManager`.
    ///
    /// - `skills_dirs`: directories to scan for skills
    /// - `disabled`: set of skill names to exclude from listings
    pub fn new(skills_dirs: Vec<PathBuf>, disabled: HashSet<String>) -> Self {
        let discovery = SkillDiscovery::new(skills_dirs.clone(), disabled);
        Self {
            discovery,
            skills_dirs,
        }
    }

    /// Build the system prompt index string, grouped by category (sorted).
    ///
    /// Only includes skills whose tool conditions match `available_tools`.
    pub fn build_system_prompt_index(&self, available_tools: &HashSet<String>) -> String {
        let summaries = self.discovery.discover_summaries(available_tools);

        // Group by category using BTreeMap for sorted output
        let mut by_category: BTreeMap<String, Vec<SkillSummary>> = BTreeMap::new();
        for summary in summaries {
            by_category
                .entry(summary.category.clone())
                .or_default()
                .push(summary);
        }

        if by_category.is_empty() {
            return String::new();
        }

        let mut output = String::from(
            "## Skills (mandatory)\n\
             Scan the skills below. If one matches your task, load it with skill_view(name) and follow its instructions.\n\n",
        );

        for (category, skills) in &by_category {
            output.push_str(&format!("  {}:\n", category));
            // Sort skills within the category by name for deterministic output
            let mut sorted_skills = skills.clone();
            sorted_skills.sort_by(|a, b| a.name.cmp(&b.name));
            for skill in &sorted_skills {
                output.push_str(&format!("    - {}: {}\n", skill.name, skill.description));
            }
        }

        output
    }

    /// List available skills, optionally filtered by category.
    pub fn list_skills(
        &self,
        category: Option<&str>,
        available_tools: &HashSet<String>,
    ) -> Vec<SkillSummary> {
        let mut summaries = self.discovery.discover_summaries(available_tools);
        if let Some(cat) = category {
            summaries.retain(|s| s.category == cat);
        }
        summaries
    }

    /// Load the full skill by name, with path traversal protection.
    pub fn view_skill(&self, name: &str) -> Result<Skill, SkillError> {
        check_path_traversal(name).map_err(SkillError::SecurityViolation)?;
        self.discovery.load_skill(name)
    }

    /// Create a new skill in the first `skills_dir`.
    ///
    /// - Validates the name
    /// - Scans content for security issues
    /// - Parses the content to ensure it is valid
    /// - Writes atomically to `<skills_dir>/<category>/<name>/SKILL.md`
    pub fn create_skill(
        &self,
        name: &str,
        content: &str,
        category: &str,
    ) -> Result<PathBuf, SkillError> {
        validate_skill_name(name).map_err(SkillError::InvalidName)?;

        if let Some(violation) = scan_skill_content(content) {
            return Err(SkillError::SecurityViolation(violation));
        }

        let primary_dir = self.skills_dirs.first().ok_or_else(|| {
            SkillError::Other(anyhow::anyhow!("no skills directories configured"))
        })?;

        let skill_dir = primary_dir.join(category).join(name);
        let skill_file = skill_dir.join("SKILL.md");

        // Validate that the content can be parsed before writing
        parse_skill_file(content, &skill_file)?;

        // Atomic write: write to a temp file first, then rename
        fs::create_dir_all(&skill_dir)?;
        atomic_write(&skill_file, content)?;

        Ok(skill_file)
    }

    /// Edit an existing skill's content entirely.
    ///
    /// - Scans content for security issues
    /// - Parses the content to ensure it is valid
    /// - Atomically overwrites the skill file
    pub fn edit_skill(&self, name: &str, content: &str) -> Result<PathBuf, SkillError> {
        if let Some(violation) = scan_skill_content(content) {
            return Err(SkillError::SecurityViolation(violation));
        }

        let skill = self.discovery.load_skill(name)?;
        let skill_file = &skill.path;

        // Validate that new content can be parsed
        parse_skill_file(content, skill_file)?;

        // Atomic overwrite
        atomic_write(skill_file, content)?;

        Ok(skill_file.clone())
    }

    /// Apply a find-and-replace patch to a skill's SKILL.md.
    ///
    /// - Finds `old_string` and replaces it with `new_string`
    /// - If `replace_all` is true, replaces all occurrences; otherwise replaces the first
    /// - Scans the resulting content for security issues
    /// - Verifies the patched content still parses correctly
    pub fn patch_skill(
        &self,
        name: &str,
        old_string: &str,
        new_string: &str,
        replace_all: bool,
    ) -> Result<PathBuf, SkillError> {
        let skill = self.discovery.load_skill(name)?;
        let skill_file = &skill.path;

        let original = fs::read_to_string(skill_file)?;

        let patched = if replace_all {
            original.replace(old_string, new_string)
        } else {
            match original.find(old_string) {
                Some(pos) => {
                    let mut result =
                        String::with_capacity(original.len() - old_string.len() + new_string.len());
                    result.push_str(&original[..pos]);
                    result.push_str(new_string);
                    result.push_str(&original[pos + old_string.len()..]);
                    result
                }
                None => {
                    return Err(SkillError::Parse(format!(
                        "old_string not found in skill '{}'",
                        name
                    )));
                }
            }
        };

        // Security scan the result
        if let Some(violation) = scan_skill_content(&patched) {
            return Err(SkillError::SecurityViolation(violation));
        }

        // Verify the patched content still parses
        parse_skill_file(&patched, skill_file)?;

        // Atomic write
        atomic_write(skill_file, &patched)?;

        Ok(skill_file.clone())
    }

    /// Extract bundled skills on first run. Call during initialization.
    pub fn ensure_bundled_skills(&self) -> anyhow::Result<u32> {
        if self.skills_dirs.is_empty() {
            return Ok(0);
        }
        crate::bundled::extract_bundled_skills(&self.skills_dirs[0])
    }

    /// Delete a skill by removing its directory.
    pub fn delete_skill(&self, name: &str) -> Result<(), SkillError> {
        let skill = self.discovery.load_skill(name)?;

        let skill_dir = skill
            .path
            .parent()
            .ok_or_else(|| SkillError::Other(anyhow::anyhow!("skill path has no parent")))?;

        fs::remove_dir_all(skill_dir)?;
        Ok(())
    }
}

/// Write `content` to `path` atomically using a sibling temp file + rename.
fn atomic_write(path: &Path, content: &str) -> Result<(), SkillError> {
    let dir = path
        .parent()
        .ok_or_else(|| SkillError::Other(anyhow::anyhow!("path has no parent directory")))?;

    // Write to a temp file in the same directory, then rename for atomicity
    let tmp_path = dir.join(format!(
        ".skill_tmp_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    ));

    fs::write(&tmp_path, content).map_err(SkillError::Io)?;

    fs::rename(&tmp_path, path).map_err(|e| {
        // Clean up temp file on failure
        let _ = fs::remove_file(&tmp_path);
        SkillError::Io(e)
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_skill_content(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n\nSkill body.\n",)
    }

    fn write_skill(root: &Path, category: &str, name: &str, description: &str) {
        let skill_dir = root.join(category).join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        let content = make_skill_content(name, description);
        fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn test_build_system_prompt_index() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        write_skill(&root, "tools", "my-skill", "Does something useful");

        let manager = SkillManager::new(vec![root], HashSet::new());
        let index = manager.build_system_prompt_index(&HashSet::new());

        assert!(
            index.contains("my-skill"),
            "index should contain skill name"
        );
        assert!(
            index.contains("Does something useful"),
            "index should contain description"
        );
        assert!(index.contains("tools"), "index should contain category");
    }

    #[test]
    fn test_list_skills() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        write_skill(&root, "tools", "list-skill", "A listable skill");

        let manager = SkillManager::new(vec![root], HashSet::new());
        let skills = manager.list_skills(None, &HashSet::new());

        assert!(!skills.is_empty(), "should have at least one skill");
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"list-skill"));
    }

    #[test]
    fn test_view_skill() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        write_skill(&root, "tools", "view-skill", "A viewable skill");

        let manager = SkillManager::new(vec![root], HashSet::new());
        let skill = manager.view_skill("view-skill").unwrap();

        assert_eq!(skill.meta.name, "view-skill");
        assert_eq!(skill.meta.description, "A viewable skill");
    }

    #[test]
    fn test_create_skill() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        let manager = SkillManager::new(vec![root.clone()], HashSet::new());
        let content = make_skill_content("new-skill", "A newly created skill");
        manager
            .create_skill("new-skill", &content, "tools")
            .unwrap();

        let skill = manager.view_skill("new-skill").unwrap();
        assert_eq!(skill.meta.name, "new-skill");
        assert_eq!(skill.meta.description, "A newly created skill");
    }

    #[test]
    fn test_delete_skill() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();

        write_skill(&root, "tools", "delete-me", "To be deleted");

        let manager = SkillManager::new(vec![root], HashSet::new());

        // Verify it exists first
        assert!(manager.view_skill("delete-me").is_ok());

        manager.delete_skill("delete-me").unwrap();

        // Verify it no longer exists
        let result = manager.view_skill("delete-me");
        assert!(result.is_err(), "skill should not be found after deletion");
        matches!(result.unwrap_err(), SkillError::NotFound(_));
    }
}
