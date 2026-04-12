use iron_core::{
    context_compressor::{
        COMPACTION_PREFIX, CompressorConfig, ContextCompressor, PRUNED_PLACEHOLDER,
    },
    llm::types::{FunctionCall, Message, ToolCall},
};

fn make_message(role: &str, content: &str) -> Message {
    Message {
        role: role.to_string(),
        content: Some(content.to_string()),
        tool_calls: None,
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

fn make_config(context_length: u64, threshold: f64, target_ratio: f64) -> CompressorConfig {
    CompressorConfig {
        context_length,
        threshold,
        target_ratio,
        protect_first_n: 3,
        auxiliary_llm: None,
    }
}

// ── should_compress ───────────────────────────────────────────────────────────

#[test]
fn test_should_compress_below_threshold() {
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.20));
    // threshold = 65_000; tokens well below
    assert!(!compressor.should_compress(64_999));
    assert!(!compressor.should_compress(0));
}

#[test]
fn test_should_compress_at_threshold() {
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.20));
    // threshold = 65_000
    assert!(compressor.should_compress(65_000));
    assert!(compressor.should_compress(65_001));
    assert!(compressor.should_compress(200_000));
}

#[test]
fn test_should_compress_various_thresholds() {
    // (context_length, threshold, prompt_tokens, expected)
    let cases: &[(u64, f64, u64, bool)] = &[
        (200_000, 0.50, 99_999, false),
        (200_000, 0.50, 100_000, true),
        (200_000, 0.50, 100_001, true),
        (128_000, 0.80, 102_399, false),
        (128_000, 0.80, 102_400, true),
        (128_000, 0.95, 121_599, false),
        (128_000, 0.95, 121_600, true),
        (32_000, 0.65, 20_799, false),
        (32_000, 0.65, 20_800, true),
    ];

    for &(context_length, threshold, prompt_tokens, expected) in cases {
        let compressor = ContextCompressor::new(make_config(context_length, threshold, 0.20));
        assert_eq!(
            compressor.should_compress(prompt_tokens),
            expected,
            "context={context_length}, threshold={threshold}, tokens={prompt_tokens}"
        );
    }
}

// ── threshold_tokens ─────────────────────────────────────────────────────────

#[test]
fn test_threshold_tokens_calculation() {
    let cases: &[(u64, f64, u64)] = &[
        (100_000, 0.65, 65_000),
        (128_000, 0.80, 102_400),
        (200_000, 0.50, 100_000),
        (32_000, 0.95, 30_400),
    ];

    for &(context_length, threshold, expected) in cases {
        let compressor = ContextCompressor::new(make_config(context_length, threshold, 0.20));
        assert_eq!(
            compressor.threshold_tokens(),
            expected,
            "context={context_length}, threshold={threshold}"
        );
    }
}

// ── tail_token_budget ────────────────────────────────────────────────────────

#[test]
fn test_tail_token_budget_calculation() {
    let cases: &[(u64, f64, f64, u64)] = &[
        // context, threshold, target_ratio, expected_budget
        (100_000, 0.65, 0.20, 13_000), // 65_000 * 0.20
        (128_000, 0.80, 0.20, 20_480), // 102_400 * 0.20
        (200_000, 0.50, 0.25, 25_000), // 100_000 * 0.25
        (32_000, 0.65, 0.10, 2_080),   // 20_800 * 0.10
    ];

    for &(context_length, threshold, target_ratio, expected) in cases {
        let compressor =
            ContextCompressor::new(make_config(context_length, threshold, target_ratio));
        assert_eq!(
            compressor.tail_token_budget(),
            expected,
            "context={context_length}, threshold={threshold}, ratio={target_ratio}"
        );
    }
}

// ── estimate_message_tokens ──────────────────────────────────────────────────

#[test]
fn test_estimate_message_tokens_content_only() {
    // "hello" = 5 chars → 5/4 + 10 = 11 tokens; + overhead 10 → 21
    let msg = Message {
        role: "user".to_string(),
        content: Some("hello".to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };
    assert_eq!(ContextCompressor::estimate_message_tokens(&msg), 21);
}

#[test]
fn test_estimate_message_tokens_with_tool_calls() {
    let args = "a".repeat(40); // 40 chars → 40/4+10 = 20 tokens
    let msg = Message {
        role: "assistant".to_string(),
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "tc1".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "some_tool".to_string(),
                arguments: args,
            },
        }]),
        tool_call_id: None,
        name: None,
    };
    // content: 0, tool_call args: 20, overhead: 10 → 30
    assert_eq!(ContextCompressor::estimate_message_tokens(&msg), 30);
}

#[test]
fn test_estimate_message_tokens_empty() {
    let msg = Message {
        role: "assistant".to_string(),
        content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };
    // only overhead
    assert_eq!(ContextCompressor::estimate_message_tokens(&msg), 10);
}

// ── prune_old_tool_results ───────────────────────────────────────────────────

#[test]
fn test_prune_old_tool_results_replaces_large() {
    // tool result >200 chars before tail_start gets replaced with placeholder
    let large_content = "x".repeat(201);
    let mut messages = vec![
        make_message("user", "hello"),
        make_tool_result(&large_content, "call_1"),
        make_message("assistant", "done"),
    ];
    // tail_start beyond all messages — all are in the prune zone
    ContextCompressor::prune_old_tool_results(&mut messages, 10);
    assert_eq!(messages[1].content.as_deref(), Some(PRUNED_PLACEHOLDER));
}

#[test]
fn test_prune_old_tool_results_keeps_small() {
    // tool result <=200 chars should NOT be replaced
    let small_content = "x".repeat(200);
    let mut messages = vec![
        make_message("user", "hello"),
        make_tool_result(&small_content, "call_1"),
        make_message("assistant", "done"),
    ];
    ContextCompressor::prune_old_tool_results(&mut messages, 10);
    assert_eq!(messages[1].content.as_deref(), Some(small_content.as_str()));
}

#[test]
fn test_prune_old_tool_results_protects_tail() {
    // tool result in tail (>= tail_start) is NOT replaced even if large
    let large_content = "x".repeat(201);
    let mut messages = vec![
        make_message("user", "hello"),
        make_message("assistant", "thinking"),
        // index 2 — tail starts here
        make_tool_result(&large_content, "call_1"),
        make_message("assistant", "done"),
    ];
    // tail_start = 2, so only messages[0..2] are in the prune zone
    ContextCompressor::prune_old_tool_results(&mut messages, 2);
    // messages[2] is in the tail, should be untouched
    assert_eq!(messages[2].content.as_deref(), Some(large_content.as_str()));
}

// ── find_boundaries ──────────────────────────────────────────────────────────

fn make_tool_call_msg(content: &str, call_id: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: Some(content.to_string()),
        tool_calls: Some(vec![ToolCall {
            id: call_id.to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "test".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        tool_call_id: None,
        name: None,
    }
}

#[test]
fn test_find_boundaries_basic() {
    // 10 messages: protect_first_n=3, verify head >= 3, tail < len, middle exists
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.20));
    let messages: Vec<Message> = (0..10)
        .map(|i| {
            if i % 2 == 0 {
                make_message("user", &"hello world ".repeat(50))
            } else {
                make_message("assistant", &"response here ".repeat(50))
            }
        })
        .collect();

    let boundary = compressor.find_boundaries(&messages);
    // head must include at least protect_first_n=3 messages
    assert!(boundary.head_end >= 3, "head_end={}", boundary.head_end);
    // tail must not include all messages
    assert!(
        boundary.tail_start < messages.len(),
        "tail_start={}",
        boundary.tail_start
    );
    // there must be a compressible middle
    assert!(
        boundary.head_end < boundary.tail_start,
        "no middle: head_end={} tail_start={}",
        boundary.head_end,
        boundary.tail_start
    );
}

#[test]
fn test_find_boundaries_tool_pair_not_split() {
    // Messages structured so that the natural tail boundary would land on a tool result.
    // The alignment step should walk back to include the assistant tool_call message too.
    //
    // Layout (protect_first_n=3):
    //   0: user
    //   1: assistant
    //   2: user
    //   3: assistant (tool_call)
    //   4: tool result
    //   5: assistant (tail)
    //   6: user (tail)
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.05));
    let messages = vec![
        make_message("user", "msg0"),
        make_message("assistant", "msg1"),
        make_message("user", "msg2"),
        make_tool_call_msg("calling tool", "call_1"),
        make_tool_result("tool output", "call_1"),
        make_message("assistant", "after tool"),
        make_message("user", "last question"),
    ];

    let boundary = compressor.find_boundaries(&messages);
    // If the boundary is compressible, the tail must not start in the middle of a
    // tool_call / tool pair — i.e. tail_start must not point to a "tool" message
    // while the previous message is an assistant with tool_calls.
    if boundary.head_end < boundary.tail_start {
        let ts = boundary.tail_start;
        // tail_start must not be a "tool" message whose predecessor is an "assistant"
        // with tool_calls (that would split the pair).
        if messages[ts].role == "tool" {
            assert!(
                ts == 0 || messages[ts - 1].tool_calls.is_none(),
                "tail boundary splits a tool_call/tool pair at index {}",
                ts
            );
        }
    }
}

#[test]
fn test_find_boundaries_too_few_messages() {
    // Only 5 messages with protect_first_n=3: not enough for a compressible middle.
    // Need at least protect_first_n + 4 = 7 messages.
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.20));
    let messages: Vec<Message> = (0..5)
        .map(|i| {
            if i % 2 == 0 {
                make_message("user", "hello")
            } else {
                make_message("assistant", "world")
            }
        })
        .collect();

    let boundary = compressor.find_boundaries(&messages);
    // No compressible middle: head_end >= tail_start
    assert!(
        boundary.head_end >= boundary.tail_start,
        "expected no compressible middle but got head_end={} tail_start={}",
        boundary.head_end,
        boundary.tail_start
    );
}

// ── sanitize_tool_pairs ──────────────────────────────────────────────────────

fn make_tool_call_message(call_id: &str, fn_name: &str) -> Message {
    Message {
        role: "assistant".to_string(),
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: call_id.to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: fn_name.to_string(),
                arguments: "{}".to_string(),
            },
        }]),
        tool_call_id: None,
        name: None,
    }
}

#[test]
fn test_sanitize_tool_pairs_removes_orphan_result() {
    // A tool result whose tool_call_id has no matching assistant tool_call
    // should be removed.
    let mut messages = vec![
        make_message("user", "hello"),
        make_message("assistant", "ok"),
        // orphan result — no assistant message references "orphan_id"
        make_tool_result("some result", "orphan_id"),
        make_message("user", "next"),
    ];

    ContextCompressor::sanitize_tool_pairs(&mut messages);

    // The orphan tool result should have been removed
    assert_eq!(messages.len(), 3);
    assert!(
        messages.iter().all(|m| m.role != "tool"),
        "orphan tool result should be removed"
    );
}

#[test]
fn test_sanitize_tool_pairs_injects_stub_for_orphan_call() {
    // An assistant tool_call with no corresponding tool result should get a stub injected.
    let mut messages = vec![
        make_message("user", "call a tool"),
        make_tool_call_message("call_abc", "my_tool"),
        // No tool result for "call_abc"
        make_message("user", "next"),
    ];

    ContextCompressor::sanitize_tool_pairs(&mut messages);

    // A stub result should now appear right after the assistant message (index 1)
    assert_eq!(messages.len(), 4, "stub should have been inserted");
    let stub = &messages[2];
    assert_eq!(stub.role, "tool");
    assert_eq!(stub.tool_call_id.as_deref(), Some("call_abc"));
    assert_eq!(stub.name.as_deref(), Some("my_tool"));
    assert_eq!(
        stub.content.as_deref(),
        Some(iron_core::context_compressor::STUB_TOOL_RESULT)
    );
}

// ── assemble ─────────────────────────────────────────────────────────────────

#[test]
fn test_assemble_with_summary() {
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.20));

    let head = vec![
        make_message("user", "msg1"),
        make_message("assistant", "msg2"),
        make_message("user", "msg3"),
    ];
    let tail = vec![
        make_message("assistant", "msg4"),
        make_message("user", "msg5"),
    ];
    let summary = Some("This is the summary.".to_string());

    let result = compressor.assemble(&head, summary, &tail);

    // 3 head + 1 summary + 2 tail = 6
    assert_eq!(result.len(), 6, "expected 6 messages");

    // The summary message should be at index 3
    let summary_msg = &result[3];
    assert!(
        summary_msg
            .content
            .as_deref()
            .unwrap_or("")
            .starts_with(COMPACTION_PREFIX),
        "summary message should start with COMPACTION_PREFIX"
    );
    assert!(
        summary_msg
            .content
            .as_deref()
            .unwrap_or("")
            .contains("This is the summary."),
        "summary message should contain the summary text"
    );
}

#[test]
fn test_assemble_without_summary() {
    let compressor = ContextCompressor::new(make_config(100_000, 0.65, 0.20));

    let head = vec![
        make_message("user", "msg1"),
        make_message("assistant", "msg2"),
    ];
    let tail = vec![make_message("user", "msg3")];

    let result = compressor.assemble(&head, None, &tail);

    // 2 head + 0 summary + 1 tail = 3
    assert_eq!(result.len(), 3, "expected 3 messages with no summary");
    // No summary message present
    assert!(
        result.iter().all(|m| !m
            .content
            .as_deref()
            .unwrap_or("")
            .contains(COMPACTION_PREFIX)),
        "no message should contain COMPACTION_PREFIX when summary is None"
    );
}
