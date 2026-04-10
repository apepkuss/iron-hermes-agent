use async_trait::async_trait;

/// Trait representing a pluggable memory backend.
///
/// Implementors provide session-scoped memory capabilities: loading state at
/// the start of a session, contributing to the system prompt, prefetching
/// relevant context per turn, syncing conversation turns, handling tool calls,
/// and cleanly shutting down.
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    /// Human-readable name of this provider (e.g. `"iron-memory"`).
    fn name(&self) -> &str;

    /// Returns `true` if the provider is ready to be used.
    fn is_available(&self) -> bool;

    /// Called once at the start of a session to initialise state.
    async fn initialize(&mut self, session_id: &str) -> anyhow::Result<()>;

    /// Returns a Markdown block to inject into the system prompt, or `None`
    /// if this provider has nothing to contribute.
    fn system_prompt_block(&self) -> Option<String>;

    /// Prefetch memory entries relevant to `query`.  Returns formatted text
    /// or `None` if nothing is relevant.
    async fn prefetch(&self, query: &str) -> Option<String>;

    /// Persist one conversation turn (user + assistant messages).
    async fn sync_turn(&self, user_msg: &str, assistant_msg: &str) -> anyhow::Result<()>;

    /// Dispatch a named tool call with JSON arguments; returns a JSON result.
    async fn handle_tool_call(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value>;

    /// Called when the session ends; flush buffers, close handles, etc.
    async fn shutdown(&self) -> anyhow::Result<()>;
}
