use std::path::Path;

use serde::Deserialize;

use crate::error::SkillError;
use crate::types::{Skill, SkillConditions, SkillMeta};

/// Raw frontmatter structure as parsed from YAML
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    platforms: Option<Vec<String>>,
    metadata: Option<RawMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    hermes: Option<RawHermesMeta>,
}

#[derive(Debug, Deserialize)]
struct RawHermesMeta {
    #[serde(default)]
    requires_tools: Vec<String>,
    #[serde(default)]
    fallback_for_tools: Vec<String>,
    #[serde(default)]
    requires_toolsets: Vec<String>,
    #[serde(default)]
    fallback_for_toolsets: Vec<String>,
}

/// Extract YAML frontmatter between `---` markers.
/// Returns (frontmatter_str, body_str) or None if no frontmatter found.
fn extract_frontmatter(content: &str) -> Option<(&str, &str)> {
    let content = content.trim_start_matches('\u{feff}'); // strip BOM if present
    if !content.starts_with("---") {
        return None;
    }
    // Find the closing `---`
    let after_open = &content[3..];
    // Allow optional whitespace/newline after opening ---
    let after_open = after_open.trim_start_matches(['\r', '\n']);
    // Find next occurrence of `---` on its own line
    let close_pos = after_open.find("\n---")?;
    let frontmatter = &after_open[..close_pos];
    let rest = &after_open[close_pos + 4..]; // skip "\n---"
    // Skip optional newline after closing ---
    let body = rest.trim_start_matches(['\r', '\n']);
    Some((frontmatter, body))
}

/// Find linked files in sibling directories relative to the skill directory.
fn find_linked_files(skill_dir: &Path) -> Vec<std::path::PathBuf> {
    let sibling_dirs = ["references", "templates", "scripts", "assets"];
    let mut linked = Vec::new();
    for dir_name in &sibling_dirs {
        let dir = skill_dir.join(dir_name);
        if dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    linked.push(path);
                }
            }
        }
    }
    linked.sort();
    linked
}

/// Parse a SKILL.md file content into a `Skill`.
pub fn parse_skill_file(content: &str, path: &Path) -> Result<Skill, SkillError> {
    let (frontmatter_str, body) = extract_frontmatter(content)
        .ok_or_else(|| SkillError::Parse("No frontmatter found in skill file".to_string()))?;

    let raw: RawFrontmatter = serde_yaml::from_str(frontmatter_str)
        .map_err(|e| SkillError::Parse(format!("Invalid YAML frontmatter: {e}")))?;

    let name = raw
        .name
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SkillError::Parse("Missing required field: name".to_string()))?;

    let description = raw
        .description
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SkillError::Parse("Missing required field: description".to_string()))?;

    let conditions = raw
        .metadata
        .and_then(|m| m.hermes)
        .map(|h| SkillConditions {
            requires_tools: h.requires_tools,
            fallback_for_tools: h.fallback_for_tools,
            requires_toolsets: h.requires_toolsets,
            fallback_for_toolsets: h.fallback_for_toolsets,
        })
        .unwrap_or_default();

    let meta = SkillMeta {
        name,
        description,
        version: raw.version,
        platforms: raw.platforms,
        conditions,
    };

    let skill_dir = path.parent().unwrap_or(path);
    let linked_files = find_linked_files(skill_dir);

    Ok(Skill {
        meta,
        body: body.to_string(),
        path: path.to_path_buf(),
        linked_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frontmatter_basic() {
        let content = "---\nname: foo\n---\nbody text";
        let result = extract_frontmatter(content);
        assert!(result.is_some());
        let (fm, body) = result.unwrap();
        assert!(fm.contains("name: foo"));
        assert_eq!(body, "body text");
    }

    #[test]
    fn test_extract_frontmatter_none() {
        let content = "no frontmatter here";
        assert!(extract_frontmatter(content).is_none());
    }
}
