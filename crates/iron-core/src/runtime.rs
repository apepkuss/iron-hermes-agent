use std::time::Instant;

/// 标识消息来源的平台和上下文
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SessionSource {
    pub platform: String,
    pub chat_id: String,
    pub user_id: String,
    pub thread_id: Option<String>,
}

/// 由 SessionSource 确定性生成 session key
pub fn build_session_key(source: &SessionSource) -> String {
    match &source.thread_id {
        Some(tid) => format!(
            "{}:{}:{}:{}",
            source.platform, source.chat_id, source.user_id, tid
        ),
        None => format!("{}:{}:{}", source.platform, source.chat_id, source.user_id),
    }
}

/// Session 元数据
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub session_id: String,
    pub session_key: String,
    pub source: SessionSource,
    pub created_at: Instant,
    pub updated_at: Instant,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub message_count: u32,
}
