use iron_core::runtime::{RuntimeConfig, SessionSource, build_session_key};

#[test]
fn test_session_key_format() {
    let source = SessionSource {
        platform: "webui".into(),
        chat_id: "abc".into(),
        user_id: "local".into(),
        thread_id: None,
    };
    assert_eq!(build_session_key(&source), "webui:abc:local");
}

#[test]
fn test_session_key_with_thread() {
    let source = SessionSource {
        platform: "slack".into(),
        chat_id: "C1".into(),
        user_id: "U1".into(),
        thread_id: Some("T1".into()),
    };
    assert_eq!(build_session_key(&source), "slack:C1:U1:T1");
}

#[test]
fn test_runtime_config_defaults() {
    let cfg = RuntimeConfig::default();
    assert_eq!(cfg.agent_timeout_secs, 600);
    assert_eq!(cfg.inactivity_timeout_secs, 300);
    assert_eq!(cfg.session_idle_timeout_secs, 1800);
    assert!(cfg.fallback_model.is_none());
}
