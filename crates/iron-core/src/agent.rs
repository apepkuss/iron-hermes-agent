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
use crate::session::SessionEnvironment;
use crate::session::types::TokenUsage;
use crate::todo::{TodoEventReceiver, TodoState};

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
    /// Only enable tools from these toolsets. Empty = all enabled.
    pub enabled_toolsets: Vec<String>,
    /// Disable tools from these toolsets. Applied after enabled_toolsets filter.
    pub disabled_toolsets: Vec<String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 90,
            identity: None,
            context_files: Vec::new(),
            model_name: String::from("unknown"),
            compressor_config: None,
            enabled_toolsets: Vec::new(),
            disabled_toolsets: Vec::new(),
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
    todo_event_rx: Option<TodoEventReceiver>,
    todo_state: Option<TodoState>,
    /// Per-session terminal environment (working dir + safe env vars).
    environment: SessionEnvironment,
}

impl Agent {
    /// Create a new `Agent`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        llm_client: LlmClient,
        tool_registry: Arc<ToolRegistry>,
        memory_manager: Arc<Mutex<MemoryManager>>,
        skill_manager: Arc<SkillManager>,
        config: AgentConfig,
        todo_event_rx: Option<TodoEventReceiver>,
        todo_state: Option<TodoState>,
        environment: SessionEnvironment,
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
            todo_event_rx,
            todo_state,
            environment,
        }
    }

    /// Return a shared reference to the agent config.
    pub fn config(&self) -> &AgentConfig {
        &self.config
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

            // Guard rail: sanitize messages before LLM call.
            // Removes orphaned tool results and injects stubs for missing results.
            Self::sanitize_messages(&mut api_messages);

            // Budget warning is injected into the last tool result (see post-tool section),
            // not as a separate system message — this preserves prompt cache.

            // b. Get tool schemas (filtered by enabled/disabled toolsets).
            let tool_names = self.filtered_tool_names();
            let tool_ctx = ToolContext {
                task_id: self.session.session_id.clone(),
                working_dir: self.environment.working_dir.clone(),
                enabled_tools: tool_names.clone(),
                env_vars: self.environment.env_vars.clone(),
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

            // e. If no tool calls — mark remaining todo items as completed and break.
            let calls = match tool_calls {
                Some(ref tc) if !tc.is_empty() => tc.clone(),
                _ => {
                    self.finalize_todo(&event_callback);
                    break;
                }
            };

            // Guard rail: deduplicate tool calls (same name + args in one turn).
            let calls = Self::deduplicate_tool_calls(calls);

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

                let is_todo = tc.function.name == "todo";

                // Emit ToolStarted event (skip for todo — it has its own TodoUpdate event)
                if !is_todo && let Some(ref cb) = event_callback {
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

                if let Some(ref cb) = event_callback {
                    // Emit ToolCompleted event (skip for todo)
                    if !is_todo {
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

                    // Relay any pending TodoUpdate events from the todo tool handler.
                    if let Some(ref rx) = self.todo_event_rx {
                        let rx = rx.lock().unwrap();
                        while let Ok(todos) = rx.try_recv() {
                            cb(AgentEvent::TodoUpdate { todos });
                        }
                    }
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

            // h. Inject budget warning into the last tool result if applicable.
            //    This is the hermes-agent pattern: inject into tool result JSON
            //    rather than adding a separate system message, preserving prompt cache.
            if let Some(warning) = budget.budget_warning()
                && let Some(last_tool_msg) = self
                    .session
                    .messages
                    .iter_mut()
                    .rev()
                    .find(|m| m.role == "tool")
                && let Some(ref content) = last_tool_msg.content
            {
                // Try to inject as a JSON field
                if let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(content) {
                    if let Some(obj) = parsed.as_object_mut() {
                        obj.insert(
                            "_budget_warning".to_string(),
                            serde_json::Value::String(warning),
                        );
                        last_tool_msg.content =
                            Some(serde_json::to_string(&parsed).unwrap_or_default());
                    }
                } else {
                    // Not JSON — append as text
                    last_tool_msg.content = Some(format!("{content}\n\n{warning}"));
                }
            }

            // i. Post-tool — loop continues.
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

    /// Mark all non-completed todo items as completed and send a final TodoUpdate event.
    fn finalize_todo(&self, event_callback: &Option<EventCallback>) {
        let (todo_state, cb) = match (&self.todo_state, event_callback) {
            (Some(state), Some(cb)) => (state, cb),
            _ => return,
        };

        let mut map = todo_state.lock().unwrap();
        let session_id = &self.session.session_id;
        if let Some(todos) = map.get_mut(session_id) {
            let mut changed = false;
            for item in todos.iter_mut() {
                if item.status != "completed" {
                    item.status = "completed".to_string();
                    changed = true;
                }
            }
            if changed {
                cb(AgentEvent::TodoUpdate {
                    todos: todos.clone(),
                });
            }
        }
    }

    /// Return tool names filtered by enabled/disabled toolsets.
    fn filtered_tool_names(&self) -> std::collections::HashSet<String> {
        self.tool_registry
            .tool_names()
            .into_iter()
            .filter(|name| {
                let toolset = self.tool_registry.toolset_of(name).unwrap_or("");
                if !self.config.enabled_toolsets.is_empty()
                    && !self.config.enabled_toolsets.iter().any(|s| s == toolset)
                {
                    return false;
                }
                if self.config.disabled_toolsets.iter().any(|s| s == toolset) {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Build the system prompt using [`PromptBuilder`].
    fn build_system_prompt(&self) -> String {
        let memory_block = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mgr = self.memory_manager.lock().await;
                mgr.system_prompt_block()
            })
        });
        let available_tools = self.filtered_tool_names();
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

    // ─── Guard Rails ───

    /// Sanitize messages before LLM call: remove orphaned tool results,
    /// inject stubs for missing tool results, drop invalid roles.
    fn sanitize_messages(messages: &mut Vec<Message>) {
        // Collect all tool_call IDs from assistant messages
        let mut call_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for msg in messages.iter() {
            if msg.role == "assistant"
                && let Some(ref calls) = msg.tool_calls
            {
                for tc in calls {
                    call_ids.insert(tc.id.clone());
                }
            }
        }

        // Collect all tool_call_ids referenced by tool results
        let mut result_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for msg in messages.iter() {
            if msg.role == "tool"
                && let Some(ref id) = msg.tool_call_id
            {
                result_ids.insert(id.clone());
            }
        }

        // 1. Remove orphaned tool results (no matching assistant call)
        let orphaned = result_ids
            .difference(&call_ids)
            .cloned()
            .collect::<Vec<_>>();
        if !orphaned.is_empty() {
            let orphan_set: std::collections::HashSet<String> = orphaned.into_iter().collect();
            messages.retain(|m| {
                !(m.role == "tool"
                    && m.tool_call_id
                        .as_ref()
                        .is_some_and(|id| orphan_set.contains(id)))
            });
            debug!(
                "Guard rail: removed {} orphaned tool result(s)",
                orphan_set.len()
            );
        }

        // 2. Inject stub results for calls with no matching result
        let missing = call_ids
            .difference(&result_ids)
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            let missing_set: std::collections::HashSet<String> = missing.into_iter().collect();
            let mut patched = Vec::with_capacity(messages.len() + missing_set.len());
            for msg in messages.drain(..) {
                let inject_after = if msg.role == "assistant" {
                    msg.tool_calls.as_ref().map_or(vec![], |calls| {
                        calls
                            .iter()
                            .filter(|tc| missing_set.contains(&tc.id))
                            .map(|tc| (tc.id.clone(), tc.function.name.clone()))
                            .collect()
                    })
                } else {
                    vec![]
                };
                patched.push(msg);
                for (cid, name) in inject_after {
                    patched.push(Message {
                        role: "tool".to_string(),
                        content: Some(
                            "[Result unavailable — see context summary above]".to_string(),
                        ),
                        tool_calls: None,
                        tool_call_id: Some(cid),
                        name: Some(name),
                    });
                }
            }
            debug!(
                "Guard rail: injected {} stub tool result(s)",
                missing_set.len()
            );
            *messages = patched;
        }

        // 3. Drop messages with invalid roles
        let valid_roles = ["system", "user", "assistant", "tool"];
        messages.retain(|m| {
            if valid_roles.contains(&m.role.as_str()) {
                true
            } else {
                debug!("Guard rail: dropped message with invalid role '{}'", m.role);
                false
            }
        });
    }

    /// Deduplicate tool calls: remove calls with identical (name, arguments).
    fn deduplicate_tool_calls(
        calls: Vec<crate::llm::types::ToolCall>,
    ) -> Vec<crate::llm::types::ToolCall> {
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        let mut unique = Vec::with_capacity(calls.len());
        for tc in calls {
            let key = (tc.function.name.clone(), tc.function.arguments.clone());
            if seen.insert(key) {
                unique.push(tc);
            } else {
                debug!(
                    "Guard rail: removed duplicate tool call '{}'",
                    tc.function.name
                );
            }
        }
        unique
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

// ─── Tool result size control (Layer 1 + Layer 2 + Layer 3) ───

/// Maximum tool result size in characters (Layer 1).
const MAX_TOOL_RESULT_CHARS: usize = 100_000;

/// Preview size for truncated results (Layer 2).
const TOOL_RESULT_PREVIEW_CHARS: usize = 1_500;

/// Directory for persisted oversized tool results (Layer 3).
const TOOL_RESULT_PERSIST_DIR: &str = "/tmp/iron-hermes-results";

/// Truncate a tool result if it exceeds the per-tool limit.
///
/// Layer 1: Check against MAX_TOOL_RESULT_CHARS — pass through if within limit.
/// Layer 2: Preview (first TOOL_RESULT_PREVIEW_CHARS chars).
/// Layer 3: Persist full result to file so LLM can retrieve it with read_file.
fn truncate_tool_result(result: &str, tool_name: &str, tool_call_id: &str) -> String {
    if result.len() <= MAX_TOOL_RESULT_CHARS {
        return result.to_string();
    }

    let total_bytes = result.len();
    let total_kb = total_bytes / 1024;

    // Layer 3: persist full result to file
    let file_path = persist_tool_result(result, tool_call_id);

    // Layer 2: take preview, truncating at last newline to avoid broken JSON/text
    let preview_end = result[..TOOL_RESULT_PREVIEW_CHARS]
        .rfind('\n')
        .unwrap_or(TOOL_RESULT_PREVIEW_CHARS);
    let preview = &result[..preview_end];

    warn!(
        "Tool result from '{}' (call {}) truncated: {} KB -> {} char preview, full saved to {}",
        tool_name,
        tool_call_id,
        total_kb,
        preview_end,
        file_path.as_deref().unwrap_or("(failed)")
    );

    match file_path {
        Some(path) => format!(
            "{preview}\n\n[... Result truncated: {total_kb} KB total. \
             Full output saved to: {path}\n\
             Use the read_file tool to retrieve the full content if needed.]"
        ),
        None => format!(
            "{preview}\n\n[... Result truncated: {total_kb} KB total. \
             Showing first {preview_end} chars. Use read_file or web_extract \
             for targeted retrieval if you need more detail.]"
        ),
    }
}

/// Persist a tool result to a file in TOOL_RESULT_PERSIST_DIR.
/// Returns the file path on success, or None on failure.
fn persist_tool_result(content: &str, tool_call_id: &str) -> Option<String> {
    let dir = std::path::Path::new(TOOL_RESULT_PERSIST_DIR);
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!("Failed to create tool result persist dir: {e}");
        return None;
    }

    let filename = format!("{tool_call_id}.txt");
    let path = dir.join(&filename);

    match std::fs::write(&path, content) {
        Ok(()) => {
            debug!(
                "Persisted tool result ({} bytes) to {}",
                content.len(),
                path.display()
            );
            Some(path.to_string_lossy().to_string())
        }
        Err(e) => {
            warn!("Failed to persist tool result: {e}");
            None
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{FunctionCall, ToolCall};

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    fn assistant_with_calls(call_ids: &[&str]) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(String::new()),
            tool_calls: Some(
                call_ids
                    .iter()
                    .map(|id| ToolCall {
                        id: id.to_string(),
                        r#type: "function".to_string(),
                        function: FunctionCall {
                            name: "test_tool".to_string(),
                            arguments: "{}".to_string(),
                        },
                    })
                    .collect(),
            ),
            tool_call_id: None,
            name: None,
        }
    }

    fn tool_result(call_id: &str) -> Message {
        Message {
            role: "tool".to_string(),
            content: Some("result".to_string()),
            tool_calls: None,
            tool_call_id: Some(call_id.to_string()),
            name: Some("test_tool".to_string()),
        }
    }

    // ── sanitize_messages tests ──

    #[test]
    fn test_sanitize_removes_orphan_tool_result() {
        let mut messages = vec![
            msg("user", "hello"),
            tool_result("orphan_call"), // no matching assistant
            msg("assistant", "hi"),
        ];
        Agent::sanitize_messages(&mut messages);
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|m| m.role != "tool"));
    }

    #[test]
    fn test_sanitize_injects_stub_for_missing_result() {
        let mut messages = vec![
            msg("user", "hello"),
            assistant_with_calls(&["call_1"]),
            // no tool result for call_1
            msg("user", "next"),
        ];
        Agent::sanitize_messages(&mut messages);
        // Should have injected a stub
        let tool_msgs: Vec<_> = messages.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tool_msgs.len(), 1);
        assert_eq!(tool_msgs[0].tool_call_id.as_deref(), Some("call_1"));
        assert!(
            tool_msgs[0]
                .content
                .as_deref()
                .unwrap()
                .contains("unavailable")
        );
    }

    #[test]
    fn test_sanitize_keeps_valid_pairs() {
        let mut messages = vec![
            msg("user", "hello"),
            assistant_with_calls(&["call_1"]),
            tool_result("call_1"),
            msg("assistant", "done"),
        ];
        let orig_len = messages.len();
        Agent::sanitize_messages(&mut messages);
        assert_eq!(messages.len(), orig_len);
    }

    #[test]
    fn test_sanitize_drops_invalid_role() {
        let mut messages = vec![
            msg("user", "hello"),
            msg("invalid_role", "bad message"),
            msg("assistant", "hi"),
        ];
        Agent::sanitize_messages(&mut messages);
        assert_eq!(messages.len(), 2);
        assert!(messages.iter().all(|m| m.role != "invalid_role"));
    }

    // ── deduplicate_tool_calls tests ──

    #[test]
    fn test_dedup_removes_duplicate_calls() {
        let calls = vec![
            ToolCall {
                id: "c1".to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "/tmp/a.txt"}"#.to_string(),
                },
            },
            ToolCall {
                id: "c2".to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "/tmp/a.txt"}"#.to_string(),
                },
            },
        ];
        let result = Agent::deduplicate_tool_calls(calls);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "c1");
    }

    #[test]
    fn test_dedup_keeps_different_args() {
        let calls = vec![
            ToolCall {
                id: "c1".to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "/tmp/a.txt"}"#.to_string(),
                },
            },
            ToolCall {
                id: "c2".to_string(),
                r#type: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path": "/tmp/b.txt"}"#.to_string(),
                },
            },
        ];
        let result = Agent::deduplicate_tool_calls(calls);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_dedup_no_duplicates_returns_same() {
        let calls = vec![ToolCall {
            id: "c1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        }];
        let result = Agent::deduplicate_tool_calls(calls);
        assert_eq!(result.len(), 1);
    }
}

#[cfg(test)]
mod tool_result_tests {
    use super::*;

    #[test]
    fn test_small_result_passes_through() {
        let result = "small result";
        let output = truncate_tool_result(result, "test_tool", "call_1");
        assert_eq!(output, result);
    }

    #[test]
    fn test_large_result_truncated_with_preview() {
        let result = "x".repeat(MAX_TOOL_RESULT_CHARS + 1000);
        let output = truncate_tool_result(&result, "test_tool", "call_2");

        // Should be much shorter than original
        assert!(output.len() < result.len());
        // Should contain truncation notice
        assert!(output.contains("Result truncated"));
        // Should mention file path
        assert!(output.contains("/tmp/iron-hermes-results/"));
        assert!(output.contains("call_2"));
    }

    #[test]
    fn test_large_result_persisted_to_file() {
        let result = "y".repeat(MAX_TOOL_RESULT_CHARS + 500);
        let _ = truncate_tool_result(&result, "test_tool", "call_persist_test");

        // File should exist
        let path = format!("{}/call_persist_test.txt", TOOL_RESULT_PERSIST_DIR);
        assert!(
            std::path::Path::new(&path).exists(),
            "Persisted file should exist at {path}"
        );

        // File content should match original
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved, result);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_truncation_notice_includes_read_file_hint() {
        let result = "z".repeat(MAX_TOOL_RESULT_CHARS + 100);
        let output = truncate_tool_result(&result, "test_tool", "call_hint_test");

        assert!(output.contains("read_file"));
        // Cleanup
        let path = format!("{}/call_hint_test.txt", TOOL_RESULT_PERSIST_DIR);
        let _ = std::fs::remove_file(&path);
    }
}
