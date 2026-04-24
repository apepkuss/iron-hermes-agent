//! LLM-facing cron job tool.

use std::sync::{Arc, Mutex};

use iron_tool_api::{ToolError, ToolRegistry, ToolResult, ToolSchema};
use serde_json::{Value, json};

use crate::cron::{CronJobPatch, NewCronJob};
use crate::session::store::SessionStore;

/// Register the `cronjob` tool into the given [`ToolRegistry`].
pub fn register_cronjob(registry: &mut ToolRegistry, session_store: Arc<Mutex<SessionStore>>) {
    let schema = ToolSchema {
        name: "cronjob".to_string(),
        description: "Manage scheduled cron jobs. Use action='create' when the user asks to run a task on a schedule. Also supports list, update, pause, resume, remove, and run."
            .to_string(),
        parameters: json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "update", "pause", "resume", "remove", "run"],
                    "description": "Operation to perform."
                },
                "id": {
                    "type": "string",
                    "description": "Cron job id. Required for update, pause, resume, remove, and run."
                },
                "name": {
                    "type": "string",
                    "description": "Human-readable job name. Optional for create; defaults from the prompt."
                },
                "prompt": {
                    "type": "string",
                    "description": "Instruction to send to the agent whenever the job runs. Required for create."
                },
                "schedule": {
                    "type": "string",
                    "description": "Schedule such as 'every 30m', 'every 2h', 'daily 09:00', or a 5-field cron expression. Required for create."
                },
                "enabled": {
                    "type": "boolean",
                    "description": "Whether the job is enabled. Defaults to true for create."
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for this job. Omit or empty string to use the current default model."
                },
                "disabled_toolsets": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional toolsets to disable while this job runs."
                }
            },
            "required": ["action"]
        }),
    };

    registry.register_sync("cronjob", "cronjob", schema, move |args, _ctx| {
        handle_cronjob(args, &session_store)
    });
}

fn handle_cronjob(
    args: Value,
    session_store: &Arc<Mutex<SessionStore>>,
) -> Result<ToolResult, ToolError> {
    let action = required_string(&args, "action")?;
    let store = session_store
        .lock()
        .map_err(|e| ToolError::ExecutionFailed(format!("session store poisoned: {e}")))?;

    let result = match action.as_str() {
        "create" => {
            let prompt = required_string(&args, "prompt")?;
            let schedule = required_string(&args, "schedule")?;
            let name =
                optional_string(&args, "name").unwrap_or_else(|| derive_name_from_prompt(&prompt));
            let job = store
                .create_cron_job(NewCronJob {
                    name,
                    prompt,
                    schedule,
                    enabled: optional_bool(&args, "enabled").unwrap_or(true),
                    model: optional_string(&args, "model"),
                    disabled_toolsets: optional_string_array(&args, "disabled_toolsets")?,
                })
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "job": job })
        }
        "list" => {
            let jobs = store
                .list_cron_jobs()
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "jobs": jobs })
        }
        "update" => {
            let id = required_string(&args, "id")?;
            let job = store
                .update_cron_job(
                    &id,
                    CronJobPatch {
                        name: optional_string(&args, "name"),
                        prompt: optional_string(&args, "prompt"),
                        schedule: optional_string(&args, "schedule"),
                        enabled: optional_bool(&args, "enabled"),
                        model: optional_model_patch(&args),
                        disabled_toolsets: optional_string_array_patch(&args, "disabled_toolsets")?,
                    },
                )
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "job": job })
        }
        "pause" => {
            let id = required_string(&args, "id")?;
            let job = store
                .update_cron_job(
                    &id,
                    CronJobPatch {
                        enabled: Some(false),
                        ..CronJobPatch::default()
                    },
                )
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "job": job })
        }
        "resume" => {
            let id = required_string(&args, "id")?;
            let job = store
                .update_cron_job(
                    &id,
                    CronJobPatch {
                        enabled: Some(true),
                        ..CronJobPatch::default()
                    },
                )
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "job": job })
        }
        "remove" => {
            let id = required_string(&args, "id")?;
            store
                .delete_cron_job(&id)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "status": "removed", "id": id })
        }
        "run" => {
            let id = required_string(&args, "id")?;
            let job = store
                .schedule_cron_job_now(&id)
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
            json!({ "status": "scheduled", "job": job })
        }
        other => {
            return Err(ToolError::InvalidArgs {
                tool: "cronjob".to_string(),
                reason: format!("unsupported action: {other}"),
            });
        }
    };

    Ok(ToolResult::ok(result))
}

fn required_string(args: &Value, key: &str) -> Result<String, ToolError> {
    optional_string(args, key).ok_or_else(|| ToolError::InvalidArgs {
        tool: "cronjob".to_string(),
        reason: format!("{key} is required"),
    })
}

fn optional_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn optional_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn optional_model_patch(args: &Value) -> Option<Option<String>> {
    if !args
        .as_object()
        .is_some_and(|obj| obj.contains_key("model"))
    {
        return None;
    }
    Some(optional_string(args, "model"))
}

fn optional_string_array_patch(args: &Value, key: &str) -> Result<Option<Vec<String>>, ToolError> {
    if !args.as_object().is_some_and(|obj| obj.contains_key(key)) {
        return Ok(None);
    }
    optional_string_array(args, key).map(Some)
}

fn optional_string_array(args: &Value, key: &str) -> Result<Vec<String>, ToolError> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(ToolError::InvalidArgs {
            tool: "cronjob".to_string(),
            reason: format!("{key} must be an array of strings"),
        });
    };
    Ok(items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn derive_name_from_prompt(prompt: &str) -> String {
    let trimmed = prompt.trim();
    let mut name: String = trimmed.chars().take(48).collect();
    if trimmed.chars().count() > 48 {
        name.push_str("...");
    }
    if name.is_empty() {
        "Scheduled task".to_string()
    } else {
        name
    }
}
