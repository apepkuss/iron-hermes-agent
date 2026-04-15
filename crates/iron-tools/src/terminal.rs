//! Terminal tool — executes shell commands with timeout and output truncation.

use crate::error::ToolError;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Maximum output size in bytes (100 KB).
const MAX_OUTPUT_BYTES: usize = 102_400;

/// Parameters for a terminal command execution.
pub struct TerminalParams {
    pub command: String,
    pub background: bool,
    pub timeout: Option<u64>,
    pub workdir: Option<PathBuf>,
    /// Session-level safe environment variables.
    /// When `Some`, `cmd.env_clear()` + `cmd.envs()` is applied to prevent
    /// sensitive information from leaking into subprocesses.
    pub env_vars: Option<HashMap<String, String>>,
}

/// Result of a terminal command execution.
pub struct TerminalResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub truncated: bool,
}

/// Tool for executing shell commands.
pub struct TerminalTool {
    default_timeout: Duration,
}

impl TerminalTool {
    /// Create a new `TerminalTool` with a default timeout in seconds.
    pub fn new(default_timeout_secs: u64) -> Self {
        Self {
            default_timeout: Duration::from_secs(default_timeout_secs),
        }
    }

    /// Execute a shell command with the given parameters.
    pub async fn execute(&self, params: TerminalParams) -> Result<TerminalResult, ToolError> {
        let timeout = params
            .timeout
            .map(Duration::from_secs)
            .unwrap_or(self.default_timeout);

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&params.command);
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        if let Some(dir) = &params.workdir {
            cmd.current_dir(dir);
        }

        // Environment isolation: use session-level safe env vars to prevent
        // sensitive information (API keys, tokens, etc.) from leaking.
        if let Some(ref env_vars) = params.env_vars {
            cmd.env_clear();
            cmd.envs(env_vars);
        }

        // Process group isolation so we can kill the whole group on timeout.
        // SAFETY: setsid() is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to spawn command: {e}")))?;

        // Take stdout/stderr handles before awaiting.
        let mut stdout_handle = child.stdout.take().expect("stdout is piped");
        let mut stderr_handle = child.stderr.take().expect("stderr is piped");

        if params.background {
            // Fire and forget — don't wait for completion.
            return Ok(TerminalResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                truncated: false,
            });
        }

        // Read stdout and stderr concurrently while enforcing the timeout.
        let read_result = tokio::time::timeout(timeout, async {
            let stdout_fut = async {
                let mut buf = Vec::new();
                stdout_handle.read_to_end(&mut buf).await.ok();
                buf
            };
            let stderr_fut = async {
                let mut buf = Vec::new();
                stderr_handle.read_to_end(&mut buf).await.ok();
                buf
            };
            tokio::join!(stdout_fut, stderr_fut)
        })
        .await;

        match read_result {
            Ok((stdout_bytes, stderr_bytes)) => {
                // Wait for the child to finish.
                let status = child.wait().await.map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to wait for child: {e}"))
                })?;

                let exit_code = status.code().unwrap_or(-1);
                let (stdout, stdout_truncated) = truncate_output(stdout_bytes);
                let (stderr, _) = truncate_output(stderr_bytes);

                Ok(TerminalResult {
                    stdout,
                    stderr,
                    exit_code,
                    truncated: stdout_truncated,
                })
            }
            Err(_elapsed) => {
                // Timeout: send SIGTERM to the process group.
                let pid = child.id().unwrap_or(0) as i32;
                if pid > 0 {
                    unsafe {
                        libc::kill(-pid, libc::SIGTERM);
                    }
                }

                // Wait up to 5 seconds for graceful shutdown, then SIGKILL.
                let graceful = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
                if graceful.is_err() {
                    if pid > 0 {
                        unsafe {
                            libc::kill(-pid, libc::SIGKILL);
                        }
                    }
                    let _ = child.wait().await;
                }

                Ok(TerminalResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 124,
                    truncated: false,
                })
            }
        }
    }
}

/// Truncate raw bytes to at most `MAX_OUTPUT_BYTES`, using a head-40% / tail-60% strategy.
/// Returns the (possibly truncated) UTF-8 string and a flag indicating truncation.
fn truncate_output(bytes: Vec<u8>) -> (String, bool) {
    if bytes.len() <= MAX_OUTPUT_BYTES {
        let s = String::from_utf8_lossy(&bytes).into_owned();
        return (s, false);
    }

    let head_len = MAX_OUTPUT_BYTES * 40 / 100;
    let tail_len = MAX_OUTPUT_BYTES - head_len;

    let head = String::from_utf8_lossy(&bytes[..head_len]).into_owned();
    let tail = String::from_utf8_lossy(&bytes[bytes.len() - tail_len..]).into_owned();

    let notice = format!(
        "\n\n[... {} bytes truncated ...]\n\n",
        bytes.len() - MAX_OUTPUT_BYTES
    );

    (head + &notice + &tail, true)
}
