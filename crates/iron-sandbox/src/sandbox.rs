use crate::bridge::{SANDBOX_TOOL_WHITELIST, generate_python_bridge, generate_shell_bridge};
use crate::error::SandboxError;
use crate::rpc::RpcServer;
use iron_tools::registry::ToolRegistry;
use regex::Regex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum SandboxStatus {
    Success,
    Error,
    Timeout,
    Interrupted,
}

pub struct SandboxConfig {
    pub timeout: Duration,
    pub max_tool_calls: u32,
    pub max_stdout: usize,
    pub max_stderr: usize,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(300),
            max_tool_calls: 50,
            max_stdout: 51200,
            max_stderr: 10240,
        }
    }
}

pub struct SandboxResult {
    pub status: SandboxStatus,
    pub stdout: String,
    pub stderr: String,
    pub tool_calls_made: u32,
    pub duration: Duration,
}

pub struct Sandbox {
    config: SandboxConfig,
    registry: Arc<ToolRegistry>,
    allowed_tools: HashSet<String>,
}

/// Env var prefixes considered safe to pass into sandbox.
const SAFE_PREFIXES: &[&str] = &[
    "PATH", "HOME", "USER", "LANG", "LC_", "TERM", "TMPDIR", "TZ", "SHELL",
];

/// Substrings that indicate a secret env var — block these.
const SECRET_PATTERNS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "CREDENTIAL",
    "PASSWD",
    "AUTH",
];

fn is_safe_env_var(name: &str) -> bool {
    let upper = name.to_uppercase();
    // If it contains a secret pattern, block it.
    for pat in SECRET_PATTERNS {
        if upper.contains(pat) {
            return false;
        }
    }
    // Allow if it starts with a safe prefix.
    for prefix in SAFE_PREFIXES {
        if upper.starts_with(prefix) {
            return true;
        }
    }
    false
}

fn collect_safe_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(name, _)| is_safe_env_var(name))
        .collect()
}

/// Redact common secret patterns from output strings.
fn redact_secrets(s: &str) -> String {
    // Match patterns like api_key=VALUE, token=VALUE, etc.
    let re = Regex::new(r"(?i)(api_?key|token|secret|password|passwd|credential|auth)[=:]\s*\S+")
        .expect("static regex is valid");
    re.replace_all(s, |caps: &regex::Captures| {
        // Keep the key name, replace value with [REDACTED]
        let key_part = &caps[1];
        format!("{}=[REDACTED]", key_part)
    })
    .to_string()
}

impl Sandbox {
    pub fn new(
        config: SandboxConfig,
        registry: Arc<ToolRegistry>,
        enabled_tools: HashSet<String>,
    ) -> Self {
        let whitelist: HashSet<String> = SANDBOX_TOOL_WHITELIST
            .iter()
            .map(|s| s.to_string())
            .collect();
        let allowed_tools: HashSet<String> =
            enabled_tools.intersection(&whitelist).cloned().collect();
        Self {
            config,
            registry,
            allowed_tools,
        }
    }

    /// Execute a Python code snippet inside the sandbox.
    pub async fn execute_python(&self, code: &str) -> Result<SandboxResult, SandboxError> {
        let tmp = tempfile::tempdir()?;
        let socket_path = tmp.path().join("rpc.sock");
        let script_path = tmp.path().join("script.py");

        let allowed_vec: Vec<&str> = self.allowed_tools.iter().map(|s| s.as_str()).collect();
        let bridge = generate_python_bridge(socket_path.to_str().unwrap(), &allowed_vec);

        let full_script = format!("{}\n{}", bridge, code);
        std::fs::write(&script_path, full_script)?;

        self.execute_subprocess(
            "python3",
            &[script_path.to_str().unwrap()],
            tmp.path().to_path_buf(),
            socket_path,
        )
        .await
    }

    /// Execute a Shell script snippet inside the sandbox.
    pub async fn execute_shell(&self, code: &str) -> Result<SandboxResult, SandboxError> {
        let tmp = tempfile::tempdir()?;
        let socket_path = tmp.path().join("rpc.sock");
        let script_path = tmp.path().join("script.sh");

        let allowed_vec: Vec<&str> = self.allowed_tools.iter().map(|s| s.as_str()).collect();
        let bridge = generate_shell_bridge(socket_path.to_str().unwrap(), &allowed_vec);

        let full_script = format!("{}\n{}", bridge, code);
        std::fs::write(&script_path, &full_script)?;

        // Make the script executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
        }

        self.execute_subprocess(
            "sh",
            &[script_path.to_str().unwrap()],
            tmp.path().to_path_buf(),
            socket_path,
        )
        .await
    }

    async fn execute_subprocess(
        &self,
        program: &str,
        args: &[&str],
        cwd: PathBuf,
        socket_path: PathBuf,
    ) -> Result<SandboxResult, SandboxError> {
        // Start RPC server
        let server = RpcServer::new(
            Arc::clone(&self.registry),
            self.allowed_tools.clone(),
            self.config.max_tool_calls,
        );
        let call_count_handle = server.serve(&socket_path).await?;

        // Collect safe env vars
        let safe_env = collect_safe_env();

        // Build command
        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(&cwd)
            .env_clear()
            .envs(safe_env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Use pre_exec to call setsid — creates a new process group
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        let start = Instant::now();

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return Err(SandboxError::Io(e)),
        };

        let pid = child.id();

        // Take stdout/stderr handles before awaiting
        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        let max_stdout = self.config.max_stdout;
        let max_stderr = self.config.max_stderr;

        // Read stdout and stderr concurrently
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(ref mut h) = stdout_handle {
                let _ = h.read_to_end(&mut buf).await;
            }
            buf.truncate(max_stdout);
            buf
        });

        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(ref mut h) = stderr_handle {
                let _ = h.read_to_end(&mut buf).await;
            }
            buf.truncate(max_stderr);
            buf
        });

        // Wait with timeout
        let timeout_result = tokio::time::timeout(self.config.timeout, child.wait()).await;

        let duration = start.elapsed();

        let status = match timeout_result {
            Ok(Ok(exit_status)) => {
                if exit_status.success() {
                    SandboxStatus::Success
                } else {
                    SandboxStatus::Error
                }
            }
            Ok(Err(_)) => SandboxStatus::Error,
            Err(_) => {
                // Timeout — kill the process group
                if let Some(pid) = pid {
                    let pgid = pid as libc::pid_t;
                    unsafe {
                        libc::kill(-pgid, libc::SIGTERM);
                    }
                    // Wait 5 seconds then SIGKILL
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    unsafe {
                        libc::kill(-pgid, libc::SIGKILL);
                    }
                }
                let _ = child.wait().await;
                SandboxStatus::Timeout
            }
        };

        let stdout_bytes = stdout_task.await.unwrap_or_default();
        let stderr_bytes = stderr_task.await.unwrap_or_default();

        let stdout_raw = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr_raw = String::from_utf8_lossy(&stderr_bytes).to_string();

        let stdout = redact_secrets(&stdout_raw);
        let stderr = redact_secrets(&stderr_raw);

        // Cleanup socket
        let _ = std::fs::remove_file(&socket_path);

        let tool_calls_made = call_count_handle.load(std::sync::atomic::Ordering::SeqCst);

        Ok(SandboxResult {
            status,
            stdout,
            stderr,
            tool_calls_made,
            duration,
        })
    }
}
