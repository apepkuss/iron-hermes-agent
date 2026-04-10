//! iron-sandbox crate — subprocess-based code execution sandbox.

pub mod bridge;
pub mod error;
pub mod rpc;
pub mod sandbox;

pub use error::SandboxError;
pub use sandbox::{Sandbox, SandboxConfig, SandboxResult, SandboxStatus};
