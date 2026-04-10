/// Default identity used when no custom identity is provided.
/// Adapted from hermes-agent's DEFAULT_AGENT_IDENTITY.
const DEFAULT_IDENTITY: &str = "\
You are Iron Hermes, an intelligent AI assistant. \
You are helpful, knowledgeable, and direct. You assist users with a wide \
range of tasks including answering questions, writing and editing code, \
analyzing information, creative work, and executing actions via your tools. \
You communicate clearly, admit uncertainty when appropriate, and prioritize \
being genuinely useful over being verbose unless otherwise directed below. \
Be targeted and efficient in your exploration and investigations.";

/// Guidance on tool usage enforcement.
/// Adapted from hermes-agent's TOOL_USE_ENFORCEMENT_GUIDANCE.
const TOOL_USE_GUIDANCE: &str = "\
# Tool-use enforcement\n\
You MUST use your tools to take action — do not describe what you would do \
or plan to do without actually doing it. When you say you will perform an \
action (e.g. 'I will run the tests', 'Let me check the file', 'I will create \
the project'), you MUST immediately make the corresponding tool call in the same \
response. Never end your turn with a promise of future action — execute it now.\n\
Keep working until the task is actually complete. Do not stop with a summary of \
what you plan to do next time. If you have tools available that can accomplish \
the task, use them instead of telling the user what you would do.\n\
Every response should either (a) contain tool calls that make progress, or \
(b) deliver a final result to the user. Responses that only describe intentions \
without acting are not acceptable.\n\
When you receive tool results, synthesize them into a clear, natural-language \
response for the user. Never output raw JSON or tool result data directly. \
Summarize, explain, and present the information in a readable format.";

/// Guidance injected into every prompt to instruct the model on memory usage.
/// Adapted from hermes-agent's MEMORY_GUIDANCE.
const MEMORY_GUIDANCE: &str = "\
You have persistent memory across sessions. Save durable facts using the memory \
tool: user preferences, environment details, tool quirks, and stable conventions. \
Memory is injected into every turn, so keep it compact and focused on facts that \
will still matter later.\n\
Prioritize what reduces future user steering — the most valuable memory is one \
that prevents the user from having to correct or remind you again. \
User preferences and recurring corrections matter more than procedural task details.\n\
Do NOT save task progress, session outcomes, completed-work logs, or temporary TODO \
state to memory. If you've discovered a new way to do something, solved a problem \
that could be necessary later, save it as a skill with the skill tool.";

/// Guidance injected into every prompt to instruct the model on skill usage.
/// Adapted from hermes-agent's SKILLS_GUIDANCE.
const SKILLS_GUIDANCE: &str = "\
After completing a complex task (5+ tool calls), fixing a tricky error, \
or discovering a non-trivial workflow, save the approach as a \
skill with skill_manage so you can reuse it next time.\n\
When using a skill and finding it outdated, incomplete, or wrong, \
patch it immediately with skill_manage(action='patch') — don't wait to be asked. \
Skills that aren't maintained become liabilities.";

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
    /// 2. Tool-use guidance ([`TOOL_USE_GUIDANCE`], always included)
    /// 3. Memory guidance ([`MEMORY_GUIDANCE`], always included)
    /// 4. Skills guidance ([`SKILLS_GUIDANCE`], always included)
    /// 5. Custom system message (if set)
    /// 6. Memory block (if set)
    /// 7. Skills index (if set)
    /// 8. Context files (each entry, in order)
    /// 9. Metadata (date, session_id, model_name)
    pub fn build(ctx: &PromptContext) -> String {
        let mut sections: Vec<&str> = Vec::new();

        // 1. Identity
        let identity = ctx.identity.as_deref().unwrap_or(DEFAULT_IDENTITY);
        sections.push(identity);

        // 2. Tool-use guidance (always)
        sections.push(TOOL_USE_GUIDANCE);

        // 3. Memory guidance (always)
        sections.push(MEMORY_GUIDANCE);

        // 4. Skills guidance (always)
        sections.push(SKILLS_GUIDANCE);

        // 5. Custom system message
        if let Some(ref msg) = ctx.custom_system_message {
            sections.push(msg.as_str());
        }

        // 6. Memory block
        if let Some(ref block) = ctx.memory_block {
            sections.push(block.as_str());
        }

        // 7. Skills index
        if let Some(ref index) = ctx.skills_index {
            sections.push(index.as_str());
        }

        // 8. Context files
        for file in &ctx.context_files {
            sections.push(file.as_str());
        }

        // 9. Metadata
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
