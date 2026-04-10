//! iron-skills crate — skill file parsing and discovery

pub mod bundled;
pub mod discovery;
pub mod error;
pub mod manager;
pub mod parser;
pub mod security;
pub mod types;

pub use discovery::SkillDiscovery;
pub use error::SkillError;
pub use manager::SkillManager;
pub use parser::parse_skill_file;
pub use security::{check_path_traversal, scan_skill_content, validate_skill_name};
pub use types::{Skill, SkillConditions, SkillMeta, SkillSummary};
