use std::path::Path;

use iron_skills::{SkillError, parse_skill_file};

fn dummy_path() -> &'static Path {
    Path::new("/tmp/test-skill/SKILL.md")
}

#[test]
fn test_parse_valid_skill() {
    let content = r#"---
name: my-skill
description: A test skill for verification
version: "1.0.0"
---
# My Skill

This is the skill body.
"#;
    let skill = parse_skill_file(content, dummy_path()).expect("should parse valid skill");
    assert_eq!(skill.meta.name, "my-skill");
    assert_eq!(skill.meta.description, "A test skill for verification");
    assert_eq!(skill.meta.version.as_deref(), Some("1.0.0"));
    assert!(skill.body.contains("This is the skill body."));
}

#[test]
fn test_parse_skill_with_conditions() {
    let content = r#"---
name: conditional-skill
description: Skill with tool conditions
metadata:
  hermes:
    requires_tools:
      - git
      - cargo
    fallback_for_tools:
      - docker
---
Body content here.
"#;
    let skill =
        parse_skill_file(content, dummy_path()).expect("should parse skill with conditions");
    assert_eq!(skill.meta.name, "conditional-skill");
    assert_eq!(skill.meta.conditions.requires_tools, vec!["git", "cargo"]);
    assert_eq!(skill.meta.conditions.fallback_for_tools, vec!["docker"]);
    assert!(skill.meta.conditions.requires_toolsets.is_empty());
    assert!(skill.meta.conditions.fallback_for_toolsets.is_empty());
}

#[test]
fn test_parse_skill_with_platforms() {
    let content = r#"---
name: platform-skill
description: Skill with platform constraints
platforms:
  - macos
  - linux
---
Platform-specific instructions.
"#;
    let skill = parse_skill_file(content, dummy_path()).expect("should parse skill with platforms");
    assert_eq!(skill.meta.name, "platform-skill");
    let platforms = skill.meta.platforms.expect("platforms should be Some");
    assert!(platforms.contains(&"macos".to_string()));
    assert!(platforms.contains(&"linux".to_string()));
}

#[test]
fn test_parse_invalid_frontmatter() {
    let content = "No frontmatter here, just plain text.\n\nSome more content.";
    let result = parse_skill_file(content, dummy_path());
    assert!(result.is_err());
    match result.unwrap_err() {
        SkillError::Parse(msg) => {
            assert!(
                msg.contains("No frontmatter"),
                "expected 'No frontmatter' in: {msg}"
            );
        }
        other => panic!("expected Parse error, got: {other:?}"),
    }
}

#[test]
fn test_parse_missing_name() {
    let content = r#"---
description: A skill without a name
---
Body.
"#;
    let result = parse_skill_file(content, dummy_path());
    assert!(result.is_err());
    match result.unwrap_err() {
        SkillError::Parse(msg) => {
            assert!(msg.contains("name"), "expected 'name' in error: {msg}");
        }
        other => panic!("expected Parse error, got: {other:?}"),
    }
}

#[test]
fn test_parse_missing_description() {
    let content = r#"---
name: no-description-skill
---
Body.
"#;
    let result = parse_skill_file(content, dummy_path());
    assert!(result.is_err());
    match result.unwrap_err() {
        SkillError::Parse(msg) => {
            assert!(
                msg.contains("description"),
                "expected 'description' in error: {msg}"
            );
        }
        other => panic!("expected Parse error, got: {other:?}"),
    }
}
