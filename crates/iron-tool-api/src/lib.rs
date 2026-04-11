//! iron-tool-api — Tool system public API: types, errors, registry, and ToolModule trait.

pub mod error;
pub mod types;

pub use error::ToolError;
pub use types::{ToolContext, ToolResult, ToolSchema};
