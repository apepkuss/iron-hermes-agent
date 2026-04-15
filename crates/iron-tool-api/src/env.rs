//! Safe environment variable filtering for tool execution.
//!
//! Provides whitelist-based filtering to prevent sensitive information
//! (API keys, tokens, passwords) from leaking into tool subprocesses.

use std::collections::HashMap;

/// Env var prefixes considered safe to pass into subprocesses.
pub const SAFE_PREFIXES: &[&str] = &[
    "PATH", "HOME", "USER", "LANG", "LC_", "TERM", "TMPDIR", "TZ", "SHELL",
];

/// Substrings that indicate a secret env var — block these.
pub const SECRET_PATTERNS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "CREDENTIAL",
    "PASSWD",
    "AUTH",
];

/// Returns `true` if the given environment variable name is safe to expose.
pub fn is_safe_env_var(name: &str) -> bool {
    let upper = name.to_uppercase();
    // If it contains a secret pattern, block it.
    for pat in SECRET_PATTERNS {
        if upper.contains(pat) {
            return false;
        }
    }
    // Allow if it starts with a safe prefix.
    for prefix in SAFE_PREFIXES {
        if upper.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Collect all safe environment variables from the current process.
pub fn collect_safe_env() -> HashMap<String, String> {
    std::env::vars()
        .filter(|(name, _)| is_safe_env_var(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_vars_allowed() {
        assert!(is_safe_env_var("PATH"));
        assert!(is_safe_env_var("HOME"));
        assert!(is_safe_env_var("USER"));
        assert!(is_safe_env_var("LANG"));
        assert!(is_safe_env_var("LC_ALL"));
        assert!(is_safe_env_var("TERM"));
        assert!(is_safe_env_var("TMPDIR"));
        assert!(is_safe_env_var("TZ"));
        assert!(is_safe_env_var("SHELL"));
    }

    #[test]
    fn secret_vars_blocked() {
        assert!(!is_safe_env_var("API_KEY"));
        assert!(!is_safe_env_var("LLM_API_KEY"));
        assert!(!is_safe_env_var("GITHUB_TOKEN"));
        assert!(!is_safe_env_var("SECRET_VALUE"));
        assert!(!is_safe_env_var("DB_PASSWORD"));
        assert!(!is_safe_env_var("CREDENTIAL_FILE"));
        assert!(!is_safe_env_var("PASSWD"));
        assert!(!is_safe_env_var("AUTH_HEADER"));
    }

    #[test]
    fn unknown_vars_blocked() {
        assert!(!is_safe_env_var("CUSTOM_VAR"));
        assert!(!is_safe_env_var("MY_SETTING"));
    }

    #[test]
    fn collect_safe_env_returns_only_safe() {
        let safe = collect_safe_env();
        for key in safe.keys() {
            assert!(is_safe_env_var(key), "{key} should be safe");
        }
    }
}
