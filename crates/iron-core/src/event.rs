use serde::Serialize;

/// Agent 运行时事件。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// LLM 文本流 delta
    TextDelta { content: String },
    /// 工具调用开始
    ToolStarted {
        tool: String,
        args_preview: String,
        call_id: String,
    },
    /// 工具调用完成
    ToolCompleted {
        tool: String,
        call_id: String,
        duration_ms: u64,
        success: bool,
        result_preview: String,
    },
    /// TODO 列表更新
    TodoUpdate { todos: Vec<TodoItem> },
}

/// TODO 任务项。
#[derive(Debug, Clone, Serialize)]
pub struct TodoItem {
    pub content: String,
    pub status: String, // "pending", "in_progress", "completed"
}

/// 事件回调类型。
pub type EventCallback = Box<dyn Fn(AgentEvent) + Send + Sync>;

/// 按字符（非字节）截断字符串；超过 `max_chars` 时追加 `...`。
///
/// 注意：必须按字符边界切分，否则对 UTF-8 多字节字符（如中文）做字节切片
/// 会触发 panic：`byte index N is not a char boundary`。
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut iter = s.chars();
    let head: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

/// 从 JSON 参数构建简短预览。
pub fn build_args_preview(args_json: &str) -> String {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    let pairs: Vec<String> = args.as_object().map_or(vec![], |obj| {
        obj.iter()
            .take(3)
            .map(|(k, v)| {
                let val = match v {
                    serde_json::Value::String(s) => format!("\"{}\"", truncate_chars(s, 30)),
                    other => truncate_chars(&other.to_string(), 30),
                };
                format!("{k}={val}")
            })
            .collect()
    });

    pairs.join(", ")
}

/// 截断字符串预览（按字符计数，而非字节）。
pub fn truncate_preview(s: &str, max_len: usize) -> String {
    truncate_chars(s, max_len)
}
