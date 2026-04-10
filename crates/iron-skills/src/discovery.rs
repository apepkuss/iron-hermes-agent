use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::warn;

use crate::error::SkillError;
use crate::parser::parse_skill_file;
use crate::types::{Skill, SkillSummary};

pub struct SkillDiscovery {
    skills_dirs: Vec<PathBuf>,
    disabled: HashSet<String>,
}

impl SkillDiscovery {
    /// Create a new `SkillDiscovery` instance.
    pub fn new(skills_dirs: Vec<PathBuf>, disabled: HashSet<String>) -> Self {
        Self {
            skills_dirs,
            disabled,
        }
    }

    /// Recursively scan all configured directories for SKILL.md files and parse each.
    pub fn discover_all(&self) -> Vec<Skill> {
        let mut skills = Vec::new();
        for dir in &self.skills_dirs {
            collect_skills(dir, &mut skills);
        }
        skills
    }

    /// Return filtered summaries based on disabled list, current platform, and tool conditions.
    pub fn discover_summaries(&self, available_tools: &HashSet<String>) -> Vec<SkillSummary> {
        self.discover_all()
            .into_iter()
            .filter(|skill| {
                // Skip disabled skills
                if self.disabled.contains(&skill.meta.name) {
                    return false;
                }

                // Platform filter
                if let Some(platforms) = &skill.meta.platforms
                    && !platforms.is_empty()
                    && !platform_matches(platforms)
                {
                    return false;
                }

                // requires_tools: all listed tools must be available
                let conds = &skill.meta.conditions;
                if !conds.requires_tools.is_empty()
                    && !conds
                        .requires_tools
                        .iter()
                        .all(|t| available_tools.contains(t))
                {
                    return false;
                }

                // fallback_for_tools: show only if ANY of the listed tools is NOT available
                if !conds.fallback_for_tools.is_empty() {
                    let any_missing = conds
                        .fallback_for_tools
                        .iter()
                        .any(|t| !available_tools.contains(t));
                    if !any_missing {
                        return false;
                    }
                }

                true
            })
            .map(|skill| {
                let category = extract_category(&skill.path);
                SkillSummary {
                    name: skill.meta.name,
                    description: skill.meta.description,
                    category,
                }
            })
            .collect()
    }

    /// Find a skill by its directory name (not necessarily the `name` field).
    pub fn load_skill(&self, name: &str) -> Result<Skill, SkillError> {
        for dir in &self.skills_dirs {
            if let Some(skill) = find_skill_in_dir(dir, name) {
                return Ok(skill);
            }
        }
        Err(SkillError::NotFound(name.to_string()))
    }
}

/// Recursively collect all skills under `dir`.
fn collect_skills(dir: &Path, skills: &mut Vec<Skill>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Cannot read skills directory {}: {}", dir.display(), e);
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Check if this directory contains a SKILL.md
            let skill_file = path.join("SKILL.md");
            if skill_file.is_file() {
                match std::fs::read_to_string(&skill_file) {
                    Ok(content) => match parse_skill_file(&content, &skill_file) {
                        Ok(skill) => skills.push(skill),
                        Err(e) => warn!("Failed to parse {}: {}", skill_file.display(), e),
                    },
                    Err(e) => warn!("Failed to read {}: {}", skill_file.display(), e),
                }
            }
            // Recurse regardless to support category subdirectories
            collect_skills(&path, skills);
        }
    }
}

/// Recursively search for a skill directory matching `name` under `dir`.
fn find_skill_in_dir(dir: &Path, name: &str) -> Option<Skill> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name()?.to_string_lossy();
            if dir_name == name {
                let skill_file = path.join("SKILL.md");
                if skill_file.is_file() {
                    let content = std::fs::read_to_string(&skill_file).ok()?;
                    return parse_skill_file(&content, &skill_file).ok();
                }
            }
            // Recurse into subdirectories
            if let Some(skill) = find_skill_in_dir(&path, name) {
                return Some(skill);
            }
        }
    }
    None
}

/// Extract category from path: `.../skills/category/skill-name/SKILL.md` → category.
/// Returns "uncategorized" if the path structure doesn't match.
fn extract_category(path: &Path) -> String {
    // path is `.../category/skill-name/SKILL.md`
    // parent = `.../category/skill-name`
    // parent.parent = `.../category`
    path.parent()
        .and_then(|skill_dir| skill_dir.parent())
        .and_then(|cat_dir| cat_dir.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "uncategorized".to_string())
}

/// Check if the current platform matches any of the listed platforms.
fn platform_matches(platforms: &[String]) -> bool {
    for p in platforms {
        match p.to_lowercase().as_str() {
            "macos" | "darwin" if cfg!(target_os = "macos") => return true,
            "linux" if cfg!(target_os = "linux") => return true,
            "windows" if cfg!(target_os = "windows") => return true,
            _ => {}
        }
    }
    false
}
