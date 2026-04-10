use rusqlite::{Connection, Result as SqlResult, params};

use crate::error::CoreError;

use super::types::{Session, SessionMessage, TokenUsage};

/// SQLite-backed session store.
pub struct SessionStore {
    conn: Connection,
}

impl SessionStore {
    /// Open (or create) a SQLite database at `db_path`.
    pub fn new(db_path: &str) -> Result<Self, CoreError> {
        let conn = Connection::open(db_path)
            .map_err(|e| CoreError::Session(format!("Failed to open database: {e}")))?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    /// Create an in-memory SQLite database (primarily for testing).
    pub fn new_in_memory() -> Result<Self, CoreError> {
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
                ",
            )
            .map_err(|e| CoreError::Session(format!("Failed to create tables: {e}")))?;

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
}

// ── helpers ──────────────────────────────────────────────────────────────────

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

/// Returns the current UTC time as an ISO 8601 string without external deps.
fn chrono_now() -> String {
    // Use std::time for a simple UTC timestamp.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Format as ISO 8601: YYYY-MM-DDTHH:MM:SSZ
    let s = secs;
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
