use std::collections::HashMap;
use std::path::PathBuf;

use iron_tool_api::env::collect_safe_env;

/// Per-session terminal environment state.
#[derive(Debug, Clone)]
pub struct SessionEnvironment {
    /// Default working directory for this session.
    /// Commands without explicit `workdir` will execute in this directory.
    pub working_dir: PathBuf,

    /// Safe environment variables for this session.
    /// Filtered from process env using whitelist + secret blocking.
    pub env_vars: HashMap<String, String>,
}

impl SessionEnvironment {
    /// Create a new session environment with the given working directory.
    /// Environment variables are collected by filtering the current process
    /// env through the safe-env whitelist.
    pub fn new(working_dir: PathBuf) -> Self {
        let env_vars = collect_safe_env();
        Self {
            working_dir,
            env_vars,
        }
    }
}
