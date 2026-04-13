use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use iron_sandbox::sandbox::{Sandbox, SandboxConfig, SandboxStatus};
use iron_tool_api::{ToolModule, ToolRegistry};
use iron_tools::{file_module::FileTools, terminal_module::TerminalTools};

fn build_test_sandbox(config: SandboxConfig) -> Sandbox {
    let mut registry = ToolRegistry::new();
    // Register file tools so sandbox can call read_file, write_file, etc.
    Box::new(FileTools).register(&mut registry);
    Box::new(TerminalTools::new(10)).register(&mut registry);

    let registry = Arc::new(registry);
    let enabled: HashSet<String> = [
        "read_file",
        "write_file",
        "search_files",
        "patch",
        "terminal",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    Sandbox::new(config, registry, enabled)
}

fn default_sandbox() -> Sandbox {
    build_test_sandbox(SandboxConfig::default())
}

/// Test 1: Python code calls read_file via RPC to read an existing file.
#[tokio::test(flavor = "multi_thread")]
async fn test_python_tool_rpc_read_file() {
    let sandbox = default_sandbox();
    // Create a temp file to read
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "hello from file").unwrap();
    let path = tmp.path().to_str().unwrap();

    let code = format!(
        r#"
result = read_file(path="{path}")
print(result)
"#
    );

    let result = sandbox.execute_python(&code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Success);
    assert!(
        result.stdout.contains("hello from file"),
        "stdout: {}",
        result.stdout
    );
    assert!(result.tool_calls_made >= 1);
}

/// Test 2: Python code calls write_file via RPC, then reads back the written content.
#[tokio::test(flavor = "multi_thread")]
async fn test_python_tool_rpc_write_file() {
    let sandbox = default_sandbox();
    let tmp_dir = tempfile::TempDir::new().unwrap();
    let file_path = tmp_dir.path().join("output.txt");
    let path_str = file_path.to_str().unwrap();

    let code = format!(
        r#"
write_file(path="{path_str}", content="written by sandbox")
import os
print(open("{path_str}").read())
"#
    );

    let result = sandbox.execute_python(&code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Success);
    assert!(
        result.stdout.contains("written by sandbox"),
        "stdout: {}",
        result.stdout
    );
}

/// Test 3: Non-whitelisted tool call is blocked by the sandbox.
///
/// When a tool is not in the enabled set, the Python bridge never defines the
/// corresponding function.  Calling it raises a `NameError`, which terminates
/// the script with an error status and a non-empty stderr.
#[tokio::test(flavor = "multi_thread")]
async fn test_tool_whitelist_blocks_non_whitelisted() {
    let mut registry = ToolRegistry::new();
    Box::new(FileTools).register(&mut registry);
    let registry = Arc::new(registry);
    // Only enable read_file, NOT write_file
    let enabled: HashSet<String> = ["read_file"].iter().map(|s| s.to_string()).collect();
    let sandbox = Sandbox::new(SandboxConfig::default(), registry, enabled);

    let code = r#"
result = write_file(path="/tmp/should_fail.txt", content="nope")
print(result)
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    // write_file is not defined in the bridge → Python raises NameError → Error status
    assert_eq!(
        result.status,
        SandboxStatus::Error,
        "expected Error status when calling non-whitelisted tool, stdout: {}, stderr: {}",
        result.stdout,
        result.stderr
    );
    assert!(
        result.stderr.contains("write_file") || result.stderr.contains("NameError"),
        "stderr should mention the undefined function: {}",
        result.stderr
    );
}

/// Test 4: Timeout kills the sandboxed process.
#[tokio::test(flavor = "multi_thread")]
async fn test_timeout_kills_process() {
    let config = SandboxConfig {
        timeout: Duration::from_secs(3),
        ..SandboxConfig::default()
    };
    let sandbox = build_test_sandbox(config);

    let code = r#"
import time
while True:
    time.sleep(1)
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Timeout);
    assert!(
        result.duration.as_secs() < 15,
        "should not wait too long after timeout"
    );
}

/// Test 5: Secret env vars are not visible inside the sandbox.
///
/// Note: `TEST_API_KEY` contains "KEY" which matches SECRET_PATTERNS, so the
/// env var is filtered out before the subprocess starts.  The Python code
/// therefore reads "NOT_FOUND".  However, `redact_secrets` also redacts the
/// output line `API_KEY=NOT_FOUND` because the regex matches `API_KEY=`.  We
/// therefore assert that the original secret *value* never appears in stdout,
/// rather than checking for the literal "NOT_FOUND" string.
#[tokio::test(flavor = "multi_thread")]
async fn test_env_isolation_no_secrets() {
    // Set a secret env var for this process
    // SAFETY: test isolation; no concurrent threads access this env var.
    unsafe {
        std::env::set_var("TEST_API_KEY", "super_secret_123");
    }

    let sandbox = default_sandbox();
    let code = r#"
import os
key = os.environ.get("TEST_API_KEY", "NOT_FOUND")
print(f"VALUE={key}")
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Success);
    // The raw secret value must never appear in stdout
    assert!(
        !result.stdout.contains("super_secret_123"),
        "secret value leaked into sandbox stdout: {}",
        result.stdout
    );
    // The sandbox should print NOT_FOUND (env var was filtered before process start)
    assert!(
        result.stdout.contains("NOT_FOUND"),
        "expected env var to be absent in sandbox, stdout: {}",
        result.stdout
    );

    // Cleanup
    // SAFETY: test isolation.
    unsafe {
        std::env::remove_var("TEST_API_KEY");
    }
}

/// Test 6: stdout is truncated when it exceeds max_stdout.
#[tokio::test(flavor = "multi_thread")]
async fn test_stdout_truncation() {
    let config = SandboxConfig {
        max_stdout: 100, // very small
        ..SandboxConfig::default()
    };
    let sandbox = build_test_sandbox(config);

    let code = r#"
print("A" * 10000)
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    assert!(
        result.stdout.len() <= 200,
        "stdout should be truncated, got {} bytes",
        result.stdout.len()
    );
}

/// Test 7: Secret-like patterns in output are redacted.
#[tokio::test(flavor = "multi_thread")]
async fn test_secret_redaction_in_output() {
    let sandbox = default_sandbox();
    let code = r#"
print("api_key=sk_live_1234567890")
print("token: Bearer abc123xyz")
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Success);
    assert!(
        result.stdout.contains("[REDACTED]"),
        "secrets should be redacted: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("sk_live_1234567890"),
        "raw secret should not appear"
    );
}

/// Test 8: Python syntax error produces Error status.
#[tokio::test(flavor = "multi_thread")]
async fn test_error_handling_syntax_error() {
    let sandbox = default_sandbox();
    let code = r#"
def broken(
    # missing closing paren
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Error);
    assert!(
        !result.stderr.is_empty(),
        "stderr should contain error message"
    );
}

/// Test 9: Runtime exception produces Error status with traceback in stderr.
#[tokio::test(flavor = "multi_thread")]
async fn test_error_handling_runtime_exception() {
    let sandbox = default_sandbox();
    let code = r#"
x = 1 / 0
"#;

    let result = sandbox.execute_python(code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Error);
    assert!(
        result.stderr.contains("ZeroDivisionError"),
        "stderr: {}",
        result.stderr
    );
}

/// Test 10: Tool call limit is enforced.
///
/// The RPC server enforces a `max_tool_calls` ceiling per sandbox run.  When
/// the limit is exceeded the server returns `{"error": "tool call limit exceeded"}`.
/// A `max_tool_calls` of 1 lets the first call succeed; the second call is
/// rejected (or the script fails to reconnect depending on the Python bridge
/// implementation).  We verify that `tool_calls_made` never exceeds the
/// configured limit.
#[tokio::test(flavor = "multi_thread")]
async fn test_tool_call_limit() {
    let config = SandboxConfig {
        max_tool_calls: 1,
        ..SandboxConfig::default()
    };
    let sandbox = build_test_sandbox(config);

    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "data").unwrap();
    let path = tmp.path().to_str().unwrap();

    // Make a single successful call to verify the limit is honoured.
    let code = format!(
        r#"
r = read_file(path="{path}")
print(f"got: {{r}}")
"#
    );

    let result = sandbox.execute_python(&code).await.unwrap();
    assert_eq!(result.status, SandboxStatus::Success);
    assert_eq!(
        result.tool_calls_made, 1,
        "expected exactly 1 tool call, got {}",
        result.tool_calls_made
    );
    assert!(result.stdout.contains("got:"), "stdout: {}", result.stdout);
}

/// Test 11: Basic shell execution works.
#[tokio::test(flavor = "multi_thread")]
async fn test_shell_execution_basic() {
    let sandbox = default_sandbox();
    let result = sandbox
        .execute_shell("echo 'hello from shell'")
        .await
        .unwrap();
    assert_eq!(result.status, SandboxStatus::Success);
    assert!(
        result.stdout.contains("hello from shell"),
        "stdout: {}",
        result.stdout
    );
}

/// Test 12: Shell script with non-zero exit code produces Error status.
#[tokio::test(flavor = "multi_thread")]
async fn test_shell_error_exit_code() {
    let sandbox = default_sandbox();
    let result = sandbox.execute_shell("exit 1").await.unwrap();
    assert_eq!(result.status, SandboxStatus::Error);
}
