use std::env;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Default value functions
// ---------------------------------------------------------------------------

mod defaults {
    pub fn model() -> String {
        "llama3".to_string()
    }
    pub fn base_url() -> String {
        "http://localhost:9068".to_string()
    }
    pub fn host() -> String {
        "0.0.0.0".to_string()
    }
    pub fn port() -> u16 {
        8080
    }
    pub fn model_name() -> String {
        "iron-hermes".to_string()
    }
    pub fn max_turns() -> u32 {
        90
    }
    pub fn agent_timeout() -> u64 {
        600
    }
    pub fn inactivity_timeout() -> u64 {
        300
    }
    pub fn review_interval() -> u32 {
        10
    }
    pub fn session_idle_timeout() -> u64 {
        1800
    }
    pub fn compression_enabled() -> bool {
        true
    }
    pub fn compression_threshold() -> f64 {
        0.65
    }
}

// ---------------------------------------------------------------------------
// IronConfig — unified configuration matching ~/.iron-hermes/config.yaml
// ---------------------------------------------------------------------------

/// Unified configuration for iron-hermes, loaded from config.yaml with
/// environment variable overrides.
///
/// Priority: environment variables > config.yaml > hard-coded defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IronConfig {
    #[serde(default = "defaults::model")]
    pub model: String,

    #[serde(default = "defaults::base_url")]
    pub base_url: String,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub context_length: Option<u64>,

    #[serde(default)]
    pub server: ServerSection,

    #[serde(default)]
    pub agent: AgentSection,

    #[serde(default)]
    pub session: SessionSection,

    #[serde(default)]
    pub compression: CompressionSection,

    #[serde(default)]
    pub fallback: FallbackSection,

    #[serde(default)]
    pub toolsets: ToolsetsSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSection {
    #[serde(default = "defaults::host")]
    pub host: String,
    #[serde(default = "defaults::port")]
    pub port: u16,
    #[serde(default = "defaults::model_name")]
    pub model_name: String,
    #[serde(default)]
    pub auth_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSection {
    #[serde(default = "defaults::max_turns")]
    pub max_turns: u32,
    #[serde(default = "defaults::agent_timeout")]
    pub timeout: u64,
    #[serde(default = "defaults::inactivity_timeout")]
    pub inactivity_timeout: u64,
    #[serde(default = "defaults::review_interval")]
    pub review_interval: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSection {
    #[serde(default = "defaults::session_idle_timeout")]
    pub idle_timeout: u64,
    /// Default working directory for new sessions.
    /// Supports `~` expansion. If not set, uses the process CWD.
    #[serde(default)]
    pub default_working_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionSection {
    #[serde(default = "defaults::compression_enabled")]
    pub enabled: bool,
    #[serde(default = "defaults::compression_threshold")]
    pub threshold: f64,
    #[serde(default)]
    pub summary_model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FallbackSection {
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsetsSection {
    #[serde(default)]
    pub disabled: Vec<String>,
}

// --- Default impls ---

impl Default for IronConfig {
    fn default() -> Self {
        Self {
            model: defaults::model(),
            base_url: defaults::base_url(),
            api_key: None,
            context_length: None,
            server: ServerSection::default(),
            agent: AgentSection::default(),
            session: SessionSection::default(),
            compression: CompressionSection::default(),
            fallback: FallbackSection::default(),
            toolsets: ToolsetsSection::default(),
        }
    }
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            host: defaults::host(),
            port: defaults::port(),
            model_name: defaults::model_name(),
            auth_token: None,
        }
    }
}

impl Default for AgentSection {
    fn default() -> Self {
        Self {
            max_turns: defaults::max_turns(),
            timeout: defaults::agent_timeout(),
            inactivity_timeout: defaults::inactivity_timeout(),
            review_interval: defaults::review_interval(),
        }
    }
}

impl Default for SessionSection {
    fn default() -> Self {
        Self {
            idle_timeout: defaults::session_idle_timeout(),
            default_working_dir: None,
        }
    }
}

impl Default for CompressionSection {
    fn default() -> Self {
        Self {
            enabled: defaults::compression_enabled(),
            threshold: defaults::compression_threshold(),
            summary_model: None,
        }
    }
}

// --- Loading logic ---

impl IronConfig {
    /// Load configuration: defaults → config.yaml → environment variable overrides → validate.
    pub fn load() -> Self {
        let mut config = Self::load_from_yaml();
        config.apply_env_overrides();
        config.validate();
        config
    }

    /// Path to the configuration file.
    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".iron-hermes")
            .join("config.yaml")
    }

    /// Load from YAML file; generate the default file if it does not exist.
    fn load_from_yaml() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_yaml::from_str::<IronConfig>(&content) {
                    Ok(config) => {
                        info!("Loaded config from {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        warn!("Failed to parse {}: {e}, using defaults", path.display());
                    }
                },
                Err(e) => {
                    warn!("Failed to read {}: {e}, using defaults", path.display());
                }
            }
        } else {
            Self::generate_default_config(&path);
        }
        Self::default()
    }

    /// Apply environment variable overrides (highest priority).
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = env::var("LLM_MODEL") {
            self.model = v;
        }
        if let Ok(v) = env::var("LLM_BASE_URL") {
            self.base_url = v;
        }
        if let Ok(v) = env::var("LLM_API_KEY") {
            self.api_key = Some(v);
        }
        if let Ok(v) = env::var("CONTEXT_LENGTH")
            && let Ok(n) = v.parse()
        {
            self.context_length = Some(n);
        }
        if let Ok(v) = env::var("IRON_HOST") {
            self.server.host = v;
        }
        if let Ok(v) = env::var("IRON_PORT")
            && let Ok(n) = v.parse()
        {
            self.server.port = n;
        }
        if let Ok(v) = env::var("IRON_MODEL_NAME") {
            self.server.model_name = v;
        }
        if let Ok(v) = env::var("IRON_AUTH_TOKEN") {
            self.server.auth_token = Some(v);
        }
        if let Ok(v) = env::var("AGENT_TIMEOUT")
            && let Ok(n) = v.parse()
        {
            self.agent.timeout = n;
        }
        if let Ok(v) = env::var("AUX_MODEL") {
            self.compression.summary_model = Some(v);
        }
        if let Ok(v) = env::var("COMPRESSION_THRESHOLD")
            && let Ok(n) = v.parse()
        {
            self.compression.threshold = n;
        }
        if let Ok(v) = env::var("FALLBACK_MODEL") {
            self.fallback.model = Some(v);
        }
    }

    /// Validate and clamp configuration values.
    pub fn validate(&mut self) {
        self.compression.threshold = self.compression.threshold.clamp(0.50, 0.95);
    }

    /// Write the annotated default configuration template to disk.
    pub fn generate_default_config(path: &std::path::Path) {
        let template = include_str!("default_config.yaml");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        match std::fs::write(path, template) {
            Ok(()) => info!("Generated default config at {}", path.display()),
            Err(e) => warn!("Failed to generate default config: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// ServerConfig — read-once at startup (kept for backward compatibility)
// ---------------------------------------------------------------------------

/// Configuration for the iron-hermes HTTP server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub model_name: String,
    #[allow(dead_code)]
    pub auth_token: Option<String>,
    pub llm_base_url: String,
    pub llm_api_key: Option<String>,
    pub llm_model: String,
    pub auxiliary_model: Option<String>,
    pub compression_threshold: f64,
    pub context_length_override: Option<u64>,
    pub fallback_model: Option<String>,
    pub agent_timeout_secs: u64,
}

impl From<&IronConfig> for ServerConfig {
    fn from(c: &IronConfig) -> Self {
        Self {
            host: c.server.host.clone(),
            port: c.server.port,
            model_name: c.server.model_name.clone(),
            auth_token: c.server.auth_token.clone(),
            llm_base_url: c.base_url.clone(),
            llm_api_key: c
                .api_key
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(String::from),
            llm_model: c.model.clone(),
            auxiliary_model: c.compression.summary_model.clone(),
            compression_threshold: c.compression.threshold,
            context_length_override: c.context_length,
            fallback_model: c.fallback.model.clone(),
            agent_timeout_secs: c.agent.timeout,
        }
    }
}

// ---------------------------------------------------------------------------
// RuntimeConfig — mutable at runtime via /api/config
// ---------------------------------------------------------------------------

/// Runtime-mutable configuration surfaced via `/api/config`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub llm_base_url: String,
    pub llm_api_key: Option<String>,
    pub llm_model: String,
    pub auxiliary_model: Option<String>,
    pub compression_threshold: f64,
    pub context_length_override: Option<u64>,
    pub fallback_model: Option<String>,
    #[serde(default = "default_agent_timeout_secs")]
    pub agent_timeout_secs: u64,
    #[serde(default = "default_inactivity_timeout_secs")]
    pub inactivity_timeout_secs: u64,
    #[serde(default = "default_session_idle_timeout_secs")]
    pub session_idle_timeout_secs: u64,
    #[serde(default)]
    pub disabled_toolsets: Vec<String>,
}

impl RuntimeConfig {
    pub fn from_iron_config(c: &IronConfig) -> Self {
        Self {
            llm_base_url: c.base_url.clone(),
            llm_api_key: c
                .api_key
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(String::from),
            llm_model: c.model.clone(),
            auxiliary_model: c.compression.summary_model.clone(),
            compression_threshold: c.compression.threshold,
            context_length_override: c.context_length,
            fallback_model: c.fallback.model.clone(),
            agent_timeout_secs: c.agent.timeout,
            inactivity_timeout_secs: c.agent.inactivity_timeout,
            session_idle_timeout_secs: c.session.idle_timeout,
            disabled_toolsets: c.toolsets.disabled.clone(),
        }
    }
}

fn default_agent_timeout_secs() -> u64 {
    600
}

fn default_inactivity_timeout_secs() -> u64 {
    300
}

fn default_session_idle_timeout_secs() -> u64 {
    1800
}
