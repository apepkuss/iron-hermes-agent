use iron_core::{
    context_compressor::{AuxiliaryLlmConfig, CompressorConfig, ContextCompressor},
    llm::types::{FunctionCall, Message, ToolCall},
};

// ── Helper functions ──────────────────────────────────────────────────────────

fn make_message(role: &str, content: &str) -> Message {
    Message {
        role: role.to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

fn make_tool_call_msg(content: &str, call_id: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: Some(content.to_string()),
        tool_calls: Some(vec![ToolCall {
            id: call_id.to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "test_tool".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        tool_call_id: None,
        name: None,
    }
}

fn make_tool_result(content: &str, call_id: &str) -> Message {
    Message {
        role: "tool".to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
        tool_call_id: Some(call_id.to_string()),
        name: Some("test_tool".to_string()),
    }
}

fn total_estimated_tokens(messages: &[Message]) -> u64 {
    messages
        .iter()
        .map(ContextCompressor::estimate_message_tokens)
        .sum()
}

// ── Group 1: Basic Effectiveness (no LLM needed) ─────────────────────────────

#[tokio::test]
async fn test_effectiveness_token_reduction_rate() {
    let config = CompressorConfig {
        context_length: 2000,
        threshold: 0.50,
        target_ratio: 0.30,
        protect_first_n: 3,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    // 15 messages, each with 200-char content (~50 tokens each, total ~750 tokens)
    let messages: Vec<Message> = (0..15)
        .map(|i| {
            if i % 2 == 0 {
                make_message("user", &"u".repeat(200))
            } else {
                make_message("assistant", &"a".repeat(200))
            }
        })
        .collect();

    let original_tokens = total_estimated_tokens(&messages);
    let result = compressor.compress(&messages, 1000).await;
    let result_tokens = total_estimated_tokens(&result);

    assert!(
        result_tokens < original_tokens,
        "compressed tokens ({result_tokens}) should be less than original ({original_tokens})"
    );

    let reduction_rate = 1.0 - (result_tokens as f64 / original_tokens as f64);
    assert!(
        reduction_rate > 0.30,
        "reduction rate {:.1}% should be > 30%",
        reduction_rate * 100.0
    );
}

#[tokio::test]
async fn test_effectiveness_large_tool_result_scenario() {
    // Simulates the real skills_list problem (77K tokens).
    //
    // The large tool result (3000-char, ~770 tokens) must land in the MIDDLE zone
    // so it gets pruned.  We ensure this by using a small target_ratio (0.05) so
    // the tail budget is tiny (~62 tokens), which stops the backward walk before
    // it reaches the large result — the tail then only contains the last few short
    // messages.
    let config = CompressorConfig {
        context_length: 5000,
        threshold: 0.50,
        target_ratio: 0.05, // tail budget = 2500 * 0.05 = 125 tokens
        protect_first_n: 2,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    let large_tool_result = "s".repeat(3000); // simulating skills_list output
    let medium_tool_result = "v".repeat(500); // simulating skill_view

    // Tail messages are short enough that 3 of them accumulate well under 770 tokens,
    // so the backward walk stops before reaching the large tool result.
    let messages = vec![
        make_message("system", "You are a helpful assistant"),
        make_message("user", "list skills"),
        make_tool_call_msg("Listing skills...", "call_list"),
        make_tool_result(&large_tool_result, "call_list"),
        make_message(
            "assistant",
            "Here are 122 skills available in the system...",
        ),
        make_message("user", "tell me about arxiv"),
        make_tool_call_msg("Viewing arxiv skill...", "call_view"),
        make_tool_result(&medium_tool_result, "call_view"),
        make_message(
            "assistant",
            "arxiv is a skill that allows searching academic papers...",
        ),
        make_message("user", "thanks, now something else"), // short tail msg
        make_message("assistant", "sure, what would you like to do?"), // short tail msg
    ];

    let original_tokens = total_estimated_tokens(&messages);
    let result = compressor.compress(&messages, 2500).await;
    let result_tokens = total_estimated_tokens(&result);

    // The 3000-char tool result must not appear verbatim in compressed output.
    // In degraded mode (no LLM) the entire middle is dropped; with LLM it would
    // be replaced with PRUNED_PLACEHOLDER before summarisation.
    let large_content_present = result.iter().any(|m| {
        m.content
            .as_deref()
            .map_or(false, |c| c == large_tool_result.as_str())
    });
    assert!(
        !large_content_present,
        "The 3000-char tool result should not appear verbatim in the compressed output"
    );

    // Total tokens should be significantly reduced
    assert!(
        result_tokens < original_tokens,
        "result tokens ({result_tokens}) should be less than original ({original_tokens})"
    );
    let reduction = original_tokens - result_tokens;
    assert!(
        reduction > original_tokens / 4,
        "token reduction ({reduction}) should be >25% of original ({original_tokens})"
    );

    // Recent messages (last 2) preserved intact
    let n = messages.len();
    let last_original: Vec<&str> = messages[n - 2..]
        .iter()
        .filter_map(|m| m.content.as_deref())
        .collect();

    let result_contents: Vec<Option<&str>> = result.iter().map(|m| m.content.as_deref()).collect();

    for expected in &last_original {
        assert!(
            result_contents.iter().any(|c| *c == Some(expected)),
            "Recent message content '{expected}' should be preserved in result"
        );
    }
}

#[tokio::test]
async fn test_effectiveness_head_protection() {
    let config = CompressorConfig {
        context_length: 2000,
        threshold: 0.50,
        target_ratio: 0.20,
        protect_first_n: 3,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    let messages: Vec<Message> = (0..12)
        .map(|i| {
            if i % 2 == 0 {
                make_message("user", &format!("user message {} {}", i, "x".repeat(100)))
            } else {
                make_message(
                    "assistant",
                    &format!("assistant response {} {}", i, "y".repeat(100)),
                )
            }
        })
        .collect();

    let result = compressor.compress(&messages, 1000).await;

    // First 3 messages (head) must be exactly preserved
    for i in 0..3 {
        assert_eq!(
            result[i].content, messages[i].content,
            "Head message {i} content should be byte-identical"
        );
        assert_eq!(
            result[i].role, messages[i].role,
            "Head message {i} role should be preserved"
        );
    }

    // System prompt (message 0) is byte-identical
    assert_eq!(
        result[0].content.as_deref(),
        messages[0].content.as_deref(),
        "First message should be byte-identical"
    );
}

#[tokio::test]
async fn test_effectiveness_tail_protection() {
    let config = CompressorConfig {
        context_length: 2000,
        threshold: 0.50,
        target_ratio: 0.30,
        protect_first_n: 3,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    let mut messages: Vec<Message> = (0..12)
        .map(|i| {
            if i % 2 == 0 {
                make_message("user", &format!("generic user msg {i} {}", "u".repeat(80)))
            } else {
                make_message(
                    "assistant",
                    &format!("generic assistant msg {i} {}", "a".repeat(80)),
                )
            }
        })
        .collect();

    // Last 3 messages with distinctive content
    messages.push(make_message("user", "TAIL_MSG_1 distinctive content here"));
    messages.push(make_message(
        "assistant",
        "TAIL_MSG_2 distinctive response here",
    ));
    messages.push(make_message("user", "TAIL_MSG_3 distinctive final message"));

    let result = compressor.compress(&messages, 1000).await;

    let result_contents: Vec<&str> = result.iter().filter_map(|m| m.content.as_deref()).collect();

    for tail_marker in &["TAIL_MSG_1", "TAIL_MSG_2", "TAIL_MSG_3"] {
        let found = result_contents.iter().any(|c| c.contains(tail_marker));
        assert!(
            found,
            "Tail message '{tail_marker}' should be preserved in result"
        );
    }
}

#[tokio::test]
async fn test_effectiveness_tool_pair_integrity() {
    let config = CompressorConfig {
        context_length: 5000,
        threshold: 0.50,
        target_ratio: 0.20,
        protect_first_n: 3,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    // Build messages with tool_call/result pairs scattered throughout:
    // Pair 1 (in head region, index 1-2)
    // Pair 2 (in middle, will be compressed)
    // Pair 3 (in tail)
    let messages = vec![
        make_message("system", "You are a helpful assistant"),
        make_tool_call_msg("calling head tool", "call_head"),
        make_tool_result("head tool result", "call_head"),
        make_message("user", "now let's do more work"),
        // Middle pair (will likely be compressed)
        make_tool_call_msg("calling middle tool", "call_mid"),
        make_tool_result(&"m".repeat(300), "call_mid"),
        make_message("assistant", "middle work done"),
        make_message("user", "continue"),
        make_message("assistant", "continuing"),
        make_message("user", "more work"),
        make_message("assistant", "doing more"),
        // Tail pair
        make_tool_call_msg("calling tail tool", "call_tail"),
        make_tool_result("tail tool result", "call_tail"),
        make_message("assistant", "tail work done"),
        make_message("user", "final question"),
    ];

    let result = compressor.compress(&messages, 2500).await;

    // Collect all call IDs produced by assistant tool_calls in result
    let call_ids: std::collections::HashSet<String> = result
        .iter()
        .filter_map(|m| m.tool_calls.as_deref())
        .flatten()
        .map(|tc| tc.id.clone())
        .collect();

    // Every tool result message must have a matching assistant tool_call
    for msg in &result {
        if msg.role == "tool" {
            if let Some(ref id) = msg.tool_call_id {
                assert!(
                    call_ids.contains(id),
                    "tool result references unknown call_id={id}"
                );
            }
        }
    }

    // Every tool_call must have a matching tool result (real or stub)
    let result_ids: std::collections::HashSet<String> = result
        .iter()
        .filter(|m| m.role == "tool")
        .filter_map(|m| m.tool_call_id.clone())
        .collect();

    for msg in &result {
        if let Some(tool_calls) = msg.tool_calls.as_deref() {
            for tc in tool_calls {
                assert!(
                    result_ids.contains(&tc.id),
                    "tool_call id={} has no matching tool result",
                    tc.id
                );
            }
        }
    }
}

#[tokio::test]
async fn test_effectiveness_multi_round_compression() {
    let config = CompressorConfig {
        context_length: 1000,
        threshold: 0.50,
        target_ratio: 0.20,
        protect_first_n: 2,
        auxiliary_llm: None,
    };
    let mut compressor = ContextCompressor::new(config);

    // Build 10 messages for round 1
    let messages: Vec<Message> = (0..10)
        .map(|i| {
            if i % 2 == 0 {
                make_message("user", &format!("round1 user msg {i} {}", "u".repeat(100)))
            } else {
                make_message(
                    "assistant",
                    &format!("round1 assistant msg {i} {}", "a".repeat(100)),
                )
            }
        })
        .collect();

    // Round 1 compression
    let mut compressed = compressor.compress(&messages, 500).await;
    assert_eq!(
        compressor.compression_count(),
        1,
        "compression_count should be 1 after round 1"
    );

    // Append 8 more messages to the compressed result
    for i in 0..8 {
        if i % 2 == 0 {
            compressed.push(make_message(
                "user",
                &format!("round2 user msg {i} {}", "u".repeat(100)),
            ));
        } else {
            compressed.push(make_message(
                "assistant",
                &format!("round2 assistant msg {i} {}", "a".repeat(100)),
            ));
        }
    }

    // Round 2 compression
    let result = compressor.compress(&compressed, 500).await;

    assert_eq!(
        compressor.compression_count(),
        2,
        "compression_count should be 2 after round 2"
    );

    // Result should still have valid structure: head preserved
    assert!(
        result.len() >= 2,
        "result should have at least head messages"
    );

    // Head messages (first 2) should still be from original conversation
    assert!(
        result[0]
            .content
            .as_deref()
            .map_or(false, |c| c.contains("round1")),
        "First message should be from original round1 conversation"
    );

    // Total messages should be fewer than 18 (10 + 8 without any compression)
    assert!(
        result.len() < 18,
        "result ({} messages) should be fewer than uncompressed total (18)",
        result.len()
    );

    // No orphan tool pairs in result
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
                    "orphan tool result references unknown call_id={id}"
                );
            }
        }
    }
}

// ── Group 2: Summary Effectiveness (needs LLM) ───────────────────────────────

#[tokio::test]
#[ignore] // Requires running LLM backend: cargo test -- --ignored
async fn test_summary_contains_key_information() {
    let config = CompressorConfig {
        context_length: 100_000,
        threshold: 0.01, // very low threshold to force compression
        target_ratio: 0.10,
        protect_first_n: 2,
        auxiliary_llm: Some(AuxiliaryLlmConfig {
            base_url: "http://localhost:9068/v1".to_string(),
            model: "mlx-community/Qwen3-8B-4bit".to_string(),
        }),
    };
    let mut compressor = ContextCompressor::new(config);

    let messages = vec![
        make_message("system", "You are a coding assistant"),
        make_message("user", "Fix the bug in src/auth/login.rs"),
        make_message(
            "assistant",
            "I found the issue. The function validate_token() on line 42 was not checking expiry. I'll fix it.",
        ),
        make_message("user", "Good. Also update the tests"),
        make_message(
            "assistant",
            "Done. I updated tests/auth_test.rs with a new test test_expired_token_rejected. All 5 tests pass now.",
        ),
        make_message("user", "Perfect. Now let's move on to the API rate limiter"),
        make_message("assistant", "Looking at src/middleware/rate_limit.rs..."),
        make_message("user", "What approach do you recommend?"),
        make_message(
            "assistant",
            "I recommend using a token bucket algorithm with Redis backend. This handles distributed rate limiting.",
        ),
        make_message("user", "Sounds good, implement it"),
        make_message("assistant", &"x".repeat(200)), // filler to push tokens
        make_message("user", "latest question about rate limiter"),
    ];

    let result = compressor.compress(&messages, 1500).await;

    // Find the compaction message
    let summary_msg = result.iter().find(|m| {
        m.content
            .as_deref()
            .map_or(false, |c| c.contains("CONTEXT COMPACTION"))
    });

    assert!(
        summary_msg.is_some(),
        "Should have a compaction summary message"
    );

    let summary = summary_msg.unwrap().content.as_deref().unwrap();

    // Summary should mention key artifacts
    let summary_lower = summary.to_lowercase();

    // Check for file references (at least one should be mentioned)
    let has_file_ref = summary_lower.contains("login.rs")
        || summary_lower.contains("auth")
        || summary_lower.contains("rate_limit");
    assert!(
        has_file_ref,
        "Summary should reference at least one file. Summary:\n{summary}"
    );
}

#[tokio::test]
#[ignore] // Requires running LLM backend
async fn test_summary_preserves_decisions() {
    let config = CompressorConfig {
        context_length: 100_000,
        threshold: 0.01,
        target_ratio: 0.10,
        protect_first_n: 2,
        auxiliary_llm: Some(AuxiliaryLlmConfig {
            base_url: "http://localhost:9068/v1".to_string(),
            model: "mlx-community/Qwen3-8B-4bit".to_string(),
        }),
    };
    let mut compressor = ContextCompressor::new(config);

    let messages = vec![
        make_message("system", "You are helpful"),
        make_message("user", "Should we use PostgreSQL or SQLite?"),
        make_message(
            "assistant",
            "I recommend SQLite for this project because: 1) No server setup needed 2) Single file database 3) Good enough for our expected load of under 1000 requests/day",
        ),
        make_message("user", "Agreed, let's go with SQLite"),
        make_message(
            "assistant",
            "Great. I'll create the schema in migrations/001_init.sql using SQLite syntax.",
        ),
        make_message("user", "Also what about the caching layer?"),
        make_message(
            "assistant",
            "For caching I suggest in-memory LRU cache using the lru crate, no need for Redis given our scale.",
        ),
        make_message("user", &"x".repeat(200)),
        make_message("assistant", &"y".repeat(200)),
        make_message("user", "continue with implementation"),
    ];

    let result = compressor.compress(&messages, 1500).await;

    let summary_msg = result.iter().find(|m| {
        m.content
            .as_deref()
            .map_or(false, |c| c.contains("CONTEXT COMPACTION"))
    });

    assert!(summary_msg.is_some(), "Should have summary");
    let summary = summary_msg
        .unwrap()
        .content
        .as_deref()
        .unwrap()
        .to_lowercase();

    // Key decision should be preserved
    let has_db_decision = summary.contains("sqlite");
    assert!(
        has_db_decision,
        "Summary should mention SQLite decision. Summary:\n{}",
        summary_msg.unwrap().content.as_deref().unwrap()
    );
}
