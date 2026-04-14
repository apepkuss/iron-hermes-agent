use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use tokio::sync::{Mutex, RwLock};

use iron_core::runtime::{AgentRuntime, RuntimeConfig as CoreRuntimeConfig};
use iron_core::todo::{new_todo_senders, new_todo_state, register_todo};
use iron_memory::manager::MemoryManager;
use iron_memory::tool_module::MemoryTools;
use iron_sandbox::tool_module::register_execute_code;
use iron_skills::manager::SkillManager;
use iron_skills::tool_module::SkillTools;
use iron_tool_api::{ToolModule, ToolRegistry};
use iron_tools::{file_module::FileTools, terminal_module::TerminalTools, web_module::WebTools};

use crate::config::{IronConfig, RuntimeConfig, ServerConfig};

pub struct AppState {
    pub config: ServerConfig,
    pub runtime: Arc<AgentRuntime>,
    pub runtime_config: Arc<RwLock<RuntimeConfig>>,
    pub tool_registry: Arc<ToolRegistry>,
    pub memory_manager: Arc<Mutex<MemoryManager>>,
    pub skill_manager: Arc<SkillManager>,
}

pub fn build_app_state(config: IronConfig) -> AppState {
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

    // Register todo tool
    let todo_state = new_todo_state();
    let todo_senders = new_todo_senders();
    register_todo(
        &mut registry,
        Arc::clone(&todo_state),
        Arc::clone(&todo_senders),
    );

    // Register execute_code using a OnceLock to break the circular dependency:
    // the handler needs Arc<ToolRegistry> for sandbox RPC dispatch, but
    // ToolRegistry must be fully built before it can be wrapped in Arc.
    let registry_holder: Arc<OnceLock<Arc<ToolRegistry>>> = Arc::new(OnceLock::new());
    register_execute_code(&mut registry, Arc::clone(&registry_holder));

    let tool_registry = Arc::new(registry);

    // Populate the OnceLock so that execute_code handlers can resolve the registry.
    registry_holder
        .set(Arc::clone(&tool_registry))
        .unwrap_or_else(|_| panic!("registry_holder already set"));

    // Derive sub-configs from IronConfig
    let server_config = ServerConfig::from(&config);
    let runtime_config = Arc::new(RwLock::new(RuntimeConfig::from_iron_config(&config)));

    let core_runtime_config = CoreRuntimeConfig {
        agent_timeout_secs: config.agent.timeout,
        inactivity_timeout_secs: config.agent.inactivity_timeout,
        session_idle_timeout_secs: config.session.idle_timeout,
        fallback_model: config.fallback.model.clone(),
        llm_base_url: config.base_url.clone(),
        llm_api_key: config.api_key.clone(),
        llm_model: config.model.clone(),
        review_interval: config.agent.review_interval,
    };

    // AgentRuntime — central session management and agent caching
    let runtime = Arc::new(AgentRuntime::new(
        core_runtime_config,
        Arc::clone(&tool_registry),
        Arc::clone(&memory_manager),
        Arc::clone(&skill_manager),
        todo_senders,
        todo_state,
    ));

    AppState {
        config: server_config,
        runtime,
        runtime_config,
        tool_registry,
        memory_manager,
        skill_manager,
    }
}
