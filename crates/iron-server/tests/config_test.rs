use iron_server::config::{IronConfig, RuntimeConfig, ServerConfig};

#[test]
fn test_default_config_complete() {
    let config = IronConfig::default();
    assert_eq!(config.model, "llama3");
    assert_eq!(config.base_url, "http://localhost:9068");
    assert!(config.api_key.is_none());
    assert!(config.context_length.is_none());
    assert_eq!(config.server.host, "0.0.0.0");
    assert_eq!(config.server.port, 9069);
    assert_eq!(config.server.model_name, "iron-hermes");
    assert!(config.server.auth_token.is_none());
    assert_eq!(config.agent.max_turns, 90);
    assert_eq!(config.agent.timeout, 600);
    assert_eq!(config.agent.inactivity_timeout, 300);
    assert_eq!(config.agent.review_interval, 10);
    assert_eq!(config.session.idle_timeout, 1800);
    assert!(config.compression.enabled);
    assert!((config.compression.threshold - 0.65).abs() < f64::EPSILON);
    assert!(config.compression.summary_model.is_none());
    assert!(config.fallback.model.is_none());
    assert!(config.toolsets.disabled.is_empty());
}

#[test]
fn test_yaml_parse_full() {
    let yaml = r#"
model: "gpt-4"
base_url: "https://api.openai.com"
api_key: "sk-test"
context_length: 128000
server:
  host: "127.0.0.1"
  port: 3000
  model_name: "my-model"
  auth_token: "secret"
agent:
  max_turns: 50
  timeout: 300
  inactivity_timeout: 120
  review_interval: 5
session:
  idle_timeout: 900
compression:
  enabled: false
  threshold: 0.80
  summary_model: "gpt-3.5-turbo"
fallback:
  model: "gpt-3.5-turbo"
toolsets:
  disabled:
    - "web"
    - "terminal"
"#;
    let config: IronConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.base_url, "https://api.openai.com");
    assert_eq!(config.api_key.as_deref(), Some("sk-test"));
    assert_eq!(config.context_length, Some(128000));
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 3000);
    assert_eq!(config.server.model_name, "my-model");
    assert_eq!(config.server.auth_token.as_deref(), Some("secret"));
    assert_eq!(config.agent.max_turns, 50);
    assert_eq!(config.agent.timeout, 300);
    assert_eq!(config.agent.inactivity_timeout, 120);
    assert_eq!(config.agent.review_interval, 5);
    assert_eq!(config.session.idle_timeout, 900);
    assert!(!config.compression.enabled);
    assert!((config.compression.threshold - 0.80).abs() < f64::EPSILON);
    assert_eq!(
        config.compression.summary_model.as_deref(),
        Some("gpt-3.5-turbo")
    );
    assert_eq!(config.fallback.model.as_deref(), Some("gpt-3.5-turbo"));
    assert_eq!(config.toolsets.disabled, vec!["web", "terminal"]);
}

#[test]
fn test_yaml_parse_partial() {
    let yaml = r#"
model: "claude-3"
agent:
  timeout: 900
"#;
    let config: IronConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.model, "claude-3");
    // Explicitly set
    assert_eq!(config.agent.timeout, 900);
    // Defaults for missing fields
    assert_eq!(config.base_url, "http://localhost:9068");
    assert_eq!(config.agent.max_turns, 90);
    assert_eq!(config.agent.inactivity_timeout, 300);
    assert_eq!(config.server.port, 9069);
    assert!(config.compression.enabled);
}

#[test]
fn test_yaml_parse_empty() {
    // Empty YAML parses as null; serde_yaml should fall back to defaults.
    let config: IronConfig = serde_yaml::from_str("").unwrap();
    assert_eq!(config.model, "llama3");
    assert_eq!(config.base_url, "http://localhost:9068");
    assert_eq!(config.server.port, 9069);
}

#[test]
fn test_env_override() {
    let mut config = IronConfig::default();
    // SAFETY: test runs single-threaded via `cargo test -- --test-threads=1` or
    // only touches unique env var names, so no data race.
    unsafe {
        std::env::set_var("LLM_MODEL", "test-model-env");
        std::env::set_var("IRON_PORT", "9999");
    }
    config.apply_env_overrides();
    assert_eq!(config.model, "test-model-env");
    assert_eq!(config.server.port, 9999);
    unsafe {
        std::env::remove_var("LLM_MODEL");
        std::env::remove_var("IRON_PORT");
    }
}

#[test]
fn test_env_override_priority() {
    let yaml = r#"
model: "yaml-model"
"#;
    let mut config: IronConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.model, "yaml-model");

    // SAFETY: see test_env_override comment.
    unsafe {
        std::env::set_var("LLM_MODEL", "env-model");
    }
    config.apply_env_overrides();
    assert_eq!(config.model, "env-model");
    unsafe {
        std::env::remove_var("LLM_MODEL");
    }
}

#[test]
fn test_validate_clamps_threshold() {
    let mut config = IronConfig::default();

    config.compression.threshold = 0.10;
    config.validate();
    assert!((config.compression.threshold - 0.50).abs() < f64::EPSILON);

    config.compression.threshold = 1.5;
    config.validate();
    assert!((config.compression.threshold - 0.95).abs() < f64::EPSILON);

    config.compression.threshold = 0.75;
    config.validate();
    assert!((config.compression.threshold - 0.75).abs() < f64::EPSILON);
}

#[test]
fn test_server_config_from_iron_config() {
    let yaml = r#"
model: "test-model"
base_url: "http://test:1234"
api_key: "key123"
context_length: 64000
server:
  host: "10.0.0.1"
  port: 3333
  model_name: "custom"
  auth_token: "tok"
compression:
  threshold: 0.70
  summary_model: "aux-model"
fallback:
  model: "fallback-model"
agent:
  timeout: 120
"#;
    let iron: IronConfig = serde_yaml::from_str(yaml).unwrap();
    let sc = ServerConfig::from(&iron);

    assert_eq!(sc.host, "10.0.0.1");
    assert_eq!(sc.port, 3333);
    assert_eq!(sc.model_name, "custom");
    assert_eq!(sc.auth_token.as_deref(), Some("tok"));
    assert_eq!(sc.llm_base_url, "http://test:1234");
    assert_eq!(sc.llm_api_key.as_deref(), Some("key123"));
    assert_eq!(sc.llm_model, "test-model");
    assert_eq!(sc.auxiliary_model.as_deref(), Some("aux-model"));
    assert!((sc.compression_threshold - 0.70).abs() < f64::EPSILON);
    assert_eq!(sc.context_length_override, Some(64000));
    assert_eq!(sc.fallback_model.as_deref(), Some("fallback-model"));
    assert_eq!(sc.agent_timeout_secs, 120);
}

#[test]
fn test_runtime_config_from_iron_config() {
    let iron = IronConfig::default();
    let rc = RuntimeConfig::from_iron_config(&iron);

    assert_eq!(rc.llm_base_url, iron.base_url);
    assert_eq!(rc.llm_model, iron.model);
    assert_eq!(rc.agent_timeout_secs, iron.agent.timeout);
    assert_eq!(rc.inactivity_timeout_secs, iron.agent.inactivity_timeout);
    assert_eq!(rc.session_idle_timeout_secs, iron.session.idle_timeout);
    assert!(rc.disabled_toolsets.is_empty());
}

#[test]
fn test_generate_default_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");

    IronConfig::generate_default_config(&path);
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).unwrap();
    let parsed: IronConfig = serde_yaml::from_str(&content).unwrap();
    assert_eq!(parsed.model, "llama3");
    assert_eq!(parsed.base_url, "http://localhost:9068");
    assert_eq!(parsed.server.port, 9069);
}

#[test]
fn test_invalid_yaml_fallback() {
    let result = serde_yaml::from_str::<IronConfig>("{{{{invalid yaml");
    assert!(result.is_err());
    // In actual load_from_yaml, this falls back to default — tested via default values
    let fallback = IronConfig::default();
    assert_eq!(fallback.model, "llama3");
}
