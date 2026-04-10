use std::path::PathBuf;

use anyhow::Context;
use serde_json::{Value, json};

use crate::store::{DEFAULT_MEMORY_CHAR_LIMIT, DEFAULT_USER_CHAR_LIMIT, MemoryStore};

/// High-level orchestration layer over a [`MemoryStore`].
///
/// `MemoryManager` owns the store, handles initialisation, formats the
/// system-prompt block from frozen snapshots, and routes incoming tool calls
/// to the appropriate store operations.
pub struct MemoryManager {
    store: MemoryStore,
}

impl MemoryManager {
    /// Create a new `MemoryManager`.
    ///
    /// `base_dir` is the directory where `MEMORY.md` and `USER.md` live.
    /// Pass `None` for either limit to use the crate defaults.
    pub fn new(
        base_dir: impl Into<PathBuf>,
        memory_char_limit: Option<usize>,
        user_char_limit: Option<usize>,
    ) -> Self {
        let store = MemoryStore::new(
            base_dir,
            memory_char_limit.unwrap_or(DEFAULT_MEMORY_CHAR_LIMIT),
            user_char_limit.unwrap_or(DEFAULT_USER_CHAR_LIMIT),
        );
        Self { store }
    }

    /// Load entries from disk and freeze session snapshots.
    ///
    /// Must be called once before [`system_prompt_block`] or
    /// [`handle_tool_call`].
    pub fn initialize(&mut self) -> anyhow::Result<()> {
        self.store
            .load_from_disk()
            .context("MemoryManager: failed to load from disk")?;
        Ok(())
    }

    /// Build the Markdown block for the system prompt from frozen snapshots.
    ///
    /// Returns `None` when both snapshots are empty.
    pub fn system_prompt_block(&self) -> Option<String> {
        let memory = self
            .store
            .format_for_system_prompt("memory")
            .unwrap_or_default();
        let user = self
            .store
            .format_for_system_prompt("user")
            .unwrap_or_default();

        if memory.is_empty() && user.is_empty() {
            return None;
        }

        let mut block = String::new();

        if !memory.is_empty() {
            block.push_str("## Agent Memory\n\n");
            block.push_str(&memory);
        }

        if !user.is_empty() {
            if !block.is_empty() {
                block.push_str("\n\n");
            }
            block.push_str("## User Profile\n\n");
            block.push_str(&user);
        }

        Some(block)
    }

    /// Route a tool call to the appropriate store operation.
    ///
    /// # Arguments
    /// * `action`  — `"add"`, `"replace"`, or `"remove"`
    /// * `target`  — `"memory"` or `"user"`
    /// * `content` — new entry text (required for `add` and `replace`)
    /// * `old_text`— substring to match (required for `replace` and `remove`)
    ///
    /// Returns a JSON object with at minimum the fields:
    /// `success`, `message`, `entry_count`, `usage`.
    pub fn handle_tool_call(
        &mut self,
        action: &str,
        target: &str,
        content: Option<&str>,
        old_text: Option<&str>,
    ) -> anyhow::Result<Value> {
        let result = match action {
            "add" => {
                let content = content.context("MemoryManager: 'add' requires 'content'")?;
                self.store
                    .add(target, content)
                    .context("MemoryManager: store.add failed")?
            }
            "replace" => {
                let old = old_text.context("MemoryManager: 'replace' requires 'old_text'")?;
                let new = content.context("MemoryManager: 'replace' requires 'content'")?;
                self.store
                    .replace(target, old, new)
                    .context("MemoryManager: store.replace failed")?
            }
            "remove" => {
                let old = old_text.context("MemoryManager: 'remove' requires 'old_text'")?;
                self.store
                    .remove(target, old)
                    .context("MemoryManager: store.remove failed")?
            }
            other => anyhow::bail!("MemoryManager: unknown action '{other}'"),
        };

        Ok(json!({
            "success":     result.success,
            "message":     result.message,
            "entry_count": result.entry_count,
            "usage":       result.usage,
        }))
    }
}
