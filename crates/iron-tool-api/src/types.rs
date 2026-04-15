use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub task_id: String,
    pub working_dir: std::path::PathBuf,
    pub enabled_tools: HashSet<String>,
    /// Safe environment variables for the current session.
    /// Terminal tools should use these instead of inheriting process env.
    pub env_vars: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: Value,
}

impl ToolResult {
    pub fn ok(output: Value) -> Self {
        Self {
            success: true,
            output,
        }
    }

    pub fn err(message: &str) -> Self {
        Self {
            success: false,
            output: Value::String(message.to_string()),
        }
    }
}
