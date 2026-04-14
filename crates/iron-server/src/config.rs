use std::env;

use serde::{Deserialize, Serialize};

/// Configuration for the iron-hermes HTTP server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Bind host. Default: `"0.0.0.0"`.
    pub host: String,
    /// Bind port. Default: `8080`.
    pub port: u16,
    /// Model name exposed via `/v1/models`. Default: `"iron-hermes"`.
    pub model_name: String,
    /// Optional bearer token for request authentication (used in future middleware).
    #[allow(dead_code)]
    pub auth_token: Option<String>,
    /// Base URL for the upstream LLM provider.
    pub llm_base_url: String,
    /// Optional API key for the upstream LLM.
    pub llm_api_key: Option<String>,
    /// Model identifier sent to the upstream LLM.
    pub llm_model: String,
    /// Optional auxiliary model for context compression summarisation.
    pub auxiliary_model: Option<String>,
    /// Compression trigger threshold (0.50–0.95). Default: 0.65.
    pub compression_threshold: f64,
    /// Optional override for the primary LLM context length (in tokens).
    pub context_length_override: Option<u64>,
    /// Optional fallback model used when the primary model is unavailable.
    pub fallback_model: Option<String>,
    /// Maximum seconds an agent is allowed to run before timing out.
    pub agent_timeout_secs: u64,
}

impl ServerConfig {
    /// Build a `ServerConfig` from environment variables.
    ///
    /// | Variable                | Default        |
    /// |-------------------------|----------------|
    /// | `IRON_HOST`             | `0.0.0.0`      |
    /// | `IRON_PORT`             | `8080`         |
    /// | `IRON_MODEL_NAME`       | `iron-hermes`  |
    /// | `IRON_AUTH_TOKEN`       | *(none)*       |
    /// | `LLM_BASE_URL`          | **required**   |
    /// | `LLM_API_KEY`           | *(none)*       |
    /// | `LLM_MODEL`             | **required**   |
    /// | `AUX_MODEL`             | *(none)*       |
    /// | `COMPRESSION_THRESHOLD` | `0.65`         |
    /// | `CONTEXT_LENGTH`        | *(none)*       |
    /// | `FALLBACK_MODEL`        | *(none)*       |
    /// | `AGENT_TIMEOUT`         | `600`          |
    pub fn from_env() -> Self {
        Self {
            host: env::var("IRON_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: env::var("IRON_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8080),
            model_name: env::var("IRON_MODEL_NAME").unwrap_or_else(|_| "iron-hermes".to_string()),
            auth_token: env::var("IRON_AUTH_TOKEN").ok(),
            llm_base_url: env::var("LLM_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            llm_api_key: env::var("LLM_API_KEY").ok(),
            llm_model: env::var("LLM_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            auxiliary_model: env::var("AUX_MODEL").ok(),
            compression_threshold: env::var("COMPRESSION_THRESHOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.65),
            context_length_override: env::var("CONTEXT_LENGTH").ok().and_then(|v| v.parse().ok()),
            fallback_model: env::var("FALLBACK_MODEL").ok(),
            agent_timeout_secs: env::var("AGENT_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(600),
        }
    }
}

/// Runtime-mutable configuration surfaced via `/api/config`.
///
/// Unlike [`ServerConfig`] (read-once at startup), this struct can be
/// updated live through the `POST /api/config` endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Base URL of the upstream LLM provider.
    pub llm_base_url: String,
    /// Model identifier used for the primary LLM.
    pub llm_model: String,
    /// Optional auxiliary model used for context compression summarisation.
    pub auxiliary_model: Option<String>,
    /// Compression trigger threshold (0.50–0.95).
    pub compression_threshold: f64,
    /// Optional override for the primary LLM context length (in tokens).
    pub context_length_override: Option<u64>,
    /// Optional fallback model used when the primary model is unavailable.
    pub fallback_model: Option<String>,
    /// Maximum seconds an agent is allowed to run before timing out.
    #[serde(default = "default_agent_timeout_secs")]
    pub agent_timeout_secs: u64,
    /// Seconds of inactivity before an agent is considered idle.
    #[serde(default = "default_inactivity_timeout_secs")]
    pub inactivity_timeout_secs: u64,
    /// Seconds before an idle session is expired and cleaned up.
    #[serde(default = "default_session_idle_timeout_secs")]
    pub session_idle_timeout_secs: u64,
    /// Toolsets to disable. Tools in these toolsets are not available to the agent.
    #[serde(default)]
    pub disabled_toolsets: Vec<String>,
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
