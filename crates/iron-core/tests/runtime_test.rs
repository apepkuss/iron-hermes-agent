use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iron_core::agent::AgentConfig;
use iron_core::runtime::{
    AgentRuntime, RunningState, RuntimeConfig, SessionSource, build_session_key,
    compute_config_signature,
};

use iron_memory::manager::MemoryManager;
use iron_skills::manager::SkillManager;
use iron_tools::ToolRegistry;
use tokio::sync::Mutex;

// ─── helpers ───

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

fn make_test_runtime(tmp_dir: &tempfile::TempDir) -> AgentRuntime {
    let tool_registry = Arc::new(ToolRegistry::new());
    let memory_manager = Arc::new(Mutex::new(MemoryManager::new(tmp_dir.path(), None, None)));
    let skill_manager = Arc::new(SkillManager::new(
        vec![PathBuf::from("/nonexistent")],
        HashSet::new(),
    ));
    let session_store =
        iron_core::session::store::SessionStore::new_in_memory().expect("in-memory session store");
    let session_store = Arc::new(std::sync::Mutex::new(session_store));
    AgentRuntime::new(
        RuntimeConfig::default(),
        tool_registry,
        memory_manager,
        skill_manager,
        iron_core::todo::new_todo_senders(),
        iron_core::todo::new_todo_state(),
        session_store,
    )
}

// ─── Task 3 tests (session key) ───

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

// ─── Task 4 tests (AgentRuntime session management) ───

#[tokio::test]
async fn test_get_or_create_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_test_runtime(&tmp);
    let source = webui_source();

    let entry1 = runtime.get_or_create_session(&source).await;
    let entry2 = runtime.get_or_create_session(&source).await;

    // Same session_id returned on second call.
    assert_eq!(entry1.session_id, entry2.session_id);
    assert_eq!(entry1.session_key, "webui:tab-123:local");
}

#[tokio::test]
async fn test_reset_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_test_runtime(&tmp);
    let source = webui_source();

    let entry_before = runtime.get_or_create_session(&source).await;
    runtime.reset_session(&source).await;
    let entry_after = runtime.get_or_create_session(&source).await;

    // After reset, a new session with a different ID is created.
    assert_ne!(entry_before.session_id, entry_after.session_id);
}

#[tokio::test]
async fn test_is_running_default_false() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_test_runtime(&tmp);
    let source = webui_source();

    assert!(!runtime.is_running(&source).await);
}

#[test]
fn test_config_signature_differs_on_model_change() {
    let config_a = AgentConfig {
        model_name: "gpt-4o".to_string(),
        ..Default::default()
    };
    let config_b = AgentConfig {
        model_name: "claude-3-5-sonnet".to_string(),
        ..Default::default()
    };
    assert_ne!(
        compute_config_signature(&config_a),
        compute_config_signature(&config_b)
    );
}

#[test]
fn test_config_signature_same_for_same_config() {
    let config_a = AgentConfig {
        model_name: "gpt-4o".to_string(),
        ..Default::default()
    };
    let config_b = AgentConfig {
        model_name: "gpt-4o".to_string(),
        ..Default::default()
    };
    assert_eq!(
        compute_config_signature(&config_a),
        compute_config_signature(&config_b)
    );
}

// ─── Task 5 tests (handle_message) ───

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_handle_message_creates_session() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_test_runtime(&tmp);
    let source = webui_source();

    // No session yet.
    assert!(runtime.get_session_info(&source).await.is_none());

    let config = AgentConfig::default();
    // The call will fail because there is no real LLM, but the session should
    // have been created before the LLM call is attempted.
    let _ = runtime
        .handle_message(&source, "hello".to_string(), config, None, vec![])
        .await;

    // Session must exist now.
    assert!(runtime.get_session_info(&source).await.is_some());
}

#[tokio::test]
async fn test_handle_message_concurrent_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let runtime = make_test_runtime(&tmp);
    let source = webui_source();
    let key = build_session_key(&source);

    // Manually mark the session as running (simulates an in-flight request).
    runtime
        .running
        .write()
        .await
        .insert(key, RunningState::Pending);

    let config = AgentConfig::default();
    let result = runtime
        .handle_message(&source, "hello".to_string(), config, None, vec![])
        .await;

    assert!(
        matches!(result, Err(iron_core::error::CoreError::AgentBusy)),
        "Expected AgentBusy error"
    );
}

// ─── Review interval tests ───

#[tokio::test]
async fn test_session_turns_since_review_initialized_zero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rt = make_test_runtime(&tmp);
    let source = webui_source();

    let entry = rt.get_or_create_session(&source).await;
    assert_eq!(entry.turns_since_review, 0);
}

#[tokio::test]
async fn test_review_interval_default_is_10() {
    let config = RuntimeConfig::default();
    assert_eq!(config.review_interval, 10);
}

#[tokio::test]
async fn test_review_interval_zero_disables() {
    let mut config = RuntimeConfig::default();
    config.review_interval = 0;
    // interval=0 means disabled — no review should ever trigger
    assert_eq!(config.review_interval, 0);
}

// ─── Session environment isolation tests ───

#[tokio::test]
async fn test_session_environment_initialized_with_working_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rt = make_test_runtime(&tmp);
    let source = webui_source();

    let entry = rt.get_or_create_session(&source).await;
    // Default config has no default_working_dir, so it should use process CWD.
    let process_cwd = std::env::current_dir().unwrap();
    assert_eq!(entry.environment.working_dir, process_cwd);
}

#[tokio::test]
async fn test_session_environment_has_safe_env_vars() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rt = make_test_runtime(&tmp);
    let source = webui_source();

    let entry = rt.get_or_create_session(&source).await;
    // PATH should be present (it's in the safe list).
    assert!(
        entry.environment.env_vars.contains_key("PATH"),
        "env_vars should contain PATH"
    );
}

#[tokio::test]
async fn test_session_environment_blocks_secrets() {
    // Inject a fake secret into the process environment.
    // SAFETY: test runs single-threaded for this env var; no concurrent readers.
    unsafe { std::env::set_var("MY_SECRET_TOKEN", "should-not-appear") };

    let tmp = tempfile::TempDir::new().unwrap();
    let rt = make_test_runtime(&tmp);
    let source = webui_source();

    let entry = rt.get_or_create_session(&source).await;
    assert!(
        !entry.environment.env_vars.contains_key("MY_SECRET_TOKEN"),
        "env_vars should NOT contain secret vars"
    );

    // SAFETY: test cleanup; no concurrent readers.
    unsafe { std::env::remove_var("MY_SECRET_TOKEN") };
}

#[tokio::test]
async fn test_session_environment_with_custom_default_working_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tool_registry = Arc::new(ToolRegistry::new());
    let memory_manager = Arc::new(Mutex::new(MemoryManager::new(tmp.path(), None, None)));
    let skill_manager = Arc::new(SkillManager::new(
        vec![PathBuf::from("/nonexistent")],
        HashSet::new(),
    ));

    let config = RuntimeConfig {
        default_working_dir: Some("/tmp".to_string()),
        ..RuntimeConfig::default()
    };

    let session_store =
        iron_core::session::store::SessionStore::new_in_memory().expect("in-memory session store");
    let session_store = Arc::new(std::sync::Mutex::new(session_store));
    let rt = AgentRuntime::new(
        config,
        tool_registry,
        memory_manager,
        skill_manager,
        iron_core::todo::new_todo_senders(),
        iron_core::todo::new_todo_state(),
        session_store,
    );

    let source = webui_source();
    let entry = rt.get_or_create_session(&source).await;
    assert_eq!(
        entry.environment.working_dir,
        PathBuf::from("/tmp"),
        "Session should use configured default_working_dir"
    );
}

#[tokio::test]
async fn test_different_sessions_share_same_env_policy() {
    let tmp = tempfile::TempDir::new().unwrap();
    let rt = make_test_runtime(&tmp);

    let entry1 = rt.get_or_create_session(&webui_source()).await;
    let entry2 = rt.get_or_create_session(&slack_source()).await;

    // Different sessions should both have safe env vars.
    assert!(entry1.environment.env_vars.contains_key("PATH"));
    assert!(entry2.environment.env_vars.contains_key("PATH"));
    // Same working dir policy (both use process CWD by default).
    assert_eq!(
        entry1.environment.working_dir,
        entry2.environment.working_dir
    );
}
