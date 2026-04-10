use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::Mutex;

use iron_memory::manager::MemoryManager;
use iron_skills::manager::SkillManager;
use iron_tools::register_all::register_default_tools;
use iron_tools::registry::ToolRegistry;

use crate::config::ServerConfig;

/// Shared application state, wrapped in `Arc` and injected into every handler.
pub struct AppState {
    pub config: ServerConfig,
    pub tool_registry: Arc<ToolRegistry>,
    pub memory_manager: Arc<Mutex<MemoryManager>>,
    #[allow(dead_code)]
    pub skill_manager: Arc<SkillManager>,
}

/// Build the application state from the given configuration.
///
/// - Registers all built-in tools via `register_default_tools()`.
/// - Creates a `MemoryManager` rooted at `~/.iron-hermes/memories/`.
/// - Creates a `SkillManager` scanning `~/.iron-hermes/skills/`.
pub fn build_app_state(config: ServerConfig) -> AppState {
    let tool_registry = Arc::new(register_default_tools());

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let base = home.join(".iron-hermes");

    let memory_dir = base.join("memories");
    std::fs::create_dir_all(&memory_dir).ok();
    let mut memory_manager = MemoryManager::new(&memory_dir, None, None);
    memory_manager.initialize().ok();
    let memory_manager = Arc::new(Mutex::new(memory_manager));

    let skills_dir = base.join("skills");
    std::fs::create_dir_all(&skills_dir).ok();
    let skill_manager = Arc::new(SkillManager::new(vec![skills_dir], HashSet::new()));

    AppState {
        config,
        tool_registry,
        memory_manager,
        skill_manager,
    }
}
