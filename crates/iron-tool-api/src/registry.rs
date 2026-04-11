use crate::error::ToolError;
use crate::types::{ToolContext, ToolResult, ToolSchema};
use serde_json::{Value, json};
use std::collections::HashMap;

type ToolHandler = Box<dyn Fn(Value, &ToolContext) -> Result<ToolResult, ToolError> + Send + Sync>;

struct ToolEntry {
    toolset: String,
    schema: ToolSchema,
    handler: ToolHandler,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolEntry>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register_sync<F>(&mut self, name: &str, toolset: &str, schema: ToolSchema, handler: F)
    where
        F: Fn(Value, &ToolContext) -> Result<ToolResult, ToolError> + Send + Sync + 'static,
    {
        self.tools.insert(
            name.to_string(),
            ToolEntry {
                toolset: toolset.to_string(),
                schema,
                handler: Box::new(handler),
            },
        );
    }

    pub fn dispatch_sync(
        &self,
        name: &str,
        args: Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let entry = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;
        (entry.handler)(args, ctx)
    }

    pub fn get_schemas(&self, ctx: &ToolContext) -> Vec<Value> {
        self.tools
            .values()
            .filter(|entry| ctx.enabled_tools.contains(&entry.schema.name))
            .map(|entry| {
                json!({
                    "type": "function",
                    "function": {
                        "name": entry.schema.name,
                        "description": entry.schema.description,
                        "parameters": entry.schema.parameters,
                    }
                })
            })
            .collect()
    }

    pub fn tool_names(&self) -> std::collections::HashSet<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    #[allow(dead_code)]
    pub fn toolset_of(&self, name: &str) -> Option<&str> {
        self.tools.get(name).map(|e| e.toolset.as_str())
    }
}
