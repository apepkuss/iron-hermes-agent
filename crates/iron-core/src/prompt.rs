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
/// Only injected for models that need explicit tool-use steering.
/// Adapted from hermes-agent's TOOL_USE_ENFORCEMENT_GUIDANCE.
const TOOL_USE_ENFORCEMENT_GUIDANCE: &str = "\
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
without acting are not acceptable.";

/// Google Gemini/Gemma specific operational guidance.
/// Adapted from hermes-agent's GOOGLE_MODEL_OPERATIONAL_GUIDANCE.
const GOOGLE_MODEL_OPERATIONAL_GUIDANCE: &str = "\
# Google model operational directives\n\
Follow these operational rules strictly:\n\
- **Absolute paths:** Always construct and use absolute file paths for all \
file system operations. Combine the project root with relative paths.\n\
- **Verify first:** Use read_file/search_files to check file contents and \
project structure before making changes. Never guess at file contents.\n\
- **Dependency checks:** Never assume a library is available. Check \
package.json, requirements.txt, Cargo.toml, etc. before importing.\n\
- **Conciseness:** Keep explanatory text brief — a few sentences, not \
paragraphs. Focus on actions and results over narration.\n\
- **Parallel tool calls:** When you need to perform multiple independent \
operations (e.g. reading several files), make all the tool calls in a \
single response rather than sequentially.\n\
- **Non-interactive commands:** Use flags like -y, --yes, --non-interactive \
to prevent CLI tools from hanging on prompts.\n\
- **Keep going:** Work autonomously until the task is fully resolved. \
Don't stop with a plan — execute it.";

/// Response format guidance injected for all models.
/// Ensures tool results are presented as human-readable text.
const RESPONSE_FORMAT_GUIDANCE: &str = "\
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

/// Model name substrings that trigger tool-use enforcement guidance.
/// Only these model families need explicit tool-use steering.
/// Models like Qwen, Llama, Hermes have good native function calling and
/// don't need (and may be harmed by) aggressive tool-use enforcement.
const TOOL_USE_ENFORCEMENT_MODELS: &[&str] = &["gpt", "codex", "gemini", "gemma", "grok"];

/// Model name substrings that trigger Google-specific operational guidance.
const GOOGLE_MODELS: &[&str] = &["gemini", "gemma"];

/// Check if a model name matches any pattern in the list (case-insensitive).
fn model_matches(model_name: &str, patterns: &[&str]) -> bool {
    let lower = model_name.to_lowercase();
    patterns.iter().any(|p| lower.contains(p))
}

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
    /// 2. Tool-use enforcement (only for models in [`TOOL_USE_ENFORCEMENT_MODELS`])
    /// 3. Google operational guidance (only for Gemini/Gemma)
    /// 4. Response format guidance (always)
    /// 5. Memory guidance (always)
    /// 6. Skills guidance (always)
    /// 7. Custom system message (if set)
    /// 8. Memory block (if set)
    /// 9. Skills index (if set)
    /// 10. Context files (each entry, in order)
    /// 11. Metadata (date, session_id, model_name)
    pub fn build(ctx: &PromptContext) -> String {
        let mut sections: Vec<&str> = Vec::new();

        // 1. Identity
        let identity = ctx.identity.as_deref().unwrap_or(DEFAULT_IDENTITY);
        sections.push(identity);

        // 2. Tool-use enforcement (conditional)
        if model_matches(&ctx.model_name, TOOL_USE_ENFORCEMENT_MODELS) {
            sections.push(TOOL_USE_ENFORCEMENT_GUIDANCE);
        }

        // 3. Google operational guidance (conditional)
        if model_matches(&ctx.model_name, GOOGLE_MODELS) {
            sections.push(GOOGLE_MODEL_OPERATIONAL_GUIDANCE);
        }

        // 4. Response format guidance (always)
        sections.push(RESPONSE_FORMAT_GUIDANCE);

        // 5. Memory guidance (always)
        sections.push(MEMORY_GUIDANCE);

        // 6. Skills guidance (always)
        sections.push(SKILLS_GUIDANCE);

        // 7. Custom system message
        if let Some(ref msg) = ctx.custom_system_message {
            sections.push(msg.as_str());
        }

        // 8. Memory block
        if let Some(ref block) = ctx.memory_block {
            sections.push(block.as_str());
        }

        // 9. Skills index
        if let Some(ref index) = ctx.skills_index {
            sections.push(index.as_str());
        }

        // 10. Context files
        for file in &ctx.context_files {
            sections.push(file.as_str());
        }

        // 11. Metadata
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
