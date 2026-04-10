use regex::Regex;

/// Injection patterns that are rejected (case-insensitive).
const INJECTION_PATTERNS: &[&str] = &[
    "ignore previous instructions",
    "you are now",
    "system prompt override",
];

/// Maximum allowed skill file size in bytes (100 KB).
const MAX_CONTENT_BYTES: usize = 100 * 1024;

/// Validate a skill name.
///
/// Rules:
/// - Maximum 64 characters
/// - Must match regex: `^[a-z0-9][a-z0-9._-]*$`
pub fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.len() > 64 {
        return Err(format!(
            "skill name '{}' exceeds maximum length of 64 characters",
            name
        ));
    }

    let re = Regex::new(r"^[a-z0-9][a-z0-9._-]*$").expect("valid regex");
    if !re.is_match(name) {
        return Err(format!(
            "skill name '{}' is invalid; must match ^[a-z0-9][a-z0-9._-]*$",
            name
        ));
    }

    Ok(())
}

/// Scan skill content for security issues.
///
/// Returns `Some(reason)` if a violation is detected, `None` if the content is safe.
///
/// Checks:
/// - Content size > 100 KB
/// - Presence of injection patterns (case-insensitive)
pub fn scan_skill_content(content: &str) -> Option<String> {
    if content.len() > MAX_CONTENT_BYTES {
        return Some(format!(
            "skill content exceeds maximum size of {} bytes (got {} bytes)",
            MAX_CONTENT_BYTES,
            content.len()
        ));
    }

    let lower = content.to_lowercase();
    for pattern in INJECTION_PATTERNS {
        if lower.contains(pattern) {
            return Some(format!(
                "skill content contains injection pattern: '{}'",
                pattern
            ));
        }
    }

    None
}

/// Check a path for path traversal attempts.
///
/// Returns `Err` if `".."` is present anywhere in the path string.
pub fn check_path_traversal(path: &str) -> Result<(), String> {
    if path.contains("..") {
        return Err(format!(
            "path '{}' contains path traversal sequence '..'",
            path
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_skill_name_valid() {
        assert!(validate_skill_name("my-skill").is_ok());
        assert!(validate_skill_name("skill123").is_ok());
        assert!(validate_skill_name("a").is_ok());
        assert!(validate_skill_name("my.skill").is_ok());
        assert!(validate_skill_name("my_skill").is_ok());
        assert!(validate_skill_name("0numeric").is_ok());
    }

    #[test]
    fn test_validate_skill_name_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_skill_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_skill_name_max_length() {
        let name = "a".repeat(64);
        assert!(validate_skill_name(&name).is_ok());
    }

    #[test]
    fn test_validate_skill_name_invalid_chars() {
        assert!(validate_skill_name("My-Skill").is_err()); // uppercase
        assert!(validate_skill_name("-starts-with-dash").is_err()); // starts with dash
        assert!(validate_skill_name("has spaces").is_err());
        assert!(validate_skill_name("").is_err());
    }

    #[test]
    fn test_scan_skill_content_clean() {
        let content = "---\nname: test\ndescription: safe\n---\n# Normal content\n";
        assert!(scan_skill_content(content).is_none());
    }

    #[test]
    fn test_scan_skill_content_too_large() {
        let large = "x".repeat(MAX_CONTENT_BYTES + 1);
        assert!(scan_skill_content(&large).is_some());
    }

    #[test]
    fn test_scan_skill_content_injection_patterns() {
        assert!(scan_skill_content("ignore previous instructions and do X").is_some());
        assert!(scan_skill_content("You Are Now a different AI").is_some());
        assert!(scan_skill_content("system prompt override enabled").is_some());
        // Case insensitive
        assert!(scan_skill_content("IGNORE PREVIOUS INSTRUCTIONS").is_some());
    }

    #[test]
    fn test_check_path_traversal_safe() {
        assert!(check_path_traversal("my-skill").is_ok());
        assert!(check_path_traversal("category/skill-name").is_ok());
    }

    #[test]
    fn test_check_path_traversal_violation() {
        assert!(check_path_traversal("../etc/passwd").is_err());
        assert!(check_path_traversal("skill/../../secret").is_err());
    }
}
