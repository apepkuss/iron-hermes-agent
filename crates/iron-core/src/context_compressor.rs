use std::time::Instant;

use crate::llm::types::Message;

/// Prefix inserted before a generated context summary.
pub const COMPACTION_PREFIX: &str = "[CONTEXT COMPACTION] Earlier turns were compacted into the summary below.\nRefer to it for context on prior work.\n\n";
/// Stub content inserted for orphaned tool_calls that have no result.
pub const STUB_TOOL_RESULT: &str = "[Result from earlier conversation — see context summary above]";

#[derive(Debug, Clone)]
pub struct AuxiliaryLlmConfig {
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct CompressorConfig {
    /// Max context tokens for the primary LLM.
    pub context_length: u64,
    /// Compression trigger threshold (0.50–0.95), default 0.65.
    pub threshold: f64,
    /// Tail-protection ratio relative to threshold_tokens, default 0.20.
    pub target_ratio: f64,
    /// Number of leading messages to protect from compression, default 3.
    pub protect_first_n: usize,
    /// Optional auxiliary LLM used for summarisation.
    pub auxiliary_llm: Option<AuxiliaryLlmConfig>,
}

pub struct ContextCompressor {
    config: CompressorConfig,
    pub previous_summary: Option<String>,
    compression_count: u32,
    pub summary_cooldown_until: Option<Instant>,
}

/// Describes the head/tail protection boundaries for a compression pass.
#[derive(Debug, Clone)]
pub struct CompressionBoundary {
    /// Index of the first message NOT in the protected head (exclusive upper bound of head).
    pub head_end: usize,
    /// Index of the first message in the protected tail (inclusive lower bound of tail).
    pub tail_start: usize,
}

/// Tool-result messages longer than this (in chars) are candidates for pruning.
pub const TOOL_RESULT_PRUNE_THRESHOLD: usize = 200;
/// Placeholder inserted when an old tool output is pruned.
pub const PRUNED_PLACEHOLDER: &str = "[Old tool output cleared to save context space]";
/// After a summarisation failure, wait this many seconds before retrying.
pub const SUMMARY_FAILURE_COOLDOWN_SECS: u64 = 600;
/// Fraction of threshold_tokens to allocate for the summary itself.
pub const SUMMARY_RATIO: f64 = 0.20;
/// Minimum token budget for a generated summary.
pub const MIN_SUMMARY_TOKENS: u64 = 2_000;
/// Maximum token budget for a generated summary.
pub const MAX_SUMMARY_TOKENS: u64 = 12_000;

impl ContextCompressor {
    /// Construct a new compressor with the given configuration.
    pub fn new(config: CompressorConfig) -> Self {
        Self {
            config,
            previous_summary: None,
            compression_count: 0,
            summary_cooldown_until: None,
        }
    }

    /// The token count at which compression is triggered.
    ///
    /// `context_length * threshold`
    pub fn threshold_tokens(&self) -> u64 {
        (self.config.context_length as f64 * self.config.threshold) as u64
    }

    /// Token budget reserved for the protected tail of the conversation.
    ///
    /// `threshold_tokens * target_ratio`
    pub fn tail_token_budget(&self) -> u64 {
        (self.threshold_tokens() as f64 * self.config.target_ratio) as u64
    }

    /// Returns `true` when `prompt_tokens` has reached or exceeded the trigger threshold.
    pub fn should_compress(&self, prompt_tokens: u64) -> bool {
        prompt_tokens >= self.threshold_tokens()
    }

    /// Cheap character-based token estimate: `len / 4 + 10`.
    pub fn estimate_tokens(text: &str) -> u64 {
        (text.len() / 4 + 10) as u64
    }

    /// Estimate tokens for a single [`Message`], including tool-call arguments and a fixed
    /// per-message overhead of 10 tokens.
    pub fn estimate_message_tokens(msg: &Message) -> u64 {
        let content_tokens = msg
            .content
            .as_deref()
            .map(Self::estimate_tokens)
            .unwrap_or(0);

        let tool_call_tokens: u64 = msg
            .tool_calls
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|tc| Self::estimate_tokens(&tc.function.arguments))
            .sum();

        content_tokens + tool_call_tokens + 10
    }

    /// How many times compression has been applied so far.
    pub fn compression_count(&self) -> u32 {
        self.compression_count
    }

    /// Phase 1: Replace old tool results (before tail_start) exceeding 200 chars with placeholder.
    pub fn prune_old_tool_results(messages: &mut [Message], tail_start: usize) {
        let boundary = tail_start.min(messages.len());
        for msg in &mut messages[..boundary] {
            if msg.role == "tool"
                && let Some(ref content) = msg.content
                && content.len() > TOOL_RESULT_PRUNE_THRESHOLD
            {
                msg.content = Some(PRUNED_PLACEHOLDER.to_string());
            }
        }
    }

    /// Phase 2: Determine head/tail protection boundaries for a compression pass.
    ///
    /// Returns a [`CompressionBoundary`] where `head_end >= tail_start` signals that there
    /// is no compressible middle (too few messages).
    pub fn find_boundaries(&self, messages: &[Message]) -> CompressionBoundary {
        let n = messages.len();
        let protect_first_n = self.config.protect_first_n;
        // Minimum message count: protect_first_n + 3 (middle) + 1 (tail) = protect_first_n + 4
        // The spec says "< protect_first_n + 3 + 1" — interpret as strict: need at least
        // protect_first_n + 4 messages for any compressible middle to exist.
        if n < protect_first_n + 4 {
            // No compressible middle — signal with head_end == n
            return CompressionBoundary {
                head_end: n,
                tail_start: n,
            };
        }

        // ── Head boundary ──────────────────────────────────────────────────────
        // Start at protect_first_n, then skip forward past any orphan tool results
        // (tool messages that appear right at the head boundary without a preceding
        // assistant message already in the head).
        let mut head_end = protect_first_n;
        while head_end < n && messages[head_end].role == "tool" {
            head_end += 1;
        }

        // ── Tail boundary ──────────────────────────────────────────────────────
        // Walk backward from the end, accumulating tokens, until we have at least
        // 3 messages AND >= tail_token_budget tokens, or we exceed the soft ceiling.
        // Never walk past head_end + 1 so there is always at least one middle message.
        let budget = self.tail_token_budget();
        let soft_ceiling = (budget as f64 * 1.5) as u64;
        let mut accumulated: u64 = 0;
        let mut tail_count: usize = 0;
        // Default tail_start: last 3 messages (minimum tail), bounded by head_end + 1.
        let min_tail_start = (head_end + 1).min(n.saturating_sub(3));
        let mut tail_start = n.saturating_sub(3).max(head_end + 1);

        for i in (min_tail_start..n).rev() {
            let tokens = Self::estimate_message_tokens(&messages[i]);
            accumulated += tokens;
            tail_count += 1;
            tail_start = i;

            // Stop if we have at least 3 messages AND met the budget
            if tail_count >= 3 && accumulated >= budget {
                break;
            }
            // Stop if we exceeded the soft ceiling with at least 3 messages
            if accumulated > soft_ceiling && tail_count >= 3 {
                break;
            }
        }

        // ── Align tail_start backward to avoid splitting tool_call / tool pairs ──
        tail_start = Self::align_boundary_backward(messages, tail_start, head_end);

        // If there is no compressible middle, return the "no-op" signal.
        if head_end >= tail_start {
            CompressionBoundary {
                head_end: n,
                tail_start: n,
            }
        } else {
            CompressionBoundary {
                head_end,
                tail_start,
            }
        }
    }

    /// Walk `start` backward past any leading "tool" messages so that the boundary
    /// lands on the assistant message that issued the tool calls, keeping the
    /// assistant+tool pair intact in the tail.
    ///
    /// Will not move below `min_start`.
    pub fn align_boundary_backward(messages: &[Message], start: usize, min_start: usize) -> usize {
        let mut pos = start;
        while pos > min_start && messages[pos].role == "tool" {
            pos -= 1;
        }
        // If we landed on an assistant message that has tool_calls, include it
        // (pos is already pointing at it, so the tail includes it). Done.
        pos
    }

    /// Phase 4a: Sanitize tool pairs in a message list.
    ///
    /// - Removes orphan tool results (tool messages whose `tool_call_id` is not
    ///   referenced by any assistant `tool_calls` entry).
    /// - Injects a stub result for orphan calls (assistant `tool_calls` entries
    ///   whose `id` has no corresponding tool message).
    pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
        // ── Collect all call IDs produced by assistant tool_calls ──────────────
        let call_ids: std::collections::HashSet<String> = messages
            .iter()
            .filter_map(|m| m.tool_calls.as_deref())
            .flatten()
            .map(|tc| tc.id.clone())
            .collect();

        // ── Collect all call IDs consumed by tool result messages ──────────────
        let result_call_ids: std::collections::HashSet<String> = messages
            .iter()
            .filter(|m| m.role == "tool")
            .filter_map(|m| m.tool_call_id.as_deref())
            .map(|id| id.to_string())
            .collect();

        // ── Remove orphan tool results ─────────────────────────────────────────
        messages.retain(|m| {
            if m.role == "tool" {
                // Keep only if this tool_call_id exists in call_ids
                m.tool_call_id
                    .as_deref()
                    .map(|id| call_ids.contains(id))
                    .unwrap_or(false)
            } else {
                true
            }
        });

        // ── Inject stub results for orphan calls ───────────────────────────────
        // Collect (assistant_msg_index, orphan tool_calls) for injection.
        // We must work with indices into the already-modified messages vec.
        let mut injections: Vec<(usize, Vec<(String, String)>)> = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            if let Some(tool_calls) = msg.tool_calls.as_deref() {
                let orphans: Vec<(String, String)> = tool_calls
                    .iter()
                    .filter(|tc| !result_call_ids.contains(&tc.id))
                    .map(|tc| (tc.id.clone(), tc.function.name.clone()))
                    .collect();
                if !orphans.is_empty() {
                    injections.push((i, orphans));
                }
            }
        }

        // Insert in reverse order of assistant index to preserve positions.
        for (assistant_idx, orphans) in injections.into_iter().rev() {
            // Insert stubs right after the assistant message, in reverse orphan order
            // so that the first orphan ends up first after insertion.
            for (call_id, fn_name) in orphans.into_iter().rev() {
                let stub = Message {
                    role: "tool".to_string(),
                    content: Some(STUB_TOOL_RESULT.to_string()),
                    tool_calls: None,
                    tool_call_id: Some(call_id),
                    name: Some(fn_name),
                };
                messages.insert(assistant_idx + 1, stub);
            }
        }
    }

    /// Phase 4b: Assemble the final message list from head, optional summary, and tail.
    ///
    /// The summary message role is chosen to avoid role conflicts with adjacent messages:
    /// tries "user" first, then "assistant", then "system".
    pub fn assemble(
        &self,
        head: &[Message],
        summary: Option<String>,
        tail: &[Message],
    ) -> Vec<Message> {
        let mut result: Vec<Message> = head.to_vec();

        if let Some(summary_text) = summary {
            // Determine a role that doesn't conflict with head's last or tail's first.
            let last_head_role = head.last().map(|m| m.role.as_str()).unwrap_or("");
            let first_tail_role = tail.first().map(|m| m.role.as_str()).unwrap_or("");

            let summary_role = ["user", "assistant", "system"]
                .iter()
                .find(|&&r| r != last_head_role && r != first_tail_role)
                .copied()
                .unwrap_or("user");

            let summary_msg = Message {
                role: summary_role.to_string(),
                content: Some(format!("{COMPACTION_PREFIX}{summary_text}")),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            };
            result.push(summary_msg);
        }

        result.extend_from_slice(tail);
        result
    }
}
