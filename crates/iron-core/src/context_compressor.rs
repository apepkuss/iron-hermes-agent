use std::time::Instant;

use crate::llm::types::Message;

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
}
