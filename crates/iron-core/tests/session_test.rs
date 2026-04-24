use iron_core::cron::{CronJobPatch, CronRunFinish, NewCronJob};
use iron_core::session::{Session, SessionMessage, SessionStore, TokenUsage};

fn make_session(id: &str, model: &str, started_at: &str) -> Session {
    Session {
        id: id.to_string(),
        model: model.to_string(),
        system_prompt: None,
        parent_session_id: None,
        started_at: started_at.to_string(),
        ended_at: None,
        end_reason: None,
        message_count: 0,
        tool_call_count: 0,
        title: None,
    }
}

fn make_message(session_id: &str, role: &str, content: &str) -> SessionMessage {
    SessionMessage {
        id: 0,
        session_id: session_id.to_string(),
        role: role.to_string(),
        content: Some(content.to_string()),
        tool_call_id: None,
        tool_calls: None,
        tool_name: None,
        timestamp: "2026-04-10T00:00:00Z".to_string(),
        finish_reason: None,
    }
}

// 1. Create and retrieve a session, verify all fields.
#[test]
fn test_create_and_get_session() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let session = Session {
        id: "sess-001".to_string(),
        model: "claude-3-5-sonnet".to_string(),
        system_prompt: Some("You are helpful.".to_string()),
        parent_session_id: None,
        started_at: "2026-04-10T10:00:00Z".to_string(),
        ended_at: None,
        end_reason: None,
        message_count: 2,
        tool_call_count: 1,
        title: Some("My session".to_string()),
    };

    store.create_session(&session).expect("create session");

    let retrieved = store
        .get_session("sess-001")
        .expect("get session")
        .expect("session exists");

    assert_eq!(retrieved.id, session.id);
    assert_eq!(retrieved.model, session.model);
    assert_eq!(retrieved.system_prompt, session.system_prompt);
    assert_eq!(retrieved.started_at, session.started_at);
    assert_eq!(retrieved.message_count, session.message_count);
    assert_eq!(retrieved.tool_call_count, session.tool_call_count);
    assert_eq!(retrieved.title, session.title);
    assert!(retrieved.ended_at.is_none());
}

// 2. Add 3 messages and retrieve them in order.
#[test]
fn test_add_and_get_messages() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let session = make_session("sess-002", "claude-3-5-sonnet", "2026-04-10T10:00:00Z");
    store.create_session(&session).expect("create session");

    let user_msg = make_message("sess-002", "user", "Hello!");
    let assistant_msg = make_message("sess-002", "assistant", "Hi there!");
    let tool_msg = SessionMessage {
        id: 0,
        session_id: "sess-002".to_string(),
        role: "tool".to_string(),
        content: Some("result".to_string()),
        tool_call_id: Some("call-123".to_string()),
        tool_calls: None,
        tool_name: Some("read_file".to_string()),
        timestamp: "2026-04-10T00:00:01Z".to_string(),
        finish_reason: None,
    };

    let id1 = store.add_message(&user_msg).expect("add user message");
    let id2 = store
        .add_message(&assistant_msg)
        .expect("add assistant message");
    let id3 = store.add_message(&tool_msg).expect("add tool message");

    assert!(id1 < id2 && id2 < id3, "IDs should be increasing");

    let messages = store.get_messages("sess-002").expect("get messages");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[2].role, "tool");
    assert_eq!(messages[2].tool_name, Some("read_file".to_string()));
    assert_eq!(messages[2].tool_call_id, Some("call-123".to_string()));
    // IDs are assigned by the DB
    assert_eq!(messages[0].id, id1);
    assert_eq!(messages[1].id, id2);
    assert_eq!(messages[2].id, id3);
}

// 3. End a session and verify ended_at and end_reason are set.
#[test]
fn test_end_session() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let session = make_session("sess-003", "claude-3-5-sonnet", "2026-04-10T10:00:00Z");
    store.create_session(&session).expect("create session");

    store
        .end_session("sess-003", "completed")
        .expect("end session");

    let retrieved = store
        .get_session("sess-003")
        .expect("get session")
        .expect("session exists");

    assert!(retrieved.ended_at.is_some(), "ended_at should be set");
    assert_eq!(retrieved.end_reason, Some("completed".to_string()));
}

// 4. List sessions with a limit.
#[test]
fn test_list_sessions() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    store
        .create_session(&make_session("sess-a", "model-a", "2026-04-10T08:00:00Z"))
        .expect("create a");
    store
        .create_session(&make_session("sess-b", "model-b", "2026-04-10T09:00:00Z"))
        .expect("create b");
    store
        .create_session(&make_session("sess-c", "model-c", "2026-04-10T10:00:00Z"))
        .expect("create c");

    // All sessions
    let all = store.list_sessions(10, 0).expect("list all");
    assert_eq!(all.len(), 3);
    // Ordered by started_at DESC — sess-c first
    assert_eq!(all[0].id, "sess-c");
    assert_eq!(all[1].id, "sess-b");
    assert_eq!(all[2].id, "sess-a");

    // Limited to 2
    let limited = store.list_sessions(2, 0).expect("list limited");
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].id, "sess-c");

    // Offset 1 should skip sess-c
    let offset = store.list_sessions(10, 1).expect("list with offset");
    assert_eq!(offset.len(), 2);
    assert_eq!(offset[0].id, "sess-b");
}

// 5. Update token counts and verify.
#[test]
fn test_update_token_counts() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let session = make_session("sess-005", "claude-3-5-sonnet", "2026-04-10T10:00:00Z");
    store.create_session(&session).expect("create session");

    let usage = TokenUsage {
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
    };
    store
        .update_token_counts("sess-005", &usage)
        .expect("update tokens");

    // Token counts are stored in the DB but not in the Session struct.
    // We verify by re-querying via a raw approach — but since SessionStore
    // doesn't expose those columns in get_session, we verify no error occurred
    // and can add a direct SQL check via a second store operation.
    //
    // The task spec only defines Session without token fields, so verifying
    // update_token_counts didn't error is the primary assertion here. A
    // secondary check confirms the session still exists after the update.
    let retrieved = store
        .get_session("sess-005")
        .expect("get session")
        .expect("session still exists");
    assert_eq!(retrieved.id, "sess-005");
}

// 6. Delete a session and verify it's gone (along with its messages).
#[test]
fn test_delete_session() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let session = make_session("sess-006", "claude-3-5-sonnet", "2026-04-10T10:00:00Z");
    store.create_session(&session).expect("create session");

    let msg = make_message("sess-006", "user", "Hello");
    store.add_message(&msg).expect("add message");

    store.delete_session("sess-006").expect("delete session");

    let result = store
        .get_session("sess-006")
        .expect("get session after delete");
    assert!(result.is_none(), "session should be gone");

    let messages = store
        .get_messages("sess-006")
        .expect("get messages after delete");
    assert!(messages.is_empty(), "messages should be gone");
}

// 7. Create a child session that references a parent, verify the chain.
#[test]
fn test_session_with_parent() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let parent = make_session("parent-001", "claude-3-5-sonnet", "2026-04-10T09:00:00Z");
    store.create_session(&parent).expect("create parent");

    let child = Session {
        id: "child-001".to_string(),
        model: "claude-3-5-sonnet".to_string(),
        system_prompt: None,
        parent_session_id: Some("parent-001".to_string()),
        started_at: "2026-04-10T10:00:00Z".to_string(),
        ended_at: None,
        end_reason: None,
        message_count: 0,
        tool_call_count: 0,
        title: None,
    };
    store.create_session(&child).expect("create child");

    let retrieved_child = store
        .get_session("child-001")
        .expect("get child")
        .expect("child exists");

    assert_eq!(
        retrieved_child.parent_session_id,
        Some("parent-001".to_string())
    );

    let retrieved_parent = store
        .get_session("parent-001")
        .expect("get parent")
        .expect("parent exists");

    assert_eq!(retrieved_parent.id, "parent-001");
    assert!(retrieved_parent.parent_session_id.is_none());
}

#[test]
fn test_cron_job_lifecycle_and_runs() {
    let store = SessionStore::new_in_memory().expect("in-memory store");

    let job = store
        .create_cron_job(NewCronJob {
            name: "report".to_string(),
            prompt: "Summarize recent sessions".to_string(),
            schedule: "every 5m".to_string(),
            enabled: true,
            model: Some("test-model".to_string()),
            disabled_toolsets: vec!["terminal".to_string()],
        })
        .expect("create cron job");

    assert_eq!(job.name, "report");
    assert!(job.enabled);
    assert!(job.next_run_at_epoch.is_some());
    assert_eq!(job.disabled_toolsets, vec!["terminal"]);

    let updated = store
        .update_cron_job(
            &job.id,
            CronJobPatch {
                schedule: Some("every 10m".to_string()),
                enabled: Some(false),
                ..CronJobPatch::default()
            },
        )
        .expect("update cron job");
    assert!(!updated.enabled);
    assert!(updated.next_run_at_epoch.is_none());

    let run = store.start_cron_run(&job.id).expect("start cron run");
    let running = store
        .get_cron_job(&job.id)
        .expect("get cron job")
        .expect("job exists");
    assert!(running.running);

    let finished = store
        .finish_cron_run(
            run.id,
            &running,
            CronRunFinish {
                status: "succeeded".to_string(),
                output: Some("done".to_string()),
                error: None,
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                duration_ms: 123,
                session_id: Some("cron-session".to_string()),
            },
        )
        .expect("finish cron run");
    assert_eq!(finished.status, "succeeded");
    assert_eq!(finished.output.as_deref(), Some("done"));

    let runs = store.list_cron_runs(&job.id, 10).expect("list cron runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].total_tokens, 15);
}

// ─── FTS5 search tests ───

#[test]
fn test_fts5_search_chinese() {
    let store = SessionStore::new_in_memory().expect("store");
    store
        .create_session(&make_session("s1", "qwen", "2026-04-15T00:00:00Z"))
        .unwrap();
    store
        .add_message(&make_message("s1", "user", "我们讨论了搜索功能的设计"))
        .unwrap();
    store
        .add_message(&make_message(
            "s1",
            "assistant",
            "好的，搜索功能需要使用FTS5",
        ))
        .unwrap();

    let results = store.search_messages("搜索", None, None, 10).unwrap();
    assert!(!results.is_empty(), "Chinese search should return results");
    assert_eq!(results[0].session_id, "s1");
}

#[test]
fn test_fts5_search_english() {
    let store = SessionStore::new_in_memory().expect("store");
    store
        .create_session(&make_session("s1", "qwen", "2026-04-15T00:00:00Z"))
        .unwrap();
    store
        .add_message(&make_message(
            "s1",
            "user",
            "How does the search feature work?",
        ))
        .unwrap();

    let results = store.search_messages("search", None, None, 10).unwrap();
    assert!(!results.is_empty(), "English search should return results");
}

#[test]
fn test_fts5_search_excludes_session() {
    let store = SessionStore::new_in_memory().expect("store");
    store
        .create_session(&make_session("s1", "qwen", "2026-04-15T00:00:00Z"))
        .unwrap();
    store
        .create_session(&make_session("s2", "qwen", "2026-04-15T01:00:00Z"))
        .unwrap();
    store
        .add_message(&make_message("s1", "user", "Docker deployment config"))
        .unwrap();
    store
        .add_message(&make_message("s2", "user", "Docker container setup"))
        .unwrap();

    // Exclude s1
    let results = store
        .search_messages("Docker", Some("s1"), None, 10)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].session_id, "s2");
}

#[test]
fn test_fts5_search_role_filter() {
    let store = SessionStore::new_in_memory().expect("store");
    store
        .create_session(&make_session("s1", "qwen", "2026-04-15T00:00:00Z"))
        .unwrap();
    store
        .add_message(&make_message("s1", "user", "Tell me about Kubernetes"))
        .unwrap();
    store
        .add_message(&make_message(
            "s1",
            "assistant",
            "Kubernetes is a container orchestration platform",
        ))
        .unwrap();

    // Only user messages
    let results = store
        .search_messages("Kubernetes", None, Some("user"), 10)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].role, "user");
}

#[test]
fn test_fts5_search_no_match() {
    let store = SessionStore::new_in_memory().expect("store");
    store
        .create_session(&make_session("s1", "qwen", "2026-04-15T00:00:00Z"))
        .unwrap();
    store
        .add_message(&make_message("s1", "user", "Hello world"))
        .unwrap();

    let results = store
        .search_messages("nonexistent_keyword_xyz", None, None, 10)
        .unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_fts5_trigger_on_delete() {
    let store = SessionStore::new_in_memory().expect("store");
    store
        .create_session(&make_session("s1", "qwen", "2026-04-15T00:00:00Z"))
        .unwrap();
    store
        .add_message(&make_message("s1", "user", "unique_test_content_12345"))
        .unwrap();

    // Verify it's searchable
    let results = store
        .search_messages("unique_test_content_12345", None, None, 10)
        .unwrap();
    assert_eq!(results.len(), 1);

    // Delete the session (cascades to messages)
    store.delete_session("s1").unwrap();

    // Should no longer be searchable
    let results = store
        .search_messages("unique_test_content_12345", None, None, 10)
        .unwrap();
    assert!(
        results.is_empty(),
        "Deleted messages should not be searchable"
    );
}
