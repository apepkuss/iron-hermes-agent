use iron_core::runtime::{SessionSource, build_session_key};

fn webui_source() -> SessionSource {
    SessionSource {
        platform: "webui".to_string(),
        chat_id: "tab-123".to_string(),
        user_id: "local".to_string(),
        thread_id: None,
    }
}

fn slack_source() -> SessionSource {
    SessionSource {
        platform: "slack".to_string(),
        chat_id: "C123".to_string(),
        user_id: "U456".to_string(),
        thread_id: Some("T789".to_string()),
    }
}

#[test]
fn test_session_source_key_webui() {
    let key = build_session_key(&webui_source());
    assert_eq!(key, "webui:tab-123:local");
}

#[test]
fn test_session_source_key_with_thread() {
    let key = build_session_key(&slack_source());
    assert_eq!(key, "slack:C123:U456:T789");
}

#[test]
fn test_session_source_key_deterministic() {
    let source = webui_source();
    let key1 = build_session_key(&source);
    let key2 = build_session_key(&source);
    assert_eq!(key1, key2);
}

#[test]
fn test_session_source_different_platforms_different_keys() {
    let webui = webui_source();
    let telegram = SessionSource {
        platform: "telegram".to_string(),
        chat_id: "tab-123".to_string(),
        user_id: "local".to_string(),
        thread_id: None,
    };
    let key_webui = build_session_key(&webui);
    let key_telegram = build_session_key(&telegram);
    assert_ne!(key_webui, key_telegram);
}
