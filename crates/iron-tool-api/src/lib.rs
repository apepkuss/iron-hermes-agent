//! iron-tool-api — Tool system public API: types, errors, registry, and ToolModule trait.

pub mod error;
pub mod module_trait;
pub mod registry;
pub mod types;

pub use error::ToolError;
pub use module_trait::ToolModule;
pub use registry::ToolRegistry;
pub use types::{ToolContext, ToolResult, ToolSchema};
