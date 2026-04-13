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
                    serde_json::Value::String(s) => {
                        if s.len() > 30 {
                            format!("\"{}...\"", &s[..27])
                        } else {
                            format!("\"{}\"", s)
                        }
                    }
                    other => {
                        let s = other.to_string();
                        if s.len() > 30 {
                            format!("{}...", &s[..27])
                        } else {
                            s
                        }
                    }
                };
                format!("{k}={val}")
            })
            .collect()
    });

    pairs.join(", ")
}

/// 截断字符串预览。
pub fn truncate_preview(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
