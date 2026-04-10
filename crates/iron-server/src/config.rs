use std::env;

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
}

impl ServerConfig {
    /// Build a `ServerConfig` from environment variables.
    ///
    /// | Variable          | Default        |
    /// |-------------------|----------------|
    /// | `IRON_HOST`       | `0.0.0.0`      |
    /// | `IRON_PORT`       | `8080`         |
    /// | `IRON_MODEL_NAME` | `iron-hermes`  |
    /// | `IRON_AUTH_TOKEN`  | *(none)*       |
    /// | `LLM_BASE_URL`    | **required**   |
    /// | `LLM_API_KEY`     | *(none)*       |
    /// | `LLM_MODEL`       | **required**   |
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
        }
    }
}
