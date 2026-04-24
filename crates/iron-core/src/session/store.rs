use rusqlite::{Connection, OptionalExtension, Result as SqlResult, params};
use uuid::Uuid;

use crate::cron::{CronJob, CronJobPatch, CronRun, CronRunFinish, NewCronJob, next_run_epoch};
use crate::error::CoreError;

use super::types::{Session, SessionMessage, TokenUsage};

/// SQLite-backed session store.
pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Ensure the libsimple FTS5 tokenizer is registered as an auto-extension.
    /// Must be called once before opening any database connections.
    fn ensure_simple_tokenizer() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            libsimple::enable_auto_extension()
                .expect("Failed to register libsimple FTS5 tokenizer");
        });
    }

    /// Open (or create) a SQLite database at `db_path`.
    pub fn new(db_path: &str) -> Result<Self, CoreError> {
        Self::ensure_simple_tokenizer();
        let conn = Connection::open(db_path)
            .map_err(|e| CoreError::Session(format!("Failed to open database: {e}")))?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    /// Create an in-memory SQLite database (primarily for testing).
    pub fn new_in_memory() -> Result<Self, CoreError> {
        Self::ensure_simple_tokenizer();
        let conn = Connection::open_in_memory()
            .map_err(|e| CoreError::Session(format!("Failed to open in-memory database: {e}")))?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<(), CoreError> {
        self.conn
            .execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| CoreError::Session(format!("Failed to enable WAL mode: {e}")))?;

        self.conn
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    model TEXT NOT NULL,
                    system_prompt TEXT,
                    parent_session_id TEXT,
                    started_at TEXT NOT NULL,
                    ended_at TEXT,
                    end_reason TEXT,
                    message_count INTEGER DEFAULT 0,
                    tool_call_count INTEGER DEFAULT 0,
                    prompt_tokens INTEGER DEFAULT 0,
                    completion_tokens INTEGER DEFAULT 0,
                    total_tokens INTEGER DEFAULT 0,
                    title TEXT
                );

                CREATE TABLE IF NOT EXISTS messages (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT,
                    tool_call_id TEXT,
                    tool_calls TEXT,
                    tool_name TEXT,
                    timestamp TEXT NOT NULL,
                    finish_reason TEXT,
                    FOREIGN KEY (session_id) REFERENCES sessions(id)
                );

                CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
                CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);

                CREATE TABLE IF NOT EXISTS cron_jobs (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    prompt TEXT NOT NULL,
                    schedule TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    model TEXT,
                    disabled_toolsets TEXT NOT NULL DEFAULT '[]',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    next_run_at TEXT,
                    next_run_at_epoch INTEGER,
                    last_run_at TEXT,
                    running INTEGER NOT NULL DEFAULT 0,
                    failure_count INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS cron_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    job_id TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    status TEXT NOT NULL,
                    output TEXT,
                    error TEXT,
                    prompt_tokens INTEGER NOT NULL DEFAULT 0,
                    completion_tokens INTEGER NOT NULL DEFAULT 0,
                    total_tokens INTEGER NOT NULL DEFAULT 0,
                    duration_ms INTEGER NOT NULL DEFAULT 0,
                    session_id TEXT,
                    FOREIGN KEY (job_id) REFERENCES cron_jobs(id)
                );

                CREATE INDEX IF NOT EXISTS idx_cron_jobs_due
                    ON cron_jobs(enabled, running, next_run_at_epoch);
                CREATE INDEX IF NOT EXISTS idx_cron_runs_job
                    ON cron_runs(job_id, id DESC);
                ",
            )
            .map_err(|e| CoreError::Session(format!("Failed to create tables: {e}")))?;

        // FTS5 full-text search index with libsimple tokenizer (jieba Chinese + English).
        self.conn
            .execute_batch(
                "
                CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
                    USING fts5(content, session_id UNINDEXED, role UNINDEXED,
                               content=messages, content_rowid=id, tokenize='simple');

                CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
                    INSERT INTO messages_fts(rowid, content, session_id, role)
                        VALUES (new.id, new.content, new.session_id, new.role);
                END;

                CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, content, session_id, role)
                        VALUES('delete', old.id, old.content, old.session_id, old.role);
                END;

                CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
                    INSERT INTO messages_fts(messages_fts, rowid, content, session_id, role)
                        VALUES('delete', old.id, old.content, old.session_id, old.role);
                    INSERT INTO messages_fts(rowid, content, session_id, role)
                        VALUES (new.id, new.content, new.session_id, new.role);
                END;
                ",
            )
            .map_err(|e| CoreError::Session(format!("Failed to create FTS5 tables: {e}")))?;

        Ok(())
    }

    /// Insert a new session row.
    pub fn create_session(&self, session: &Session) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT INTO sessions (
                    id, model, system_prompt, parent_session_id, started_at,
                    ended_at, end_reason, message_count, tool_call_count, title
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    session.id,
                    session.model,
                    session.system_prompt,
                    session.parent_session_id,
                    session.started_at,
                    session.ended_at,
                    session.end_reason,
                    session.message_count,
                    session.tool_call_count,
                    session.title,
                ],
            )
            .map_err(|e| CoreError::Session(format!("Failed to create session: {e}")))?;
        Ok(())
    }

    /// Update `ended_at` (current UTC time) and `end_reason` for the given session.
    pub fn end_session(&self, session_id: &str, reason: &str) -> Result<(), CoreError> {
        let now = chrono_now();
        let rows = self
            .conn
            .execute(
                "UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3",
                params![now, reason, session_id],
            )
            .map_err(|e| CoreError::Session(format!("Failed to end session: {e}")))?;

        if rows == 0 {
            return Err(CoreError::Session(format!(
                "Session not found: {session_id}"
            )));
        }
        Ok(())
    }

    /// Retrieve a session by its ID.
    pub fn get_session(&self, session_id: &str) -> Result<Option<Session>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, model, system_prompt, parent_session_id, started_at,
                        ended_at, end_reason, message_count, tool_call_count, title
                 FROM sessions WHERE id = ?1",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare query: {e}")))?;

        let mut rows = stmt
            .query_map(params![session_id], row_to_session)
            .map_err(|e| CoreError::Session(format!("Failed to query session: {e}")))?;

        match rows.next() {
            Some(result) => {
                Ok(Some(result.map_err(|e| {
                    CoreError::Session(format!("Failed to read session: {e}"))
                })?))
            }
            None => Ok(None),
        }
    }

    /// List sessions ordered by `started_at` DESC.
    pub fn list_sessions(&self, limit: u32, offset: u32) -> Result<Vec<Session>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, model, system_prompt, parent_session_id, started_at,
                        ended_at, end_reason, message_count, tool_call_count, title
                 FROM sessions
                 ORDER BY started_at DESC
                 LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map(params![limit, offset], row_to_session)
            .map_err(|e| CoreError::Session(format!("Failed to list sessions: {e}")))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(
                row.map_err(|e| CoreError::Session(format!("Failed to read session row: {e}")))?,
            );
        }
        Ok(sessions)
    }

    /// Insert a message row. Sets `message.id` is ignored on insert (AUTOINCREMENT).
    pub fn add_message(&self, message: &SessionMessage) -> Result<i64, CoreError> {
        self.conn
            .execute(
                "INSERT INTO messages (
                    session_id, role, content, tool_call_id, tool_calls,
                    tool_name, timestamp, finish_reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    message.session_id,
                    message.role,
                    message.content,
                    message.tool_call_id,
                    message.tool_calls,
                    message.tool_name,
                    message.timestamp,
                    message.finish_reason,
                ],
            )
            .map_err(|e| CoreError::Session(format!("Failed to add message: {e}")))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// Retrieve all messages for a session, ordered by `id` ASC.
    pub fn get_messages(&self, session_id: &str) -> Result<Vec<SessionMessage>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, session_id, role, content, tool_call_id, tool_calls,
                        tool_name, timestamp, finish_reason
                 FROM messages
                 WHERE session_id = ?1
                 ORDER BY id ASC",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map(params![session_id], row_to_message)
            .map_err(|e| CoreError::Session(format!("Failed to get messages: {e}")))?;

        let mut messages = Vec::new();
        for row in rows {
            messages.push(
                row.map_err(|e| CoreError::Session(format!("Failed to read message row: {e}")))?,
            );
        }
        Ok(messages)
    }

    /// Update token usage columns on a session row.
    pub fn update_token_counts(
        &self,
        session_id: &str,
        usage: &TokenUsage,
    ) -> Result<(), CoreError> {
        let rows = self
            .conn
            .execute(
                "UPDATE sessions
                 SET prompt_tokens = ?1, completion_tokens = ?2, total_tokens = ?3
                 WHERE id = ?4",
                params![
                    usage.prompt_tokens,
                    usage.completion_tokens,
                    usage.total_tokens,
                    session_id,
                ],
            )
            .map_err(|e| CoreError::Session(format!("Failed to update token counts: {e}")))?;

        if rows == 0 {
            return Err(CoreError::Session(format!(
                "Session not found: {session_id}"
            )));
        }
        Ok(())
    }

    /// Search messages using FTS5 full-text search.
    ///
    /// Returns matching messages ranked by relevance (BM25), excluding the
    /// given session.  Uses the `simple_query()` SQL function from libsimple
    /// to tokenize the query with jieba (Chinese) + default (English).
    pub fn search_messages(
        &self,
        query: &str,
        exclude_session_id: Option<&str>,
        role_filter: Option<&str>,
        limit: u32,
    ) -> Result<Vec<super::types::MessageMatch>, CoreError> {
        let exclude = exclude_session_id.unwrap_or("");

        let (sql, roles): (String, Vec<String>) = if let Some(filter) = role_filter {
            let role_list: Vec<String> = filter.split(',').map(|r| r.trim().to_string()).collect();
            let placeholders: String = role_list
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 3))
                .collect::<Vec<_>>()
                .join(",");
            (
                format!(
                    "SELECT m.session_id, m.content, m.role, rank, m.id \
                     FROM messages_fts \
                     JOIN messages m ON messages_fts.rowid = m.id \
                     WHERE messages_fts MATCH simple_query(?1) \
                       AND m.session_id != ?2 \
                       AND m.role IN ({placeholders}) \
                     ORDER BY rank \
                     LIMIT {limit}"
                ),
                role_list,
            )
        } else {
            (
                format!(
                    "SELECT m.session_id, m.content, m.role, rank, m.id \
                     FROM messages_fts \
                     JOIN messages m ON messages_fts.rowid = m.id \
                     WHERE messages_fts MATCH simple_query(?1) \
                       AND m.session_id != ?2 \
                     ORDER BY rank \
                     LIMIT {limit}"
                ),
                vec![],
            )
        };

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| CoreError::Session(format!("Failed to prepare FTS5 query: {e}")))?;

        let mut idx = 1;
        stmt.raw_bind_parameter(idx, query)
            .map_err(|e| CoreError::Session(format!("Failed to bind query: {e}")))?;
        idx += 1;
        stmt.raw_bind_parameter(idx, exclude)
            .map_err(|e| CoreError::Session(format!("Failed to bind exclude: {e}")))?;
        idx += 1;
        for role in &roles {
            stmt.raw_bind_parameter(idx, role.as_str())
                .map_err(|e| CoreError::Session(format!("Failed to bind role: {e}")))?;
            idx += 1;
        }

        let mut results = Vec::new();
        let mut rows = stmt.raw_query();
        while let Some(row) = rows
            .next()
            .map_err(|e| CoreError::Session(format!("Failed to read FTS5 row: {e}")))?
        {
            results.push(super::types::MessageMatch {
                session_id: row.get(0).unwrap_or_default(),
                content: row.get(1).unwrap_or_default(),
                role: row.get(2).unwrap_or_default(),
                rank: row.get(3).unwrap_or(0.0),
                message_id: row.get(4).unwrap_or(0),
            });
        }
        Ok(results)
    }

    /// List sessions that have at least one user message, ordered by
    /// `started_at` DESC. Filters out empty / system-only sessions.
    pub fn list_non_empty_sessions(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Session>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, model, system_prompt, parent_session_id, started_at,
                        ended_at, end_reason, message_count, tool_call_count, title
                 FROM sessions
                 WHERE EXISTS (
                    SELECT 1 FROM messages m
                    WHERE m.session_id = sessions.id AND m.role = 'user'
                 )
                 ORDER BY started_at DESC
                 LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare query: {e}")))?;

        let rows = stmt
            .query_map(params![limit, offset], row_to_session)
            .map_err(|e| CoreError::Session(format!("Failed to list sessions: {e}")))?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(
                row.map_err(|e| CoreError::Session(format!("Failed to read session row: {e}")))?,
            );
        }
        Ok(sessions)
    }

    /// Read the first user message content of a session, used as a display
    /// title fallback when `title` is NULL.
    pub fn first_user_message(&self, session_id: &str) -> Result<Option<String>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT content FROM messages
                 WHERE session_id = ?1 AND role = 'user'
                 ORDER BY id ASC LIMIT 1",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare query: {e}")))?;

        let mut rows = stmt
            .query_map(params![session_id], |row| row.get::<_, Option<String>>(0))
            .map_err(|e| CoreError::Session(format!("Failed to query first user message: {e}")))?;

        match rows.next() {
            Some(Ok(content)) => Ok(content),
            Some(Err(e)) => Err(CoreError::Session(format!(
                "Failed to read first user message: {e}"
            ))),
            None => Ok(None),
        }
    }

    /// Update the `title` column for a session.
    pub fn update_session_title(
        &self,
        session_id: &str,
        title: Option<&str>,
    ) -> Result<(), CoreError> {
        let rows = self
            .conn
            .execute(
                "UPDATE sessions SET title = ?1 WHERE id = ?2",
                params![title, session_id],
            )
            .map_err(|e| CoreError::Session(format!("Failed to update session title: {e}")))?;

        if rows == 0 {
            return Err(CoreError::Session(format!(
                "Session not found: {session_id}"
            )));
        }
        Ok(())
    }

    /// Delete a session and all of its messages.
    pub fn delete_session(&self, session_id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "DELETE FROM messages WHERE session_id = ?1",
                params![session_id],
            )
            .map_err(|e| CoreError::Session(format!("Failed to delete messages: {e}")))?;

        let rows = self
            .conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .map_err(|e| CoreError::Session(format!("Failed to delete session: {e}")))?;

        if rows == 0 {
            return Err(CoreError::Session(format!(
                "Session not found: {session_id}"
            )));
        }
        Ok(())
    }

    pub fn create_cron_job(&self, input: NewCronJob) -> Result<CronJob, CoreError> {
        let now_epoch = unix_now();
        let next_epoch = if input.enabled {
            Some(next_run_epoch(&input.schedule, now_epoch)?)
        } else {
            None
        };
        let now = chrono_from_unix_secs(now_epoch);
        let id = Uuid::new_v4().to_string();
        let disabled_toolsets =
            serde_json::to_string(&input.disabled_toolsets).unwrap_or_else(|_| "[]".to_string());
        let next_run_at = next_epoch.map(chrono_from_unix_secs);

        self.conn
            .execute(
                "INSERT INTO cron_jobs (
                    id, name, prompt, schedule, enabled, model, disabled_toolsets,
                    created_at, updated_at, next_run_at, next_run_at_epoch,
                    last_run_at, running, failure_count
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, 0, 0)",
                params![
                    id,
                    input.name,
                    input.prompt,
                    input.schedule,
                    input.enabled as i32,
                    input.model,
                    disabled_toolsets,
                    now,
                    now,
                    next_run_at,
                    next_epoch,
                ],
            )
            .map_err(|e| CoreError::Session(format!("Failed to create cron job: {e}")))?;

        self.get_cron_job(&id)?
            .ok_or_else(|| CoreError::Session("created cron job not found".to_string()))
    }

    pub fn list_cron_jobs(&self) -> Result<Vec<CronJob>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, prompt, schedule, enabled, model, disabled_toolsets,
                        created_at, updated_at, next_run_at, next_run_at_epoch,
                        last_run_at, running, failure_count
                 FROM cron_jobs
                 ORDER BY created_at DESC",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare cron job list: {e}")))?;
        let rows = stmt
            .query_map([], row_to_cron_job)
            .map_err(|e| CoreError::Session(format!("Failed to list cron jobs: {e}")))?;
        collect_rows(rows, "cron job")
    }

    pub fn get_cron_job(&self, id: &str) -> Result<Option<CronJob>, CoreError> {
        self.conn
            .query_row(
                "SELECT id, name, prompt, schedule, enabled, model, disabled_toolsets,
                        created_at, updated_at, next_run_at, next_run_at_epoch,
                        last_run_at, running, failure_count
                 FROM cron_jobs WHERE id = ?1",
                params![id],
                row_to_cron_job,
            )
            .optional()
            .map_err(|e| CoreError::Session(format!("Failed to get cron job: {e}")))
    }

    pub fn update_cron_job(&self, id: &str, patch: CronJobPatch) -> Result<CronJob, CoreError> {
        let Some(mut job) = self.get_cron_job(id)? else {
            return Err(CoreError::Session(format!("Cron job not found: {id}")));
        };

        if let Some(name) = patch.name {
            job.name = name;
        }
        if let Some(prompt) = patch.prompt {
            job.prompt = prompt;
        }
        if let Some(schedule) = patch.schedule {
            job.schedule = schedule;
        }
        if let Some(enabled) = patch.enabled {
            job.enabled = enabled;
        }
        if let Some(model) = patch.model {
            job.model = model;
        }
        if let Some(disabled) = patch.disabled_toolsets {
            job.disabled_toolsets = disabled;
        }

        let now_epoch = unix_now();
        let updated_at = chrono_from_unix_secs(now_epoch);
        let next_epoch = if job.enabled && !job.running {
            Some(next_run_epoch(&job.schedule, now_epoch)?)
        } else {
            None
        };
        let next_run_at = next_epoch.map(chrono_from_unix_secs);
        let disabled_toolsets =
            serde_json::to_string(&job.disabled_toolsets).unwrap_or_else(|_| "[]".to_string());

        let rows = self
            .conn
            .execute(
                "UPDATE cron_jobs
                 SET name = ?1, prompt = ?2, schedule = ?3, enabled = ?4,
                     model = ?5, disabled_toolsets = ?6, updated_at = ?7,
                     next_run_at = ?8, next_run_at_epoch = ?9
                 WHERE id = ?10",
                params![
                    job.name,
                    job.prompt,
                    job.schedule,
                    job.enabled as i32,
                    job.model,
                    disabled_toolsets,
                    updated_at,
                    next_run_at,
                    next_epoch,
                    id,
                ],
            )
            .map_err(|e| CoreError::Session(format!("Failed to update cron job: {e}")))?;

        if rows == 0 {
            return Err(CoreError::Session(format!("Cron job not found: {id}")));
        }
        self.get_cron_job(id)?
            .ok_or_else(|| CoreError::Session(format!("Cron job not found after update: {id}")))
    }

    pub fn delete_cron_job(&self, id: &str) -> Result<(), CoreError> {
        self.conn
            .execute("DELETE FROM cron_runs WHERE job_id = ?1", params![id])
            .map_err(|e| CoreError::Session(format!("Failed to delete cron runs: {e}")))?;
        let rows = self
            .conn
            .execute("DELETE FROM cron_jobs WHERE id = ?1", params![id])
            .map_err(|e| CoreError::Session(format!("Failed to delete cron job: {e}")))?;
        if rows == 0 {
            return Err(CoreError::Session(format!("Cron job not found: {id}")));
        }
        Ok(())
    }

    pub fn schedule_cron_job_now(&self, id: &str) -> Result<CronJob, CoreError> {
        let Some(job) = self.get_cron_job(id)? else {
            return Err(CoreError::Session(format!("Cron job not found: {id}")));
        };
        if job.running {
            return Err(CoreError::Session(format!(
                "Cron job already running: {id}"
            )));
        }
        if !job.enabled {
            return Err(CoreError::Session(format!(
                "Cron job is paused; resume it before running: {id}"
            )));
        }

        let now_epoch = unix_now();
        let next_run_at = chrono_from_unix_secs(now_epoch);
        self.conn
            .execute(
                "UPDATE cron_jobs
                 SET next_run_at = ?1, next_run_at_epoch = ?2
                 WHERE id = ?3",
                params![next_run_at, now_epoch, id],
            )
            .map_err(|e| CoreError::Session(format!("Failed to schedule cron job now: {e}")))?;

        self.get_cron_job(id)?
            .ok_or_else(|| CoreError::Session(format!("Cron job not found after update: {id}")))
    }

    pub fn due_cron_jobs(&self, now_epoch: i64, limit: u32) -> Result<Vec<CronJob>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, prompt, schedule, enabled, model, disabled_toolsets,
                        created_at, updated_at, next_run_at, next_run_at_epoch,
                        last_run_at, running, failure_count
                 FROM cron_jobs
                 WHERE enabled = 1 AND running = 0
                   AND next_run_at_epoch IS NOT NULL
                   AND next_run_at_epoch <= ?1
                 ORDER BY next_run_at_epoch ASC
                 LIMIT ?2",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare due cron query: {e}")))?;
        let rows = stmt
            .query_map(params![now_epoch, limit], row_to_cron_job)
            .map_err(|e| CoreError::Session(format!("Failed to query due cron jobs: {e}")))?;
        collect_rows(rows, "cron job")
    }

    pub fn start_cron_run(&self, job_id: &str) -> Result<CronRun, CoreError> {
        let Some(job) = self.get_cron_job(job_id)? else {
            return Err(CoreError::Session(format!("Cron job not found: {job_id}")));
        };
        if job.running {
            return Err(CoreError::Session(format!(
                "Cron job already running: {job_id}"
            )));
        }

        let now_epoch = unix_now();
        let started_at = chrono_from_unix_secs(now_epoch);
        let changed = self
            .conn
            .execute(
                "UPDATE cron_jobs
                 SET running = 1, next_run_at = NULL, next_run_at_epoch = NULL
                 WHERE id = ?1 AND running = 0",
                params![job_id],
            )
            .map_err(|e| CoreError::Session(format!("Failed to mark cron job running: {e}")))?;
        if changed == 0 {
            return Err(CoreError::Session(format!(
                "Cron job already running: {job_id}"
            )));
        }

        self.conn
            .execute(
                "INSERT INTO cron_runs (job_id, started_at, status)
                 VALUES (?1, ?2, 'running')",
                params![job_id, started_at],
            )
            .map_err(|e| CoreError::Session(format!("Failed to create cron run: {e}")))?;
        let run_id = self.conn.last_insert_rowid();
        self.get_cron_run(run_id)?
            .ok_or_else(|| CoreError::Session("created cron run not found".to_string()))
    }

    pub fn finish_cron_run(
        &self,
        run_id: i64,
        job: &CronJob,
        finish: CronRunFinish,
    ) -> Result<CronRun, CoreError> {
        let now_epoch = unix_now();
        let finished_at = chrono_from_unix_secs(now_epoch);
        let next_epoch = if job.enabled {
            Some(next_run_epoch(&job.schedule, now_epoch)?)
        } else {
            None
        };
        let next_run_at = next_epoch.map(chrono_from_unix_secs);
        let failed = finish.status != "succeeded";

        self.conn
            .execute(
                "UPDATE cron_runs
                 SET finished_at = ?1, status = ?2, output = ?3, error = ?4,
                     prompt_tokens = ?5, completion_tokens = ?6, total_tokens = ?7,
                     duration_ms = ?8, session_id = ?9
                 WHERE id = ?10",
                params![
                    finished_at,
                    finish.status,
                    finish.output,
                    finish.error,
                    finish.prompt_tokens,
                    finish.completion_tokens,
                    finish.total_tokens,
                    finish.duration_ms,
                    finish.session_id,
                    run_id,
                ],
            )
            .map_err(|e| CoreError::Session(format!("Failed to finish cron run: {e}")))?;

        self.conn
            .execute(
                "UPDATE cron_jobs
                 SET running = 0, last_run_at = ?1, next_run_at = ?2,
                     next_run_at_epoch = ?3,
                     failure_count = CASE WHEN ?4 THEN failure_count + 1 ELSE 0 END
                 WHERE id = ?5",
                params![finished_at, next_run_at, next_epoch, failed, job.id],
            )
            .map_err(|e| CoreError::Session(format!("Failed to update cron job after run: {e}")))?;

        self.get_cron_run(run_id)?
            .ok_or_else(|| CoreError::Session(format!("Cron run not found: {run_id}")))
    }

    pub fn list_cron_runs(&self, job_id: &str, limit: u32) -> Result<Vec<CronRun>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, job_id, started_at, finished_at, status, output, error,
                        prompt_tokens, completion_tokens, total_tokens,
                        duration_ms, session_id
                 FROM cron_runs
                 WHERE job_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )
            .map_err(|e| CoreError::Session(format!("Failed to prepare cron run list: {e}")))?;
        let rows = stmt
            .query_map(params![job_id, limit], row_to_cron_run)
            .map_err(|e| CoreError::Session(format!("Failed to list cron runs: {e}")))?;
        collect_rows(rows, "cron run")
    }

    pub fn get_cron_run(&self, id: i64) -> Result<Option<CronRun>, CoreError> {
        self.conn
            .query_row(
                "SELECT id, job_id, started_at, finished_at, status, output, error,
                        prompt_tokens, completion_tokens, total_tokens,
                        duration_ms, session_id
                 FROM cron_runs WHERE id = ?1",
                params![id],
                row_to_cron_run,
            )
            .optional()
            .map_err(|e| CoreError::Session(format!("Failed to get cron run: {e}")))
    }

    pub fn reset_stale_cron_running_flags(&self) -> Result<(), CoreError> {
        self.conn
            .execute("UPDATE cron_jobs SET running = 0 WHERE running = 1", [])
            .map_err(|e| CoreError::Session(format!("Failed to reset cron running flags: {e}")))?;
        self.conn
            .execute(
                "UPDATE cron_runs
                 SET status = 'failed', finished_at = ?1, error = 'server restarted before run completed'
                 WHERE status = 'running'",
                params![chrono_now()],
            )
            .map_err(|e| CoreError::Session(format!("Failed to close stale cron runs: {e}")))?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, schedule FROM cron_jobs
                 WHERE enabled = 1 AND running = 0 AND next_run_at_epoch IS NULL",
            )
            .map_err(|e| {
                CoreError::Session(format!("Failed to prepare stale cron reschedule: {e}"))
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CoreError::Session(format!("Failed to query stale cron jobs: {e}")))?;
        let pairs = collect_rows(rows, "cron job schedule")?;
        drop(stmt);

        let now = unix_now();
        for (id, schedule) in pairs {
            let next_epoch = next_run_epoch(&schedule, now)?;
            let next_run_at = chrono_from_unix_secs(next_epoch);
            self.conn
                .execute(
                    "UPDATE cron_jobs
                     SET next_run_at = ?1, next_run_at_epoch = ?2
                     WHERE id = ?3",
                    params![next_run_at, next_epoch, id],
                )
                .map_err(|e| CoreError::Session(format!("Failed to reschedule cron job: {e}")))?;
        }
        Ok(())
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn collect_rows<T>(
    rows: impl Iterator<Item = SqlResult<T>>,
    label: &str,
) -> Result<Vec<T>, CoreError> {
    let mut items = Vec::new();
    for row in rows {
        items.push(row.map_err(|e| CoreError::Session(format!("Failed to read {label}: {e}")))?);
    }
    Ok(items)
}

fn row_to_session(row: &rusqlite::Row<'_>) -> SqlResult<Session> {
    Ok(Session {
        id: row.get(0)?,
        model: row.get(1)?,
        system_prompt: row.get(2)?,
        parent_session_id: row.get(3)?,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        end_reason: row.get(6)?,
        message_count: row.get(7)?,
        tool_call_count: row.get(8)?,
        title: row.get(9)?,
    })
}

fn row_to_message(row: &rusqlite::Row<'_>) -> SqlResult<SessionMessage> {
    Ok(SessionMessage {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        tool_call_id: row.get(4)?,
        tool_calls: row.get(5)?,
        tool_name: row.get(6)?,
        timestamp: row.get(7)?,
        finish_reason: row.get(8)?,
    })
}

fn row_to_cron_job(row: &rusqlite::Row<'_>) -> SqlResult<CronJob> {
    let disabled_json: String = row.get(6)?;
    let disabled_toolsets = serde_json::from_str(&disabled_json).unwrap_or_default();
    Ok(CronJob {
        id: row.get(0)?,
        name: row.get(1)?,
        prompt: row.get(2)?,
        schedule: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
        model: row.get(5)?,
        disabled_toolsets,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        next_run_at: row.get(9)?,
        next_run_at_epoch: row.get(10)?,
        last_run_at: row.get(11)?,
        running: row.get::<_, i64>(12)? != 0,
        failure_count: row.get(13)?,
    })
}

fn row_to_cron_run(row: &rusqlite::Row<'_>) -> SqlResult<CronRun> {
    Ok(CronRun {
        id: row.get(0)?,
        job_id: row.get(1)?,
        started_at: row.get(2)?,
        finished_at: row.get(3)?,
        status: row.get(4)?,
        output: row.get(5)?,
        error: row.get(6)?,
        prompt_tokens: row.get(7)?,
        completion_tokens: row.get(8)?,
        total_tokens: row.get(9)?,
        duration_ms: row.get(10)?,
        session_id: row.get(11)?,
    })
}

/// Returns the current UTC time as an ISO 8601 string without external deps.
pub fn chrono_now() -> String {
    chrono_from_unix_secs(unix_now())
}

pub fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Format a Unix epoch seconds value as ISO 8601 UTC without external deps.
pub fn chrono_from_unix_secs(secs: i64) -> String {
    // Format as ISO 8601: YYYY-MM-DDTHH:MM:SSZ
    let s = secs.max(0) as u64;
    let sec = s % 60;
    let min = (s / 60) % 60;
    let hour = (s / 3600) % 24;
    let days = s / 86400; // days since 1970-01-01

    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Gregorian calendar algorithm
    let mut year = 1970u64;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}
