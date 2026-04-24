use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use tracing::{debug, warn};

use iron_core::agent::AgentConfig;
use iron_core::context_compressor::{AuxiliaryLlmConfig, CompressorConfig};
use iron_core::cron::{CronJob, CronRunFinish};
use iron_core::runtime::{AgentRuntime, SessionSource};
use iron_core::session::store::{SessionStore, unix_now};

use crate::config::RuntimeConfig;

const POLL_INTERVAL_SECS: u64 = 15;
const DUE_JOB_LIMIT: u32 = 8;
const MAX_STORED_OUTPUT_CHARS: usize = 200_000;

pub fn spawn_cron_scheduler(
    runtime: Arc<AgentRuntime>,
    session_store: Arc<std::sync::Mutex<SessionStore>>,
    runtime_config: Arc<tokio::sync::RwLock<RuntimeConfig>>,
) {
    if let Ok(store) = session_store.lock()
        && let Err(e) = store.reset_stale_cron_running_flags()
    {
        warn!("Failed to reset stale cron state: {e}");
    }

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;
            let due = match session_store.lock() {
                Ok(store) => store.due_cron_jobs(unix_now(), DUE_JOB_LIMIT),
                Err(e) => {
                    warn!("Cron store lock failed: {e}");
                    continue;
                }
            };

            match due {
                Ok(jobs) => {
                    for job in jobs {
                        spawn_cron_job_run(
                            Arc::clone(&runtime),
                            Arc::clone(&session_store),
                            Arc::clone(&runtime_config),
                            job,
                        );
                    }
                }
                Err(e) => warn!("Failed to query due cron jobs: {e}"),
            }
        }
    });
}

pub fn spawn_cron_job_run(
    runtime: Arc<AgentRuntime>,
    session_store: Arc<std::sync::Mutex<SessionStore>>,
    runtime_config: Arc<tokio::sync::RwLock<RuntimeConfig>>,
    job: CronJob,
) {
    tokio::spawn(async move {
        if let Err(e) = run_cron_job(runtime, session_store, runtime_config, job).await {
            warn!("Cron run failed before result persistence: {e}");
        }
    });
}

async fn run_cron_job(
    runtime: Arc<AgentRuntime>,
    session_store: Arc<std::sync::Mutex<SessionStore>>,
    runtime_config: Arc<tokio::sync::RwLock<RuntimeConfig>>,
    job: CronJob,
) -> anyhow::Result<()> {
    let run = {
        let store = session_store
            .lock()
            .map_err(|e| anyhow::anyhow!("cron store lock failed: {e}"))?;
        store.start_cron_run(&job.id)?
    };

    debug!(
        "Started cron job '{}' ({}) run {}",
        job.name, job.id, run.id
    );
    let start = Instant::now();
    let source = SessionSource {
        platform: "cron".to_string(),
        chat_id: job.id.clone(),
        user_id: "scheduler".to_string(),
        thread_id: None,
    };

    let rc = runtime_config.read().await;
    let agent_config = build_agent_config(&rc, &job);
    drop(rc);
    let result = runtime
        .handle_message(&source, job.prompt.clone(), agent_config, None, Vec::new())
        .await;

    let session_id = runtime
        .get_session_info(&source)
        .await
        .map(|entry| entry.session_id);
    let duration_ms = start.elapsed().as_millis() as u64;

    let finish = match result {
        Ok(response) => CronRunFinish {
            status: "succeeded".to_string(),
            output: Some(truncate_chars(&response.content, MAX_STORED_OUTPUT_CHARS)),
            error: None,
            prompt_tokens: response.usage.prompt_tokens,
            completion_tokens: response.usage.completion_tokens,
            total_tokens: response.usage.total_tokens,
            duration_ms,
            session_id,
        },
        Err(e) => CronRunFinish {
            status: "failed".to_string(),
            output: None,
            error: Some(e.to_string()),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            duration_ms,
            session_id,
        },
    };

    let store = session_store
        .lock()
        .map_err(|e| anyhow::anyhow!("cron store lock failed: {e}"))?;
    store.finish_cron_run(run.id, &job, finish)?;
    Ok(())
}

fn build_agent_config(rc: &RuntimeConfig, job: &CronJob) -> AgentConfig {
    let mut disabled: HashSet<String> = rc.disabled_toolsets.iter().cloned().collect();
    disabled.extend(job.disabled_toolsets.iter().cloned());

    AgentConfig {
        model_name: job.model.clone().unwrap_or_else(|| rc.llm_model.clone()),
        compressor_config: build_compressor_config(rc),
        disabled_toolsets: disabled.into_iter().collect(),
        ..AgentConfig::default()
    }
}

fn build_compressor_config(rc: &RuntimeConfig) -> Option<CompressorConfig> {
    if rc.compression_threshold <= 0.0 {
        return None;
    }
    let context_length = rc.context_length_override.unwrap_or(128_000);
    Some(CompressorConfig {
        context_length,
        threshold: rc.compression_threshold,
        target_ratio: 0.20,
        protect_first_n: 3,
        auxiliary_llm: rc.auxiliary_model.as_ref().map(|m| AuxiliaryLlmConfig {
            base_url: rc.llm_base_url.clone(),
            model: m.clone(),
        }),
    })
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut out: String = input.chars().take(max_chars).collect();
    out.push_str("\n\n[truncated]");
    out
}
