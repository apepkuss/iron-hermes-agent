use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

use iron_tool_api::{ToolModule, ToolRegistry, ToolResult, ToolSchema};
use serde_json::{Value, json};

use crate::manager::SkillManager;

/// ToolModule that exposes SkillManager operations as LLM-callable tools.
pub struct SkillTools {
    pub manager: Arc<SkillManager>,
}

impl ToolModule for SkillTools {
    fn register(self: Box<Self>, registry: &mut ToolRegistry) {
        let manager = self.manager;

        // ── skills_list ──────────────────────────────────────────────────────
        {
            let mgr = Arc::clone(&manager);
            registry.register_sync(
                "skills_list",
                "skills",
                ToolSchema {
                    name: "skills_list".to_string(),
                    description: "List available skills, optionally filtered by category."
                        .to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "category": {
                                "type": "string",
                                "description": "Optional category filter"
                            }
                        },
                        "required": []
                    }),
                },
                move |args: Value, ctx| {
                    let category = args["category"].as_str();
                    let skills = mgr.list_skills(category, &ctx.enabled_tools);
                    let count = skills.len() as u64;
                    let skills_json: Vec<Value> = skills
                        .into_iter()
                        .map(|s| {
                            json!({
                                "name": s.name,
                                "description": s.description,
                                "category": s.category,
                            })
                        })
                        .collect();
                    Ok(ToolResult::ok(json!({
                        "skills": skills_json,
                        "count": count,
                        "hint": "Use skill_view(name) to load a specific skill's full content."
                    })))
                },
            );
        }

        // ── skill_view ───────────────────────────────────────────────────────
        {
            let mgr = Arc::clone(&manager);
            registry.register_sync(
                "skill_view",
                "skills",
                ToolSchema {
                    name: "skill_view".to_string(),
                    description:
                        "View a skill's full content. Optionally read a linked file within it."
                            .to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Skill name"
                            },
                            "file_path": {
                                "type": "string",
                                "description": "Optional relative path to a linked file (e.g. references/api.md)"
                            }
                        },
                        "required": ["name"]
                    }),
                },
                move |args: Value, _ctx| {
                    let name = match args["name"].as_str() {
                        Some(n) => n,
                        None => return Ok(ToolResult::err("missing required field: name")),
                    };

                    let skill = match mgr.view_skill(name) {
                        Ok(s) => s,
                        Err(e) => return Ok(ToolResult::err(&e.to_string())),
                    };

                    // If a file_path is given, read that specific linked file
                    if let Some(file_path) = args["file_path"].as_str() {
                        if file_path.contains("..") {
                            return Ok(ToolResult::err("path traversal not allowed"));
                        }
                        let skill_dir = match skill.path.parent() {
                            Some(d) => d,
                            None => return Ok(ToolResult::err("skill path has no parent")),
                        };
                        let full_path = skill_dir.join(file_path);
                        if !full_path.starts_with(skill_dir) {
                            return Ok(ToolResult::err("resolved path escapes skill directory"));
                        }
                        return match fs::read_to_string(&full_path) {
                            Ok(content) => Ok(ToolResult::ok(json!({
                                "name": skill.meta.name,
                                "file_path": file_path,
                                "content": content,
                            }))),
                            Err(e) => Ok(ToolResult::err(&e.to_string())),
                        };
                    }

                    // Build linked_files map grouped by subdirectory
                    let mut linked_files: HashMap<String, Vec<String>> = HashMap::new();
                    let skill_dir = skill.path.parent();
                    for linked in &skill.linked_files {
                        if let Some(dir) = skill_dir
                            && let Ok(rel) = linked.strip_prefix(dir)
                        {
                            let rel_str = rel.to_string_lossy().to_string();
                            // First component is the subdir
                            let subdir = rel
                                .components()
                                .next()
                                .map(|c| c.as_os_str().to_string_lossy().to_string())
                                .unwrap_or_default();
                            linked_files.entry(subdir).or_default().push(rel_str);
                        }
                    }

                    Ok(ToolResult::ok(json!({
                        "name": skill.meta.name,
                        "description": skill.meta.description,
                        "content": skill.body,
                        "linked_files": linked_files,
                    })))
                },
            );
        }

        // ── skill_manage ─────────────────────────────────────────────────────
        {
            let mgr = Arc::clone(&manager);
            registry.register_sync(
                "skill_manage",
                "skills",
                ToolSchema {
                    name: "skill_manage".to_string(),
                    description: "Create, edit, patch, or delete a skill, or manage linked files."
                        .to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "action": {
                                "type": "string",
                                "enum": ["create", "edit", "patch", "delete", "write_file", "remove_file"],
                                "description": "Action to perform"
                            },
                            "name": {
                                "type": "string",
                                "description": "Skill name"
                            },
                            "content": {
                                "type": "string",
                                "description": "Full skill content (for create/edit)"
                            },
                            "category": {
                                "type": "string",
                                "description": "Skill category directory (required for create)"
                            },
                            "old_string": {
                                "type": "string",
                                "description": "String to find (for patch)"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement string (for patch)"
                            },
                            "replace_all": {
                                "type": "boolean",
                                "description": "Replace all occurrences (for patch, default false)"
                            },
                            "file_path": {
                                "type": "string",
                                "description": "Relative path to linked file (for write_file/remove_file)"
                            },
                            "file_content": {
                                "type": "string",
                                "description": "Content for linked file (for write_file)"
                            }
                        },
                        "required": ["action", "name"]
                    }),
                },
                move |args: Value, _ctx| {
                    let action = match args["action"].as_str() {
                        Some(a) => a,
                        None => return Ok(ToolResult::err("missing required field: action")),
                    };
                    let name = match args["name"].as_str() {
                        Some(n) => n,
                        None => return Ok(ToolResult::err("missing required field: name")),
                    };

                    match action {
                        "create" => {
                            let content = match args["content"].as_str() {
                                Some(c) => c,
                                None => return Ok(ToolResult::err("missing required field: content")),
                            };
                            let category = match args["category"].as_str() {
                                Some(c) => c,
                                None => return Ok(ToolResult::err("missing required field: category")),
                            };
                            match mgr.create_skill(name, content, category) {
                                Ok(path) => Ok(ToolResult::ok(json!({
                                    "message": "skill created",
                                    "path": path.to_string_lossy()
                                }))),
                                Err(e) => Ok(ToolResult::err(&e.to_string())),
                            }
                        }
                        "edit" => {
                            let content = match args["content"].as_str() {
                                Some(c) => c,
                                None => return Ok(ToolResult::err("missing required field: content")),
                            };
                            match mgr.edit_skill(name, content) {
                                Ok(path) => Ok(ToolResult::ok(json!({
                                    "message": "skill updated",
                                    "path": path.to_string_lossy()
                                }))),
                                Err(e) => Ok(ToolResult::err(&e.to_string())),
                            }
                        }
                        "patch" => {
                            let old_string = match args["old_string"].as_str() {
                                Some(s) => s,
                                None => return Ok(ToolResult::err("missing required field: old_string")),
                            };
                            let new_string = match args["new_string"].as_str() {
                                Some(s) => s,
                                None => return Ok(ToolResult::err("missing required field: new_string")),
                            };
                            let replace_all = args["replace_all"].as_bool().unwrap_or(false);
                            match mgr.patch_skill(name, old_string, new_string, replace_all) {
                                Ok(path) => Ok(ToolResult::ok(json!({
                                    "message": "skill patched",
                                    "path": path.to_string_lossy()
                                }))),
                                Err(e) => Ok(ToolResult::err(&e.to_string())),
                            }
                        }
                        "delete" => match mgr.delete_skill(name) {
                            Ok(()) => Ok(ToolResult::ok(json!({"message": "skill deleted"}))),
                            Err(e) => Ok(ToolResult::err(&e.to_string())),
                        },
                        "write_file" => {
                            let file_path = match args["file_path"].as_str() {
                                Some(p) => p,
                                None => return Ok(ToolResult::err("missing required field: file_path")),
                            };
                            let file_content = match args["file_content"].as_str() {
                                Some(c) => c,
                                None => return Ok(ToolResult::err("missing required field: file_content")),
                            };
                            match mgr.write_linked_file(name, file_path, file_content) {
                                Ok(path) => Ok(ToolResult::ok(json!({
                                    "message": "linked file written",
                                    "path": path.to_string_lossy()
                                }))),
                                Err(e) => Ok(ToolResult::err(&e.to_string())),
                            }
                        }
                        "remove_file" => {
                            let file_path = match args["file_path"].as_str() {
                                Some(p) => p,
                                None => return Ok(ToolResult::err("missing required field: file_path")),
                            };
                            match mgr.remove_linked_file(name, file_path) {
                                Ok(()) => Ok(ToolResult::ok(json!({"message": "linked file removed"}))),
                                Err(e) => Ok(ToolResult::err(&e.to_string())),
                            }
                        }
                        _ => Ok(ToolResult::err(&format!("unknown action: {action}"))),
                    }
                },
            );
        }
    }
}
