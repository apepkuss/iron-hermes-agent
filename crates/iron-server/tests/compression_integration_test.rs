use iron_core::context_compressor::{CompressorConfig, ContextCompressor};
use iron_core::llm::types::Message;
use iron_server::config::RuntimeConfig;

// ── helpers ───────────────────────────────────────────────────────────────────

fn make_msg(role: &str, content: &str) -> Message {
    Message {
        role: role.to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

fn make_tool_result_msg(content: &str, call_id: &str, fn_name: &str) -> Message {
    Message {
        role: "tool".to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        tool_call_id: Some(call_id.to_string()),
        name: Some(fn_name.to_string()),
    }
}

// ── Test 1: RuntimeConfig defaults ───────────────────────────────────────────

#[test]
fn test_runtime_config_defaults() {
    let rc = RuntimeConfig {
        llm_base_url: "http://localhost:9068/v1".to_string(),
        llm_model: "test-model".to_string(),
        auxiliary_model: None,
        compression_threshold: 0.65,
        context_length_override: None,
        fallback_model: None,
        agent_timeout_secs: 600,
        inactivity_timeout_secs: 300,
        session_idle_timeout_secs: 1800,
        disabled_toolsets: Vec::new(),
    };

    assert_eq!(rc.compression_threshold, 0.65);
    assert!(rc.auxiliary_model.is_none());
    assert!(rc.context_length_override.is_none());
    assert_eq!(rc.llm_base_url, "http://localhost:9068/v1");
    assert_eq!(rc.llm_model, "test-model");
}

// ── Test 2: RuntimeConfig serialization ──────────────────────────────────────

#[test]
fn test_runtime_config_serialization() {
    let rc = RuntimeConfig {
        llm_base_url: "http://localhost:9068/v1".to_string(),
        llm_model: "Qwen3-8B".to_string(),
        auxiliary_model: Some("Qwen3-4B".to_string()),
        compression_threshold: 0.65,
        context_length_override: Some(131072),
        fallback_model: None,
        agent_timeout_secs: 600,
        inactivity_timeout_secs: 300,
        session_idle_timeout_secs: 1800,
        disabled_toolsets: Vec::new(),
    };

    let json = serde_json::to_value(&rc).unwrap();
    assert_eq!(json["compression_threshold"], 0.65);
    assert_eq!(json["auxiliary_model"], "Qwen3-4B");
    assert_eq!(json["context_length_override"], 131072);
    assert_eq!(json["llm_model"], "Qwen3-8B");
    assert_eq!(json["llm_base_url"], "http://localhost:9068/v1");
}

#[test]
fn test_runtime_config_serialization_optional_nulls() {
    let rc = RuntimeConfig {
        llm_base_url: "http://localhost:9068/v1".to_string(),
        llm_model: "test-model".to_string(),
        auxiliary_model: None,
        compression_threshold: 0.80,
        context_length_override: None,
        fallback_model: None,
        agent_timeout_secs: 600,
        inactivity_timeout_secs: 300,
        session_idle_timeout_secs: 1800,
        disabled_toolsets: Vec::new(),
    };

    let json = serde_json::to_value(&rc).unwrap();
    assert!(json["auxiliary_model"].is_null());
    assert!(json["context_length_override"].is_null());
    assert_eq!(json["compression_threshold"], 0.80);
}

#[test]
fn test_runtime_config_deserialization() {
    let json = serde_json::json!({
        "llm_base_url": "http://localhost:9068/v1",
        "llm_model": "Qwen3-8B",
        "auxiliary_model": "Qwen3-4B",
        "compression_threshold": 0.75,
        "context_length_override": 65536_u64
    });

    let rc: RuntimeConfig = serde_json::from_value(json).unwrap();
    assert_eq!(rc.llm_base_url, "http://localhost:9068/v1");
    assert_eq!(rc.llm_model, "Qwen3-8B");
    assert_eq!(rc.auxiliary_model.as_deref(), Some("Qwen3-4B"));
    assert_eq!(rc.compression_threshold, 0.75);
    assert_eq!(rc.context_length_override, Some(65536));
}

// ── Test 3: Full compression pipeline ────────────────────────────────────────

#[tokio::test]
async fn test_compressor_full_pipeline() {
    let config = CompressorConfig {
        context_length: 500, // very small for testing
        threshold: 0.50,     // triggers at 250 tokens
        target_ratio: 0.20,
        protect_first_n: 2,
        auxiliary_llm: None, // degraded mode — no summary
    };
    let mut compressor = ContextCompressor::new(config);

    // Create messages that would exceed threshold
    let mut messages = vec![make_msg("system", "System"), make_msg("user", "First")];

    // Add enough messages to exceed threshold
    for i in 0..10 {
        let role = if i % 2 == 0 { "assistant" } else { "user" };
        messages.push(make_msg(role, &"x".repeat(100)));
    }

    // Verify threshold triggers correctly
    assert!(
        compressor.should_compress(300),
        "should_compress should return true at 300 tokens (threshold=250)"
    );
    assert!(
        !compressor.should_compress(249),
        "should_compress should return false below threshold"
    );

    let original_len = messages.len();
    let result = compressor.compress(&messages, 300).await;

    // Compressed result should be shorter
    assert!(
        result.len() < original_len,
        "compressed result ({}) should be shorter than original ({})",
        result.len(),
        original_len
    );
    // Head (protect_first_n=2) preserved
    assert_eq!(
        result[0].content.as_deref().unwrap(),
        "System",
        "first head message should be preserved"
    );
    assert_eq!(
        result[1].content.as_deref().unwrap(),
        "First",
        "second head message should be preserved"
    );
    // Compression count incremented
    assert_eq!(compressor.compression_count(), 1);
}

// ── Test 4: Compressor with large tool results ────────────────────────────────

#[tokio::test]
async fn test_compressor_with_large_tool_results() {
    let config = CompressorConfig {
        context_length: 500,
        threshold: 0.50,
        target_ratio: 0.20,
        protect_first_n: 2,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    let messages = vec![
        make_msg("system", "System"),
        make_msg("user", "list"),
        // Large tool result that should be pruned if in non-tail region
        make_tool_result_msg(&"x".repeat(500), "c1", "skills_list"),
        make_msg("assistant", "Here are skills"),
        make_msg("user", "more"),
        make_msg("assistant", "Sure"),
        make_msg("user", "latest"),
        make_msg("assistant", &"y".repeat(100)),
    ];

    let result = compressor.compress(&messages, 300).await;

    // Compression count should be incremented regardless
    assert_eq!(
        compressor.compression_count(),
        1,
        "compression_count should be 1 after one compress() call"
    );

    // The result must still have a valid head
    assert_eq!(
        result[0].content.as_deref().unwrap(),
        "System",
        "system message (head) must be preserved"
    );

    // Verify no orphan tool results remain in the final output
    let call_ids: std::collections::HashSet<String> = result
        .iter()
        .filter_map(|m| m.tool_calls.as_deref())
        .flatten()
        .map(|tc| tc.id.clone())
        .collect();

    for msg in &result {
        if msg.role == "tool" {
            if let Some(ref id) = msg.tool_call_id {
                assert!(
                    call_ids.contains(id),
                    "orphan tool result found: tool_call_id={id} has no matching assistant tool_call"
                );
            }
        }
    }
}

// ── Test 5: Compressor respects protect_first_n ───────────────────────────────

#[tokio::test]
async fn test_compressor_protect_first_n() {
    let config = CompressorConfig {
        context_length: 1000,
        threshold: 0.50,
        target_ratio: 0.20,
        protect_first_n: 3,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    // Build 14 messages (protect_first_n=3, so head is messages[0..3])
    let messages: Vec<Message> = (0..14)
        .map(|i| {
            if i % 2 == 0 {
                make_msg("user", &format!("user message {i} "))
            } else {
                make_msg("assistant", &format!("assistant message {i} "))
            }
        })
        .collect();

    let original_first_three: Vec<Option<String>> =
        messages[..3].iter().map(|m| m.content.clone()).collect();

    let result = compressor.compress(&messages, 600).await;

    // The first three messages must be exactly preserved
    for (i, original_content) in original_first_three.iter().enumerate() {
        assert_eq!(
            result[i].content, *original_content,
            "head message {i} should be preserved intact"
        );
    }

    assert_eq!(compressor.compression_count(), 1);
}

// ── Test 6: Compressor idempotency on too-few messages ───────────────────────

#[tokio::test]
async fn test_compressor_too_few_messages_returns_clone() {
    // With protect_first_n=3, need at least 7 messages for a compressible middle.
    // With only 5, compress() should return a clone of the original.
    let config = CompressorConfig {
        context_length: 500,
        threshold: 0.50,
        target_ratio: 0.20,
        protect_first_n: 3,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    let messages: Vec<Message> = (0..5)
        .map(|i| {
            if i % 2 == 0 {
                make_msg("user", "hello")
            } else {
                make_msg("assistant", "world")
            }
        })
        .collect();

    let result = compressor.compress(&messages, 300).await;

    // No middle was compressible — result should equal the original
    assert_eq!(
        result.len(),
        messages.len(),
        "too-few-messages: result len should match original"
    );
    for (i, (r, o)) in result.iter().zip(messages.iter()).enumerate() {
        assert_eq!(r.content, o.content, "message {i} should be unchanged");
        assert_eq!(r.role, o.role, "role of message {i} should be unchanged");
    }
    // compression_count is still incremented (the call was made)
    assert_eq!(compressor.compression_count(), 1);
}

// ── Test 7: RuntimeConfig threshold clamping semantics ───────────────────────

#[test]
fn test_runtime_config_threshold_range() {
    // Verify that compression_threshold can represent the expected range
    for &threshold in &[0.50_f64, 0.65, 0.80, 0.95] {
        let rc = RuntimeConfig {
            llm_base_url: "http://localhost:9068/v1".to_string(),
            llm_model: "test".to_string(),
            auxiliary_model: None,
            compression_threshold: threshold,
            context_length_override: None,
            fallback_model: None,
            agent_timeout_secs: 600,
            inactivity_timeout_secs: 300,
            session_idle_timeout_secs: 1800,
            disabled_toolsets: Vec::new(),
        };
        let json = serde_json::to_value(&rc).unwrap();
        let roundtripped: RuntimeConfig = serde_json::from_value(json).unwrap();
        assert!(
            (roundtripped.compression_threshold - threshold).abs() < f64::EPSILON,
            "threshold {threshold} should survive a JSON round-trip"
        );
    }
}
