/// Default identity used when no custom identity is provided.
const DEFAULT_IDENTITY: &str = "You are a helpful AI assistant with access to tools. Use the tools available to you to help the user accomplish their tasks.";

/// Guidance injected into every prompt to instruct the model on memory usage.
const MEMORY_GUIDANCE: &str = "You have persistent memory across sessions. Save durable facts using the memory tool: user preferences, environment details, tool quirks, and stable conventions. Memory is injected into every turn, so keep it compact and focused on facts that will still matter later. Prioritize what reduces future user steering. Do NOT save task progress, session outcomes, or temporary state to memory.";

/// Guidance injected into every prompt to instruct the model on skill usage.
const SKILLS_GUIDANCE: &str = "After completing a complex task (5+ tool calls), fixing a tricky error, or discovering a non-trivial workflow, save the approach as a skill with skill_manage so you can reuse it next time. When using a skill and finding it outdated, incomplete, or wrong, patch it immediately with skill_manage(action='patch').";

/// Context passed to [`PromptBuilder::build`] to assemble the system prompt.
pub struct PromptContext {
    /// SOUL.md content or `None` to fall back to [`DEFAULT_IDENTITY`].
    pub identity: Option<String>,
    /// Frozen memory snapshot from `MemoryManager`.
    pub memory_block: Option<String>,
    /// Skills index from `SkillManager`.
    pub skills_index: Option<String>,
    /// Additional context files such as AGENTS.md.
    pub context_files: Vec<String>,
    /// Optional custom system message from the user or configuration.
    pub custom_system_message: Option<String>,
    /// Name of the model being used (included in metadata).
    pub model_name: String,
    /// Current session identifier (included in metadata).
    pub session_id: String,
    /// Current date in ISO-8601 format (included in metadata).
    pub current_date: String,
}

/// Assembles the system prompt from multiple layers.
pub struct PromptBuilder;

impl PromptBuilder {
    /// Build the full system prompt from the provided [`PromptContext`].
    ///
    /// Sections are assembled in this order, each non-empty section separated
    /// by `"\n\n"`:
    ///
    /// 1. Identity (custom or [`DEFAULT_IDENTITY`])
    /// 2. Memory guidance ([`MEMORY_GUIDANCE`], always included)
    /// 3. Skills guidance ([`SKILLS_GUIDANCE`], always included)
    /// 4. Custom system message (if set)
    /// 5. Memory block (if set)
    /// 6. Skills index (if set)
    /// 7. Context files (each entry, in order)
    /// 8. Metadata (date, session_id, model_name)
    pub fn build(ctx: &PromptContext) -> String {
        let mut sections: Vec<&str> = Vec::new();

        // 1. Identity
        let identity = ctx.identity.as_deref().unwrap_or(DEFAULT_IDENTITY);
        sections.push(identity);

        // 2. Memory guidance (always)
        sections.push(MEMORY_GUIDANCE);

        // 3. Skills guidance (always)
        sections.push(SKILLS_GUIDANCE);

        // 4. Custom system message
        if let Some(ref msg) = ctx.custom_system_message {
            sections.push(msg.as_str());
        }

        // 5. Memory block
        if let Some(ref block) = ctx.memory_block {
            sections.push(block.as_str());
        }

        // 6. Skills index
        if let Some(ref index) = ctx.skills_index {
            sections.push(index.as_str());
        }

        // 7. Context files
        for file in &ctx.context_files {
            sections.push(file.as_str());
        }

        // 8. Metadata
        let metadata = format!(
            "Date: {}\nSession: {}\nModel: {}",
            ctx.current_date, ctx.session_id, ctx.model_name,
        );

        // Join all collected sections, then append metadata
        let mut output = sections.join("\n\n");
        output.push_str("\n\n");
        output.push_str(&metadata);

        output
    }
}
