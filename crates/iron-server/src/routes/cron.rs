use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use iron_core::cron::{CronJobPatch, NewCronJob, next_run_epoch};
use iron_core::session::store::unix_now;

use crate::cron_runner::spawn_cron_job_run;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CronJobBody {
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub disabled_toolsets: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CronJobPatchBody {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<String>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub model: Option<Option<String>>,
    pub disabled_toolsets: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct RunsQuery {
    pub limit: Option<u32>,
}

fn default_enabled() -> bool {
    true
}

pub async fn list_jobs(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.session_store.lock() {
        Ok(store) => match store.list_cron_jobs() {
            Ok(jobs) => (StatusCode::OK, Json(json!({ "jobs": jobs }))),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("session store poisoned: {e}") })),
        ),
    }
}

pub async fn create_job(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CronJobBody>,
) -> impl IntoResponse {
    if let Err(resp) = validate_job_fields(&body.name, &body.prompt, &body.schedule) {
        return resp;
    }

    let input = NewCronJob {
        name: body.name.trim().to_string(),
        prompt: body.prompt.trim().to_string(),
        schedule: body.schedule.trim().to_string(),
        enabled: body.enabled,
        model: normalize_model(body.model),
        disabled_toolsets: body.disabled_toolsets,
    };

    match state.session_store.lock() {
        Ok(store) => match store.create_cron_job(input) {
            Ok(job) => (StatusCode::CREATED, Json(json!({ "job": job }))),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("session store poisoned: {e}") })),
        ),
    }
}

pub async fn update_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<CronJobPatchBody>,
) -> impl IntoResponse {
    if let Some(schedule) = body.schedule.as_deref()
        && let Err(e) = next_run_epoch(schedule, unix_now())
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        );
    }

    let patch = CronJobPatch {
        name: body.name.map(|s| s.trim().to_string()),
        prompt: body.prompt.map(|s| s.trim().to_string()),
        schedule: body.schedule.map(|s| s.trim().to_string()),
        enabled: body.enabled,
        model: body.model.map(|m| normalize_model(m)),
        disabled_toolsets: body.disabled_toolsets,
    };

    match state.session_store.lock() {
        Ok(store) => match store.update_cron_job(&id, patch) {
            Ok(job) => (StatusCode::OK, Json(json!({ "job": job }))),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("session store poisoned: {e}") })),
        ),
    }
}

pub async fn delete_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.session_store.lock() {
        Ok(store) => match store.delete_cron_job(&id) {
            Ok(()) => (StatusCode::OK, Json(json!({ "status": "deleted" }))),
            Err(e) => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": e.to_string() })),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("session store poisoned: {e}") })),
        ),
    }
}

pub async fn run_job_now(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let job = match state.session_store.lock() {
        Ok(store) => match store.get_cron_job(&id) {
            Ok(Some(job)) => job,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "cron job not found" })),
                );
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                );
            }
        },
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("session store poisoned: {e}") })),
            );
        }
    };

    if job.running {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "cron job is already running" })),
        );
    }

    spawn_cron_job_run(
        Arc::clone(&state.runtime),
        Arc::clone(&state.session_store),
        Arc::clone(&state.runtime_config),
        job,
    );

    (StatusCode::ACCEPTED, Json(json!({ "status": "started" })))
}

pub async fn list_runs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<RunsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    match state.session_store.lock() {
        Ok(store) => match store.list_cron_runs(&id, limit) {
            Ok(runs) => (StatusCode::OK, Json(json!({ "runs": runs }))),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            ),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("session store poisoned: {e}") })),
        ),
    }
}

fn validate_job_fields(
    name: &str,
    prompt: &str,
    schedule: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "name is required" })),
        ));
    }
    if prompt.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "prompt is required" })),
        ));
    }
    if let Err(e) = next_run_epoch(schedule, unix_now()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        ));
    }
    Ok(())
}

fn normalize_model(model: Option<String>) -> Option<String> {
    model.and_then(|m| {
        let trimmed = m.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}
