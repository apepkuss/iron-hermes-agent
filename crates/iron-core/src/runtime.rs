use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{Mutex, RwLock};
use tracing::debug;
use uuid::Uuid;

use crate::agent::{Agent, AgentConfig, AgentResponse};
use crate::context_compressor::CompressorConfig;
use crate::error::CoreError;
use crate::event::EventCallback;
use crate::llm::client::{LlmClient, LlmConfig};
use crate::llm::types::Message;
use crate::session::SessionEnvironment;
use crate::todo::{TodoSenders, TodoState, create_todo_channel};

use iron_memory::manager::MemoryManager;
use iron_skills::manager::SkillManager;
use iron_tools::registry::ToolRegistry;

// ─── SessionSource ───

/// 标识消息来源的平台和上下文
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionSource {
    pub platform: String,
    pub chat_id: String,
    pub user_id: String,
    pub thread_id: Option<String>,
}

/// 由 SessionSource 确定性生成 session key
pub fn build_session_key(source: &SessionSource) -> String {
    match &source.thread_id {
        Some(tid) => format!(
            "{}:{}:{}:{}",
            source.platform, source.chat_id, source.user_id, tid
        ),
        None => format!("{}:{}:{}", source.platform, source.chat_id, source.user_id),
    }
}

/// Session 元数据
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub session_id: String,
    pub session_key: String,
    pub source: SessionSource,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub message_count: u32,
    /// Last model used in this session (for detecting model switches).
    pub last_model: Option<String>,
    /// Turns since last background review (memory/skill). Resets after review.
    pub turns_since_review: u32,
    /// Per-session terminal environment (working dir + safe env vars).
    pub environment: SessionEnvironment,
}

// ─── RuntimeConfig ───

/// Configuration for [`AgentRuntime`].
pub struct RuntimeConfig {
    /// Maximum wall-clock seconds for a single `chat()` call.
    pub agent_timeout_secs: u64,
    /// Seconds of inactivity before a running slot is considered stale.
    pub inactivity_timeout_secs: u64,
    /// Seconds of idle before a session is eligible for eviction.
    pub session_idle_timeout_secs: u64,
    /// Optional override model; when `Some`, replaces `AgentConfig::model_name`.
    pub fallback_model: Option<String>,
    /// LLM base URL.
    pub llm_base_url: String,
    /// LLM API key.
    pub llm_api_key: Option<String>,
    /// LLM model name.
    pub llm_model: String,
    /// Number of user turns between background reviews (0 = disabled). Default: 10.
    pub review_interval: u32,
    /// Default working directory for new sessions.
    /// Supports `~` expansion. If `None`, uses the process CWD.
    pub default_working_dir: Option<String>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            agent_timeout_secs: 600,
            inactivity_timeout_secs: 300,
            session_idle_timeout_secs: 1800,
            fallback_model: None,
            llm_base_url: String::from("http://localhost:11434"),
            llm_api_key: None,
            llm_model: String::from("gpt-4o"),
            review_interval: 10,
            default_working_dir: None,
        }
    }
}

// ─── RunningState ───

/// Tracks whether an agent slot is pending or actively running.
pub enum RunningState {
    Pending,
    Running(Instant),
}

// ─── CachedAgent ───

/// An agent kept in the runtime cache between turns.
pub struct CachedAgent {
    pub agent: Agent,
    pub config_signature: String,
    pub last_used: Instant,
}

// ─── ActivityTracker ───

pub struct ActivityTracker {
    pub last_activity: Instant,
}

// ─── helpers ───

/// Compute a stable string signature for an [`AgentConfig`].
///
/// Two configs that yield the same signature can share a cached agent.
pub fn compute_config_signature(config: &AgentConfig) -> String {
    let compressor_part = match &config.compressor_config {
        None => String::from("none"),
        Some(CompressorConfig {
            context_length,
            threshold,
            target_ratio,
            ..
        }) => format!("comp:{context_length}:{threshold}:{target_ratio}"),
    };
    format!("{}|{}", config.model_name, compressor_part)
}

// ─── AgentRuntime ───

/// Central runtime that manages sessions, running state, and cached agents.
pub struct AgentRuntime {
    config: RwLock<RuntimeConfig>,
    /// session_key → SessionEntry
    sessions: RwLock<HashMap<String, SessionEntry>>,
    /// session_key → RunningState  (pub so tests can inject state)
    pub running: RwLock<HashMap<String, RunningState>>,
    /// session_key → ActivityTracker
    activity: RwLock<HashMap<String, ActivityTracker>>,
    /// session_key → CachedAgent
    agents: Mutex<HashMap<String, CachedAgent>>,

    tool_registry: Arc<ToolRegistry>,
    memory_manager: Arc<Mutex<MemoryManager>>,
    skill_manager: Arc<SkillManager>,
    todo_senders: TodoSenders,
    todo_state: TodoState,
}

impl AgentRuntime {
    /// Create a new runtime.
    pub fn new(
        config: RuntimeConfig,
        tool_registry: Arc<ToolRegistry>,
        memory_manager: Arc<Mutex<MemoryManager>>,
        skill_manager: Arc<SkillManager>,
        todo_senders: TodoSenders,
        todo_state: TodoState,
    ) -> Self {
        Self {
            config: RwLock::new(config),
            sessions: RwLock::new(HashMap::new()),
            running: RwLock::new(HashMap::new()),
            activity: RwLock::new(HashMap::new()),
            agents: Mutex::new(HashMap::new()),
            tool_registry,
            memory_manager,
            skill_manager,
            todo_senders,
            todo_state,
        }
    }

    /// Update the LLM API key at runtime.
    pub async fn set_api_key(&self, api_key: Option<String>) {
        self.config.write().await.llm_api_key = api_key;
    }

    /// Return an existing session or create a new one.
    pub async fn get_or_create_session(&self, source: &SessionSource) -> SessionEntry {
        let key = build_session_key(source);

        {
            let sessions = self.sessions.read().await;
            if let Some(entry) = sessions.get(&key) {
                return entry.clone();
            }
        }

        let default_working_dir = self.config.read().await.default_working_dir.clone();
        let working_dir = default_working_dir
            .map(|d| {
                // Expand ~ to the user's home directory.
                let expanded = d.replacen(
                    '~',
                    &dirs::home_dir().unwrap_or_default().to_string_lossy(),
                    1,
                );
                std::path::PathBuf::from(expanded)
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let now = Instant::now();
        let entry = SessionEntry {
            session_id: Uuid::new_v4().to_string(),
            session_key: key.clone(),
            source: source.clone(),
            created_at: now,
            updated_at: now,
            input_tokens: 0,
            output_tokens: 0,
            message_count: 0,
            last_model: None,
            turns_since_review: 0,
            environment: SessionEnvironment::new(working_dir),
        };

        let mut sessions = self.sessions.write().await;
        // Double-check after acquiring write lock.
        sessions.entry(key).or_insert_with(|| entry.clone()).clone()
    }

    /// Remove a session (and its running/activity entries) so the next call
    /// starts a fresh one.
    pub async fn reset_session(&self, source: &SessionSource) {
        let key = build_session_key(source);
        self.sessions.write().await.remove(&key);
        self.running.write().await.remove(&key);
        self.activity.write().await.remove(&key);
        self.agents.lock().await.remove(&key);
    }

    /// Return `true` if an agent is currently executing for this session.
    pub async fn is_running(&self, source: &SessionSource) -> bool {
        let key = build_session_key(source);
        self.running.read().await.contains_key(&key)
    }

    /// Return the current [`SessionEntry`] if it exists.
    pub async fn get_session_info(&self, source: &SessionSource) -> Option<SessionEntry> {
        let key = build_session_key(source);
        self.sessions.read().await.get(&key).cloned()
    }

    // ─── handle_message ───

    /// Process a user message through the agent for the given session.
    ///
    /// Returns [`CoreError::AgentBusy`] immediately if the session already has
    /// an active agent call in flight.
    ///
    /// `conversation_history` — when non-empty, the messages are loaded into
    /// the agent before the user message is sent.  This supports the
    /// OpenAI-compatible pattern where the client resends the full message
    /// history on every request.
    pub async fn handle_message(
        &self,
        source: &SessionSource,
        user_message: String,
        agent_config: AgentConfig,
        event_callback: Option<EventCallback>,
        conversation_history: Vec<crate::llm::types::Message>,
    ) -> Result<AgentResponse, CoreError> {
        let key = build_session_key(source);

        // 1. Reject concurrent calls for the same session.
        {
            let running = self.running.read().await;
            if running.contains_key(&key) {
                return Err(CoreError::AgentBusy);
            }
        }

        // 2. Mark slot as pending.
        self.running
            .write()
            .await
            .insert(key.clone(), RunningState::Pending);

        // 3. Execute and always clean up the running slot.
        let result = self
            .execute_message(
                source,
                &key,
                user_message,
                agent_config,
                event_callback,
                conversation_history,
            )
            .await;

        self.running.write().await.remove(&key);

        result
    }

    // ─── private helpers ───

    async fn execute_message(
        &self,
        source: &SessionSource,
        key: &str,
        user_message: String,
        agent_config: AgentConfig,
        event_callback: Option<EventCallback>,
        conversation_history: Vec<crate::llm::types::Message>,
    ) -> Result<AgentResponse, CoreError> {
        // 1. Ensure session exists.
        let session_entry = self.get_or_create_session(source).await;

        // 2. Compute config signature and obtain an agent.
        let signature = compute_config_signature(&agent_config);
        let mut agent = self
            .get_or_create_agent(
                key,
                &signature,
                agent_config,
                &session_entry.session_id,
                session_entry.environment.clone(),
            )
            .await;

        // Load the session id into the agent.
        agent.set_session_id(session_entry.session_id.clone());

        // 2b. Detect model switch and inject note into user message.
        let current_model = agent.config().model_name.clone();
        let user_message = {
            let mut sessions = self.sessions.write().await;
            if let Some(entry) = sessions.get_mut(key) {
                if let Some(ref prev_model) = entry.last_model {
                    if *prev_model != current_model {
                        let note = format!(
                            "[Note: model was just switched from {} to {}. \
                             Adjust your self-identification accordingly.]\n\n",
                            prev_model, current_model
                        );
                        debug!("Model switch detected: {} -> {}", prev_model, current_model);
                        entry.last_model = Some(current_model.clone());
                        format!("{note}{user_message}")
                    } else {
                        user_message
                    }
                } else {
                    entry.last_model = Some(current_model.clone());
                    user_message
                }
            } else {
                user_message
            }
        };

        // 3a. If conversation history was provided, load it into the agent.
        //     This supports the OpenAI-compatible pattern where the client
        //     resends all prior messages on every request.
        if !conversation_history.is_empty() {
            agent.load_history(conversation_history);
        }

        // 3. Mark as actively running.
        self.running
            .write()
            .await
            .insert(key.to_string(), RunningState::Running(Instant::now()));

        // Update activity.
        self.activity.write().await.insert(
            key.to_string(),
            ActivityTracker {
                last_activity: Instant::now(),
            },
        );

        // 4. Call agent with timeout.
        let timeout_secs = self.config.read().await.agent_timeout_secs;
        let timeout_duration = std::time::Duration::from_secs(timeout_secs);

        let chat_result =
            tokio::time::timeout(timeout_duration, agent.chat(user_message, event_callback)).await;

        match chat_result {
            Ok(Ok(response)) => {
                // 5a. Update session stats.
                let should_review = {
                    let mut sessions = self.sessions.write().await;
                    if let Some(entry) = sessions.get_mut(key) {
                        entry.input_tokens += response.usage.prompt_tokens as u64;
                        entry.output_tokens += response.usage.completion_tokens as u64;
                        entry.message_count += 1;
                        entry.turns_since_review += 1;
                        entry.updated_at = Instant::now();

                        let interval = self.config.read().await.review_interval;
                        if interval > 0 && entry.turns_since_review >= interval {
                            entry.turns_since_review = 0;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                // 5b. Spawn background review if due.
                if should_review {
                    let messages_snapshot = agent.session().messages.clone();
                    self.spawn_background_review(messages_snapshot).await;
                }

                self.cache_agent(key, agent, &signature).await;
                debug!("Agent completed successfully for session {key}");
                Ok(response)
            }
            Ok(Err(e)) => {
                self.cache_agent(key, agent, &signature).await;
                Err(e)
            }
            Err(_elapsed) => {
                self.cache_agent(key, agent, &signature).await;
                Err(CoreError::Timeout(timeout_secs))
            }
        }
    }

    /// Retrieve a cached agent if the signature matches, or create a new one.
    async fn get_or_create_agent(
        &self,
        key: &str,
        signature: &str,
        agent_config: AgentConfig,
        session_id: &str,
        environment: SessionEnvironment,
    ) -> Agent {
        {
            let mut agents = self.agents.lock().await;
            if let Some(cached) = agents.get(key)
                && cached.config_signature == signature
            {
                // Take the cached agent out of the map.
                return agents.remove(key).unwrap().agent;
            }
        }

        // Cache miss — build a new agent.
        // Use model from AgentConfig (per-request override) if non-default,
        // otherwise fall back to RuntimeConfig (startup default).
        let cfg = self.config.read().await;
        let model = if agent_config.model_name.is_empty() || agent_config.model_name == "unknown" {
            cfg.llm_model.clone()
        } else {
            agent_config.model_name.clone()
        };
        let llm_config = LlmConfig {
            base_url: cfg.llm_base_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model,
            temperature: None,
            max_tokens: None,
        };
        drop(cfg);

        // Load SOUL.md identity and context files if present
        let mut agent_config = agent_config;
        if agent_config.identity.is_none() {
            agent_config.identity = load_soul_md();
        }
        if agent_config.context_files.is_empty() {
            agent_config.context_files = load_context_files();
        }

        let llm_client = LlmClient::new(llm_config);
        let todo_rx = create_todo_channel(&self.todo_senders, session_id);
        Agent::new(
            llm_client,
            Arc::clone(&self.tool_registry),
            Arc::clone(&self.memory_manager),
            Arc::clone(&self.skill_manager),
            agent_config,
            Some(todo_rx),
            Some(Arc::clone(&self.todo_state)),
            environment,
        )
    }

    /// Store an agent back into the cache.
    async fn cache_agent(&self, key: &str, agent: Agent, signature: &str) {
        let mut agents = self.agents.lock().await;
        agents.insert(
            key.to_string(),
            CachedAgent {
                agent,
                config_signature: signature.to_string(),
                last_used: Instant::now(),
            },
        );
    }
}

// ─── Background Review ───

/// Review prompt sent to a background agent to check if memory or skills
/// should be updated based on the conversation so far.
const BACKGROUND_REVIEW_PROMPT: &str = "\
Review the conversation above and consider two things:\n\n\
**Memory**: Has the user revealed things about themselves — their persona, \
preferences, or personal details? Has the user expressed expectations about \
how you should behave or operate? If so, save using the memory tool.\n\n\
**Skills**: Was a non-trivial approach used to complete a task that required \
trial and error, or changing course? If a relevant skill already exists, \
update it. Otherwise, create a new one if the approach is reusable.\n\n\
Only act if there's something genuinely worth saving. \
If nothing stands out, just say 'Nothing to save.' and stop.";

impl AgentRuntime {
    /// Spawn a background review agent that scans the conversation for
    /// memory/skill save opportunities. Runs silently with max 8 iterations
    /// and shared memory/skill stores.
    async fn spawn_background_review(&self, messages_snapshot: Vec<Message>) {
        let cfg = self.config.read().await;
        let llm_config = LlmConfig {
            base_url: cfg.llm_base_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model: cfg.llm_model.clone(),
            temperature: None,
            max_tokens: None,
        };
        drop(cfg);

        let tool_registry = Arc::clone(&self.tool_registry);
        let memory_manager = Arc::clone(&self.memory_manager);
        let skill_manager = Arc::clone(&self.skill_manager);

        tokio::spawn(async move {
            debug!(
                "Background review started ({} messages)",
                messages_snapshot.len()
            );

            let review_config = AgentConfig {
                max_iterations: 8,
                ..AgentConfig::default()
            };

            let llm_client = LlmClient::new(llm_config);
            let mut review_agent = Agent::new(
                llm_client,
                tool_registry,
                memory_manager,
                skill_manager,
                review_config,
                None,
                None,
                SessionEnvironment::new(std::env::current_dir().unwrap_or_default()),
            );

            // Load the conversation history
            review_agent.load_history(messages_snapshot);

            // Run the review (silently, no event callback)
            match review_agent
                .chat(BACKGROUND_REVIEW_PROMPT.to_string(), None)
                .await
            {
                Ok(resp) => {
                    debug!(
                        "Background review completed: {} tool calls, status: {:?}",
                        resp.tool_calls_made, resp.status
                    );
                }
                Err(e) => {
                    debug!("Background review failed: {e}");
                }
            }
        });
    }
}

// ─── File Loaders ───

/// Load `~/.iron-hermes/SOUL.md` if it exists and is non-empty.
///
/// Returns `Some(content)` with the file contents, or `None` if the file
/// does not exist or is empty.  Used as the agent identity (slot #1 in
/// the system prompt).
fn load_soul_md() -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home.join(".iron-hermes").join("SOUL.md");
    load_optional_file(&path, "SOUL.md")
}

/// Load context files from `~/.iron-hermes/`.
///
/// Checks for these files in priority order (first match wins):
///   1. `HERMES.md` (iron-hermes native)
///   2. `AGENTS.md` (generic convention)
///
/// Returns a vec of loaded context strings (0 or 1 entry).
/// Each file is capped at 20,000 characters.
fn load_context_files() -> Vec<String> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let base = home.join(".iron-hermes");

    const MAX_CONTEXT_CHARS: usize = 20_000;
    let candidates = ["HERMES.md", "AGENTS.md"];

    for name in &candidates {
        let path = base.join(name);
        if let Some(content) = load_optional_file(&path, name) {
            let truncated = if content.len() > MAX_CONTEXT_CHARS {
                debug!(
                    "Context file {} truncated from {} to {} chars",
                    name,
                    content.len(),
                    MAX_CONTEXT_CHARS
                );
                content[..MAX_CONTEXT_CHARS].to_string()
            } else {
                content
            };
            return vec![truncated];
        }
    }

    Vec::new()
}

/// Load an optional file, returning its trimmed content or None.
fn load_optional_file(path: &std::path::Path, label: &str) -> Option<String> {
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                debug!("Loaded {} ({} chars)", label, trimmed.len());
                Some(trimmed)
            }
        }
        Err(e) => {
            debug!("Could not read {}: {e}", label);
            None
        }
    }
}
