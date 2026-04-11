//! iron-tools crate — tool handler implementations + re-exports from iron-tool-api.

// Re-export iron-tool-api public modules so downstream crates
// can continue using `iron_tools::registry::ToolRegistry` etc.
pub use iron_tool_api::error;
pub use iron_tool_api::module_trait;
pub use iron_tool_api::registry;
pub use iron_tool_api::types;

// Convenience re-exports at crate root
pub use iron_tool_api::{ToolContext, ToolError, ToolModule, ToolRegistry, ToolResult, ToolSchema};

// Handler implementations
pub mod file;
pub mod file_module;
pub mod terminal;
pub mod terminal_module;
pub mod web;
pub mod web_module;
