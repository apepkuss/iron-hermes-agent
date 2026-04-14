use std::sync::Arc;

use serde_json::Value;
use tracing::{debug, error, warn};

use crate::budget::IterationBudget;
use crate::context_compressor::{CompressorConfig, ContextCompressor};
use crate::error::CoreError;
use crate::event::{AgentEvent, EventCallback, build_args_preview, truncate_preview};
use crate::llm::client::LlmClient;
use crate::llm::types::Message;
use crate::prompt::{PromptBuilder, PromptContext};
use crate::session::types::TokenUsage;

use iron_memory::manager::MemoryManager;
use iron_skills::manager::SkillManager;
use iron_tools::registry::ToolRegistry;
use iron_tools::types::ToolContext;
use tokio::sync::Mutex;

// ─── Configuration ───

/// Configuration for the [`Agent`].
pub struct AgentConfig {
    /// Maximum number of LLM round-trips per `chat()` call. Default: 90.
    pub max_iterations: u32,
    /// Optional SOUL.md content used as the model identity.
    pub identity: Option<String>,
    /// Additional context files (e.g. AGENTS.md) injected into the system prompt.
    pub context_files: Vec<String>,
    /// Model name for metadata in the system prompt.
    pub model_name: String,
    /// Optional context compressor configuration. When set, compression is enabled.
    pub compressor_config: Option<CompressorConfig>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 90,
            identity: None,
            context_files: Vec::new(),
            model_name: String::from("unknown"),
            compressor_config: None,
        }
    }
}

// ─── Chat status & response ───

/// Outcome of a single `chat()` invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatStatus {
    Completed,
    Interrupted,
    BudgetExhausted,
    Partial,
    Failed,
}

/// The result returned by [`Agent::chat`].
pub struct AgentResponse {
    pub content: String,
    pub status: ChatStatus,
    pub usage: TokenUsage,
    pub tool_calls_made: u32,
}

// ─── Session state ───

/// In-memory session state threaded through the agent loop.
pub struct SessionState {
    pub session_id: String,
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
}

impl SessionState {
    /// Create a new, empty session state.
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            messages: Vec::new(),
            system_prompt: None,
        }
    }
}

// ─── Agent ───

/// The Agent orchestrates the LLM call loop, tool dispatch, and budget tracking.
pub struct Agent {
    llm_client: LlmClient,
    tool_registry: Arc<ToolRegistry>,
    memory_manager: Arc<Mutex<MemoryManager>>,
    skill_manager: Arc<SkillManager>,
    config: AgentConfig,
    context_compressor: Option<ContextCompressor>,
    session: SessionState,
}

impl Agent {
    /// Create a new `Agent`.
    pub fn new(
        llm_client: LlmClient,
        tool_registry: Arc<ToolRegistry>,
        memory_manager: Arc<Mutex<MemoryManager>>,
        skill_manager: Arc<SkillManager>,
        config: AgentConfig,
    ) -> Self {
        let context_compressor = config
            .compressor_config
            .as_ref()
            .map(|c| ContextCompressor::new(c.clone()));
        Self {
            llm_client,
            tool_registry,
            memory_manager,
            skill_manager,
            config,
            context_compressor,
            session: SessionState::new(String::new()),
        }
    }

    /// Return a shared reference to the internal session.
    pub fn session(&self) -> &SessionState {
        &self.session
    }

    /// Return a mutable reference to the internal session.
    pub fn session_mut(&mut self) -> &mut SessionState {
        &mut self.session
    }

    /// Set the session ID.
    pub fn set_session_id(&mut self, id: String) {
        self.session.session_id = id;
    }

    /// Load conversation history into the session, clearing any cached system prompt.
    pub fn load_history(&mut self, messages: Vec<Message>) {
        self.session.messages = messages;
        self.session.system_prompt = None;
    }

    /// Run a single user turn through the agent loop.
    ///
    /// This method appends the user message to the session, builds the system
    /// prompt (if needed), and enters the iterative LLM → tool → LLM loop
    /// until the model produces a final text response or the budget is
    /// exhausted.
    pub async fn chat(
        &mut self,
        user_message: String,
        event_callback: Option<EventCallback>,
    ) -> Result<AgentResponse, CoreError> {
        // 1. Build system prompt if not already set.
        if self.session.system_prompt.is_none() {
            let prompt = self.build_system_prompt();
            debug!(
                "System prompt built ({} chars), identity: {:?}",
                prompt.len(),
                self.config
                    .identity
                    .as_deref()
                    .map(|s| &s[..s.len().min(80)])
            );
            self.session.system_prompt = Some(prompt);
        }

        // 2. Append user message.
        self.session.messages.push(Message {
            role: "user".to_string(),
            content: Some(user_message),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        });

        // 3. Create iteration budget.
        let budget = IterationBudget::new(self.config.max_iterations);

        let mut total_usage = TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        };
        let mut tool_calls_made: u32 = 0;
        let mut last_content = String::new();
        let mut consecutive_invalid_tool_retries: u32 = 0;
        const MAX_INVALID_TOOL_RETRIES: u32 = 3;

        // 4. Main loop.
        while budget.consume() {
            // a. Build messages for the API call.
            let mut api_messages = Vec::new();

            // System message.
            if let Some(ref sys) = self.session.system_prompt {
                api_messages.push(Message {
                    role: "system".to_string(),
                    content: Some(sys.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }

            // Conversation messages.
            api_messages.extend(self.session.messages.clone());

            // Inject budget warning as a system message if applicable.
            if let Some(warning) = budget.budget_warning() {
                api_messages.push(Message {
                    role: "system".to_string(),
                    content: Some(warning),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                });
            }

            // b. Get tool schemas.
            let tool_names = self.tool_registry.tool_names();
            let tool_ctx = ToolContext {
                task_id: self.session.session_id.clone(),
                working_dir: std::env::current_dir().unwrap_or_default(),
                enabled_tools: tool_names.clone(),
            };
            let schemas = self.tool_registry.get_schemas(&tool_ctx);
            let tools_param = if schemas.is_empty() {
                None
            } else {
                Some(schemas)
            };

            // c. Call LLM (with retry for transient errors).
            let chat_result = self
                .call_llm_with_retry(&api_messages, &tools_param, &event_callback)
                .await;

            let response = match chat_result {
                Ok(r) => r,
                Err(e) => {
                    error!("LLM call failed after retries: {e}");
                    return Ok(AgentResponse {
                        content: format!("LLM call failed: {e}"),
                        status: ChatStatus::Failed,
                        usage: total_usage,
                        tool_calls_made,
                    });
                }
            };

            // Accumulate usage.
            if let Some(ref usage) = response.usage {
                total_usage.prompt_tokens += usage.prompt_tokens;
                total_usage.completion_tokens += usage.completion_tokens;
                total_usage.total_tokens += usage.total_tokens;
            }

            // Context compression check.
            if let Some(ref mut compressor) = self.context_compressor
                && let Some(ref usage) = response.usage
                && compressor.should_compress(usage.prompt_tokens as u64)
            {
                debug!(
                    "Context compression triggered at {} prompt tokens",
                    usage.prompt_tokens
                );
                self.session.messages = compressor
                    .compress(&self.session.messages, usage.prompt_tokens as u64)
                    .await;
                self.session.system_prompt = Some(self.build_system_prompt());
            }

            // d. Parse response — extract assistant message.
            let choice = response
                .choices
                .first()
                .ok_or_else(|| CoreError::LlmRequest("Empty choices in response".to_string()))?;

            let assistant_content = choice.message.content.clone().unwrap_or_default();
            let tool_calls = choice.message.tool_calls.clone();

            // Build the assistant message for session history.
            // Always include content (even empty string) — some LLM providers
            // reject messages with missing content field.
            let assistant_msg = Message {
                role: "assistant".to_string(),
                content: Some(assistant_content.clone()),
                tool_calls: tool_calls.clone(),
                tool_call_id: None,
                name: None,
            };
            self.session.messages.push(assistant_msg);

            last_content = assistant_content;

            // e. If no tool calls — we're done.
            let calls = match tool_calls {
                Some(ref tc) if !tc.is_empty() => tc.clone(),
                _ => break,
            };

            // f. Validate tool calls.
            let mut has_invalid = false;
            for tc in &calls {
                if !self.tool_registry.has_tool(&tc.function.name) {
                    warn!("Invalid tool call: {}", tc.function.name);
                    has_invalid = true;
                    self.session.messages.push(Message {
                        role: "tool".to_string(),
                        content: Some(format!(
                            "Error: tool '{}' not found. Available tools: {}",
                            tc.function.name,
                            tool_names.iter().cloned().collect::<Vec<_>>().join(", ")
                        )),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                        name: Some(tc.function.name.clone()),
                    });
                }
            }

            if has_invalid {
                consecutive_invalid_tool_retries += 1;
                if consecutive_invalid_tool_retries >= MAX_INVALID_TOOL_RETRIES {
                    return Ok(AgentResponse {
                        content: "Agent failed: too many invalid tool calls in a row.".to_string(),
                        status: ChatStatus::Failed,
                        usage: total_usage,
                        tool_calls_made,
                    });
                }
                continue;
            }

            // Reset invalid retries on valid tool calls.
            consecutive_invalid_tool_retries = 0;

            // g. Execute tools sequentially.
            for tc in &calls {
                let args: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(Value::Object(serde_json::Map::new()));

                // Emit ToolStarted event
                if let Some(ref cb) = event_callback {
                    let preview = build_args_preview(&tc.function.arguments);
                    cb(AgentEvent::ToolStarted {
                        tool: tc.function.name.clone(),
                        args_preview: preview,
                        call_id: tc.id.clone(),
                    });
                }

                debug!("Dispatching tool: {} with args: {}", tc.function.name, args);

                let start = std::time::Instant::now();
                let result = self
                    .tool_registry
                    .dispatch_sync(&tc.function.name, args, &tool_ctx);
                let duration = start.elapsed();

                // Emit ToolCompleted event
                if let Some(ref cb) = event_callback {
                    let (success, preview) = match &result {
                        Ok(tr) => {
                            let output_str = tr.output.to_string();
                            let size = output_str.len();
                            let size_str = if size >= 1024 {
                                format!("{:.1}KB", size as f64 / 1024.0)
                            } else {
                                format!("{}B", size)
                            };
                            (tr.success, format!("returned {size_str}"))
                        }
                        Err(e) => (false, truncate_preview(&e.to_string(), 100)),
                    };
                    cb(AgentEvent::ToolCompleted {
                        tool: tc.function.name.clone(),
                        call_id: tc.id.clone(),
                        duration_ms: duration.as_millis() as u64,
                        success,
                        result_preview: preview,
                    });
                }

                let result_text = match result {
                    Ok(tr) => serde_json::to_string(&tr).unwrap_or_else(|_| {
                        format!(
                            "{{\"success\":{},\"output\":\"serialization error\"}}",
                            tr.success
                        )
                    }),
                    Err(e) => {
                        format!("{{\"success\":false,\"output\":\"{}\"}}", e)
                    }
                };

                // Layer 1+2: Truncate oversized tool results.
                let result_text = truncate_tool_result(&result_text, &tc.function.name, &tc.id);

                self.session.messages.push(Message {
                    role: "tool".to_string(),
                    content: Some(result_text),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                    name: Some(tc.function.name.clone()),
                });

                tool_calls_made += 1;
            }

            // h. Post-tool — loop continues.
        }

        // 5. Determine final status.
        let status = if budget.remaining() == 0 {
            ChatStatus::BudgetExhausted
        } else {
            ChatStatus::Completed
        };

        Ok(AgentResponse {
            content: last_content,
            status,
            usage: total_usage,
            tool_calls_made,
        })
    }

    // ─── Private helpers ───

    /// Build the system prompt using [`PromptBuilder`].
    fn build_system_prompt(&self) -> String {
        let memory_block = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mgr = self.memory_manager.lock().await;
                mgr.system_prompt_block()
            })
        });
        let available_tools = self.tool_registry.tool_names();
        let skills_index = self
            .skill_manager
            .build_system_prompt_index(&available_tools);

        let ctx = PromptContext {
            identity: self.config.identity.clone(),
            memory_block,
            skills_index: if skills_index.is_empty() {
                None
            } else {
                Some(skills_index)
            },
            context_files: self.config.context_files.clone(),
            custom_system_message: None,
            model_name: self.config.model_name.clone(),
            session_id: self.session.session_id.clone(),
            current_date: chrono_today(),
            available_tools: available_tools.clone(),
        };

        PromptBuilder::build(&ctx)
    }

    /// Call the LLM with up to 3 retries for transient (5xx / timeout) errors.
    async fn call_llm_with_retry(
        &self,
        messages: &[Message],
        tools: &Option<Vec<Value>>,
        event_callback: &Option<EventCallback>,
    ) -> Result<crate::llm::types::ChatResponse, CoreError> {
        const MAX_RETRIES: u32 = 3;
        let mut last_err: Option<CoreError> = None;

        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt - 1));
                tokio::time::sleep(delay).await;
                debug!("Retrying LLM call (attempt {})", attempt + 1);
            }

            let result = if event_callback.is_some() {
                self.call_llm_streaming(messages, tools, event_callback)
                    .await
            } else {
                self.llm_client.chat(messages.to_vec(), tools.clone()).await
            };

            match result {
                Ok(resp) => return Ok(resp),
                Err(ref e) if Self::is_retryable(e) && attempt < MAX_RETRIES => {
                    warn!("Transient LLM error (attempt {}): {e}", attempt + 1);
                    last_err = Some(result.unwrap_err());
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_err.unwrap_or_else(|| CoreError::LlmRequest("Unknown retry failure".to_string())))
    }

    /// Call LLM in streaming mode and reassemble into a [`ChatResponse`].
    async fn call_llm_streaming(
        &self,
        messages: &[Message],
        tools: &Option<Vec<Value>>,
        event_callback: &Option<EventCallback>,
    ) -> Result<crate::llm::types::ChatResponse, CoreError> {
        use crate::llm::types::{
            ChatResponse, Choice, FunctionCall, ResponseMessage, ToolCall, Usage,
        };

        let mut rx = self
            .llm_client
            .chat_stream(messages.to_vec(), tools.clone())
            .await?;

        let mut content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut finish_reason: Option<String> = None;
        let mut usage: Option<Usage> = None;
        let mut response_id = String::new();
        let mut model = String::new();

        // Tool call assembly state: index -> (id, type, name, arguments)
        let mut tc_state: std::collections::HashMap<u32, (String, String, String, String)> =
            std::collections::HashMap::new();

        // Timeout for the entire streaming response (5 minutes).
        let stream_timeout = std::time::Duration::from_secs(300);
        let stream_deadline = tokio::time::Instant::now() + stream_timeout;

        loop {
            let chunk_result = match tokio::time::timeout_at(stream_deadline, rx.recv()).await {
                Ok(Some(result)) => result,
                Ok(None) => break, // channel closed, stream done
                Err(_) => {
                    warn!(
                        "LLM streaming response timed out after {}s",
                        stream_timeout.as_secs()
                    );
                    break;
                }
            };
            let chunk = chunk_result?;

            if response_id.is_empty() {
                response_id = chunk.id.clone();
            }
            if model.is_empty()
                && let Some(ref m) = chunk.model
            {
                model = m.clone();
            }

            if let Some(u) = chunk.usage {
                usage = Some(u);
            }

            for choice in &chunk.choices {
                if let Some(ref reason) = choice.finish_reason {
                    finish_reason = Some(reason.clone());
                }

                if let Some(ref delta_content) = choice.delta.content {
                    content.push_str(delta_content);
                    if !delta_content.is_empty()
                        && let Some(cb) = event_callback
                    {
                        cb(AgentEvent::TextDelta {
                            content: delta_content.clone(),
                        });
                    }
                }

                if let Some(ref delta_tcs) = choice.delta.tool_calls {
                    for dtc in delta_tcs {
                        let entry = tc_state.entry(dtc.index).or_insert_with(|| {
                            (String::new(), String::new(), String::new(), String::new())
                        });

                        if let Some(ref id) = dtc.id {
                            entry.0 = id.clone();
                        }
                        if let Some(ref t) = dtc.r#type {
                            entry.1 = t.clone();
                        }
                        if let Some(ref f) = dtc.function {
                            if let Some(ref name) = f.name {
                                entry.2 = name.clone();
                            }
                            if let Some(ref args) = f.arguments {
                                entry.3.push_str(args);
                            }
                        }
                    }
                }
            }
        }

        // Assemble tool calls from state.
        let mut indices: Vec<u32> = tc_state.keys().copied().collect();
        indices.sort();
        for idx in indices {
            let (id, typ, name, arguments) = tc_state.remove(&idx).unwrap();
            tool_calls.push(ToolCall {
                id,
                r#type: if typ.is_empty() {
                    "function".to_string()
                } else {
                    typ
                },
                function: FunctionCall { name, arguments },
            });
        }

        let response = ChatResponse {
            id: response_id,
            object: "chat.completion".to_string(),
            model,
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".to_string(),
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                },
                finish_reason,
            }],
            usage,
        };

        Ok(response)
    }

    /// Check whether an error is transient and worth retrying.
    fn is_retryable(err: &CoreError) -> bool {
        match err {
            CoreError::LlmApi { status, .. } => *status >= 500 || *status == 429,
            CoreError::LlmRequest(_) => true, // network / timeout errors
            _ => false,
        }
    }
}

/// Return today's date in ISO-8601 format.
fn chrono_today() -> String {
    // Use a simple approach: read from system time.
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Approximate date calculation (good enough for metadata).
    let days = secs / 86400;
    let (y, m, d) = epoch_days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

// ─── Tool result size control (Layer 1 + Layer 2) ───

/// Maximum tool result size in characters (Layer 1).
const MAX_TOOL_RESULT_CHARS: usize = 100_000;

/// Preview size for truncated results (Layer 2).
const TOOL_RESULT_PREVIEW_CHARS: usize = 1_500;

/// Truncate a tool result if it exceeds the per-tool limit.
///
/// Layer 1: Check against MAX_TOOL_RESULT_CHARS.
/// Layer 2: If exceeded, return a preview (first TOOL_RESULT_PREVIEW_CHARS) with
/// a truncation notice, so the LLM knows the result was shortened.
fn truncate_tool_result(result: &str, tool_name: &str, tool_call_id: &str) -> String {
    if result.len() <= MAX_TOOL_RESULT_CHARS {
        return result.to_string();
    }

    let total_bytes = result.len();
    let total_kb = total_bytes / 1024;

    // Take the preview, truncating at the last newline to avoid broken JSON/text.
    let preview_end = result[..TOOL_RESULT_PREVIEW_CHARS]
        .rfind('\n')
        .unwrap_or(TOOL_RESULT_PREVIEW_CHARS);
    let preview = &result[..preview_end];

    warn!(
        "Tool result from '{}' (call {}) truncated: {} KB -> {} char preview",
        tool_name, tool_call_id, total_kb, preview_end
    );

    format!(
        "{preview}\n\n[... Result truncated: {total_kb} KB total. \
         Showing first {preview_end} chars. Use read_file or web_extract \
         for targeted retrieval if you need more detail.]"
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
