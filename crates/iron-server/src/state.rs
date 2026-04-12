use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use iron_memory::manager::MemoryManager;
use iron_memory::tool_module::MemoryTools;
use iron_skills::manager::SkillManager;
use iron_skills::tool_module::SkillTools;
use iron_tool_api::{ToolModule, ToolRegistry};
use iron_tools::{file_module::FileTools, terminal_module::TerminalTools, web_module::WebTools};

use crate::config::{RuntimeConfig, ServerConfig};

pub struct AppState {
    pub config: ServerConfig,
    pub tool_registry: Arc<ToolRegistry>,
    pub memory_manager: Arc<Mutex<MemoryManager>>,
    pub skill_manager: Arc<SkillManager>,
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
}

pub fn build_app_state(config: ServerConfig) -> AppState {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let base = home.join(".iron-hermes");

    // Memory
    let memory_dir = base.join("memories");
    std::fs::create_dir_all(&memory_dir).ok();
    let mut memory_manager = MemoryManager::new(&memory_dir, None, None);
    memory_manager.initialize().ok();
    let memory_manager = Arc::new(Mutex::new(memory_manager));

    // Skills
    let skills_dir = base.join("skills");
    std::fs::create_dir_all(&skills_dir).ok();
    let skill_manager = SkillManager::new(vec![skills_dir], HashSet::new());
    skill_manager.ensure_bundled_skills().ok();
    let skill_manager = Arc::new(skill_manager);

    // Tool registry — assemble via ToolModule trait
    let mut registry = ToolRegistry::new();

    let modules: Vec<Box<dyn ToolModule>> = vec![
        Box::new(TerminalTools::new(30)),
        Box::new(FileTools),
        Box::new(WebTools::from_env()),
        Box::new(SkillTools {
            manager: Arc::clone(&skill_manager),
        }),
        Box::new(MemoryTools {
            manager: Arc::clone(&memory_manager),
        }),
    ];

    for module in modules {
        module.register(&mut registry);
    }

    let tool_registry = Arc::new(registry);

    // Runtime config — initialized from ServerConfig, mutable via /api/config
    let runtime_config = Arc::new(RwLock::new(RuntimeConfig {
        llm_base_url: config.llm_base_url.clone(),
        llm_model: config.llm_model.clone(),
        auxiliary_model: config.auxiliary_model.clone(),
        compression_threshold: config.compression_threshold,
        context_length_override: config.context_length_override,
    }));

    AppState {
        config,
        tool_registry,
        memory_manager,
        skill_manager,
        runtime_config,
    }
}
