use iron_core::{
    context_compressor::{CompressorConfig, ContextCompressor, PRUNED_PLACEHOLDER},
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
