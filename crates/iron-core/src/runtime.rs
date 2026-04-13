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
}

impl AgentRuntime {
    /// Create a new runtime.
    pub fn new(
        config: RuntimeConfig,
        tool_registry: Arc<ToolRegistry>,
        memory_manager: Arc<Mutex<MemoryManager>>,
        skill_manager: Arc<SkillManager>,
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
        }
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
    pub async fn handle_message(
        &self,
        source: &SessionSource,
        user_message: String,
        agent_config: AgentConfig,
        event_callback: Option<EventCallback>,
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
            .execute_message(source, &key, user_message, agent_config, event_callback)
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
    ) -> Result<AgentResponse, CoreError> {
        // 1. Ensure session exists.
        let session_entry = self.get_or_create_session(source).await;

        // 2. Compute config signature and obtain an agent.
        let signature = compute_config_signature(&agent_config);
        let mut agent = self
            .get_or_create_agent(key, &signature, agent_config)
            .await;

        // Load the session id into the agent.
        agent.set_session_id(session_entry.session_id.clone());

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
                {
                    let mut sessions = self.sessions.write().await;
                    if let Some(entry) = sessions.get_mut(key) {
                        entry.input_tokens += response.usage.prompt_tokens as u64;
                        entry.output_tokens += response.usage.completion_tokens as u64;
                        entry.message_count += 1;
                        entry.updated_at = Instant::now();
                    }
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
        let cfg = self.config.read().await;
        let llm_config = LlmConfig {
            base_url: cfg.llm_base_url.clone(),
            api_key: cfg.llm_api_key.clone(),
            model: cfg.llm_model.clone(),
            temperature: None,
            max_tokens: None,
        };
        drop(cfg);

        let llm_client = LlmClient::new(llm_config);
        Agent::new(
            llm_client,
            Arc::clone(&self.tool_registry),
            Arc::clone(&self.memory_manager),
            Arc::clone(&self.skill_manager),
            agent_config,
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
