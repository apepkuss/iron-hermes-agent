use std::fs;
use std::path::{Path, PathBuf};

use crate::error::MemoryError;
use crate::security::scan_content;

/// Delimiter used between memory entries in the markdown files.
const ENTRY_DELIMITER: &str = "\n§\n";

/// Default character limits.
pub const DEFAULT_MEMORY_CHAR_LIMIT: usize = 2200;
pub const DEFAULT_USER_CHAR_LIMIT: usize = 1375;

/// Result returned by MemoryStore tool operations.
pub struct MemoryToolResult {
    pub success: bool,
    pub message: String,
    pub entry_count: usize,
    pub usage: String, // "X% — Y/Z chars"
}

/// File-backed memory store with frozen session snapshots.
pub struct MemoryStore {
    base_dir: PathBuf,
    memory_entries: Vec<String>,
    user_entries: Vec<String>,
    memory_char_limit: usize,
    user_char_limit: usize,
    /// Frozen at load_from_disk(); used by format_for_system_prompt().
    system_prompt_snapshot: Option<String>,
    /// Frozen at load_from_disk(); used by format_for_system_prompt() for "user".
    user_prompt_snapshot: Option<String>,
}

impl MemoryStore {
    /// Create a new MemoryStore pointing at `base_dir`.
    pub fn new(
        base_dir: impl Into<PathBuf>,
        memory_char_limit: usize,
        user_char_limit: usize,
    ) -> Self {
        Self {
            base_dir: base_dir.into(),
            memory_entries: Vec::new(),
            user_entries: Vec::new(),
            memory_char_limit,
            user_char_limit,
            system_prompt_snapshot: None,
            user_prompt_snapshot: None,
        }
    }

    // ──────────────────────────────────────────────────────────────
    // Persistence helpers
    // ──────────────────────────────────────────────────────────────

    fn memory_path(&self) -> PathBuf {
        self.base_dir.join("MEMORY.md")
    }

    fn user_path(&self) -> PathBuf {
        self.base_dir.join("USER.md")
    }

    /// Parse a file on disk into a `Vec<String>` of entries.
    fn parse_file(path: &Path) -> Result<Vec<String>, MemoryError> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        let entries: Vec<String> = raw
            .split(ENTRY_DELIMITER)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(entries)
    }

    /// Atomically write `entries` to `path` (write tmp, then rename).
    fn persist(path: &Path, entries: &[String]) -> Result<(), MemoryError> {
        let tmp_path = path.with_extension("tmp");
        let content = entries.join(ENTRY_DELIMITER);
        fs::write(&tmp_path, &content)?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    // ──────────────────────────────────────────────────────────────
    // Target helpers
    // ──────────────────────────────────────────────────────────────

    fn validate_target(target: &str) -> Result<(), MemoryError> {
        match target {
            "memory" | "user" => Ok(()),
            other => Err(MemoryError::InvalidTarget(other.to_string())),
        }
    }

    fn entries_mut(&mut self, target: &str) -> &mut Vec<String> {
        if target == "memory" {
            &mut self.memory_entries
        } else {
            &mut self.user_entries
        }
    }

    fn entries(&self, target: &str) -> &Vec<String> {
        if target == "memory" {
            &self.memory_entries
        } else {
            &self.user_entries
        }
    }

    fn char_limit(&self, target: &str) -> usize {
        if target == "memory" {
            self.memory_char_limit
        } else {
            self.user_char_limit
        }
    }

    fn file_path(&self, target: &str) -> PathBuf {
        if target == "memory" {
            self.memory_path()
        } else {
            self.user_path()
        }
    }

    /// Compute total chars used by the entry list (entries + delimiters).
    fn total_chars(entries: &[String]) -> usize {
        if entries.is_empty() {
            return 0;
        }
        let content_chars: usize = entries.iter().map(|e| e.len()).sum();
        let delimiter_chars = ENTRY_DELIMITER.len() * (entries.len().saturating_sub(1));
        content_chars + delimiter_chars
    }

    /// Format usage string: "X% — Y/Z chars"
    fn usage_string(entries: &[String], limit: usize) -> String {
        let used = Self::total_chars(entries);
        let pct = (used * 100).checked_div(limit).unwrap_or(0);
        format!("{pct}% — {used}/{limit} chars")
    }

    // ──────────────────────────────────────────────────────────────
    // Public API
    // ──────────────────────────────────────────────────────────────

    /// Load entries from disk and freeze snapshots for this session.
    pub fn load_from_disk(&mut self) -> Result<(), MemoryError> {
        fs::create_dir_all(&self.base_dir)?;

        self.memory_entries = Self::parse_file(&self.memory_path())?;
        self.user_entries = Self::parse_file(&self.user_path())?;

        // Freeze snapshots — these never change during the session.
        self.system_prompt_snapshot = Some(self.render_entries(&self.memory_entries));
        self.user_prompt_snapshot = Some(self.render_entries(&self.user_entries));

        Ok(())
    }

    fn render_entries(&self, entries: &[String]) -> String {
        entries.join(ENTRY_DELIMITER)
    }

    /// Return the frozen snapshot for the given target.
    /// Panics if `load_from_disk()` has not been called.
    pub fn format_for_system_prompt(&self, target: &str) -> Result<String, MemoryError> {
        Self::validate_target(target)?;
        let snapshot = if target == "memory" {
            self.system_prompt_snapshot.as_deref().unwrap_or("")
        } else {
            self.user_prompt_snapshot.as_deref().unwrap_or("")
        };
        Ok(snapshot.to_string())
    }

    /// Add a new entry to the given target.
    pub fn add(&mut self, target: &str, content: &str) -> Result<MemoryToolResult, MemoryError> {
        Self::validate_target(target)?;

        // Security scan
        if let Some(violation) = scan_content(content) {
            return Ok(MemoryToolResult {
                success: false,
                message: format!("Rejected: security violation — {violation}"),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), self.char_limit(target)),
            });
        }

        let trimmed = content.trim().to_string();

        // Duplicate check — treat as success so the model does not retry.
        if self.entries(target).contains(&trimmed) {
            return Ok(MemoryToolResult {
                success: true,
                message: format!("Entry \"{}\" already exists, no action needed.", trimmed),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), self.char_limit(target)),
            });
        }

        // Char limit check
        let mut new_entries = self.entries(target).clone();
        new_entries.push(trimmed.clone());
        let used = Self::total_chars(&new_entries);
        let limit = self.char_limit(target);
        if used > limit {
            return Ok(MemoryToolResult {
                success: false,
                message: format!("Rejected: char limit exceeded ({used} > {limit})."),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), limit),
            });
        }

        // Persist
        let path = self.file_path(target);
        Self::persist(&path, &new_entries)?;
        *self.entries_mut(target) = new_entries;

        Ok(MemoryToolResult {
            success: true,
            message: format!("Added entry to '{target}'."),
            entry_count: self.entries(target).len(),
            usage: Self::usage_string(self.entries(target), self.char_limit(target)),
        })
    }

    /// Replace an entry (identified by substring match) with new content.
    pub fn replace(
        &mut self,
        target: &str,
        old_text: &str,
        new_content: &str,
    ) -> Result<MemoryToolResult, MemoryError> {
        Self::validate_target(target)?;

        // Security scan new content
        if let Some(violation) = scan_content(new_content) {
            return Ok(MemoryToolResult {
                success: false,
                message: format!("Rejected: security violation — {violation}"),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), self.char_limit(target)),
            });
        }

        let new_trimmed = new_content.trim().to_string();

        let idx = self
            .entries(target)
            .iter()
            .position(|e| e.contains(old_text));

        let Some(idx) = idx else {
            return Ok(MemoryToolResult {
                success: false,
                message: format!("Not found: no entry contains '{old_text}'."),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), self.char_limit(target)),
            });
        };

        let mut new_entries = self.entries(target).clone();
        new_entries[idx] = new_trimmed;

        let used = Self::total_chars(&new_entries);
        let limit = self.char_limit(target);
        if used > limit {
            return Ok(MemoryToolResult {
                success: false,
                message: format!("Rejected: char limit exceeded ({used} > {limit})."),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), limit),
            });
        }

        let path = self.file_path(target);
        Self::persist(&path, &new_entries)?;
        *self.entries_mut(target) = new_entries;

        Ok(MemoryToolResult {
            success: true,
            message: format!("Replaced entry in '{target}'."),
            entry_count: self.entries(target).len(),
            usage: Self::usage_string(self.entries(target), self.char_limit(target)),
        })
    }

    /// Remove an entry identified by substring match.
    pub fn remove(
        &mut self,
        target: &str,
        old_text: &str,
    ) -> Result<MemoryToolResult, MemoryError> {
        Self::validate_target(target)?;

        let idx = self
            .entries(target)
            .iter()
            .position(|e| e.contains(old_text));

        let Some(idx) = idx else {
            return Ok(MemoryToolResult {
                success: false,
                message: format!("Not found: no entry contains '{old_text}'."),
                entry_count: self.entries(target).len(),
                usage: Self::usage_string(self.entries(target), self.char_limit(target)),
            });
        };

        let mut new_entries = self.entries(target).clone();
        new_entries.remove(idx);

        let path = self.file_path(target);
        Self::persist(&path, &new_entries)?;
        *self.entries_mut(target) = new_entries;

        Ok(MemoryToolResult {
            success: true,
            message: format!("Removed entry from '{target}'."),
            entry_count: self.entries(target).len(),
            usage: Self::usage_string(self.entries(target), self.char_limit(target)),
        })
    }
}
