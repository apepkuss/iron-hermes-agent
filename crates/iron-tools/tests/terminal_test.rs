use iron_tools::terminal::{TerminalParams, TerminalTool};
use std::collections::HashMap;
use std::path::PathBuf;

#[tokio::test]
async fn test_execute_simple_command() {
    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "echo hello".to_string(),
        background: false,
        timeout: None,
        workdir: None,
        env_vars: None,
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("hello"),
        "stdout should contain 'hello'"
    );
}

#[tokio::test]
async fn test_execute_with_workdir() {
    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "pwd".to_string(),
        background: false,
        timeout: None,
        workdir: Some(PathBuf::from("/tmp")),
        env_vars: None,
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("tmp"),
        "stdout should contain 'tmp', got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn test_execute_timeout() {
    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "sleep 60".to_string(),
        background: false,
        timeout: Some(1),
        workdir: None,
        env_vars: None,
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_ne!(
        result.exit_code, 0,
        "timed out command should have non-zero exit code"
    );
}

#[tokio::test]
async fn test_execute_failing_command() {
    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "false".to_string(),
        background: false,
        timeout: None,
        workdir: None,
        env_vars: None,
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_ne!(
        result.exit_code, 0,
        "failing command should have non-zero exit code"
    );
}

#[tokio::test]
async fn test_output_truncation() {
    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "yes | head -200000".to_string(),
        background: false,
        timeout: Some(10),
        workdir: None,
        env_vars: None,
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert!(
        result.stdout.len() <= 102_400 + 512,
        "stdout should be <= ~100KB, got {} bytes",
        result.stdout.len()
    );
    assert!(result.truncated, "output should be marked as truncated");
}

#[tokio::test]
async fn test_env_vars_isolation_blocks_secrets() {
    // Set a fake secret in the process environment for this test.
    // SAFETY: test runs single-threaded for this env var; no concurrent readers.
    unsafe { std::env::set_var("TEST_SECRET_API_KEY", "super-secret-value") };

    let mut safe_env = HashMap::new();
    safe_env.insert(
        "PATH".to_string(),
        std::env::var("PATH").unwrap_or_default(),
    );
    safe_env.insert(
        "HOME".to_string(),
        std::env::var("HOME").unwrap_or_default(),
    );

    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "env".to_string(),
        background: false,
        timeout: None,
        workdir: None,
        env_vars: Some(safe_env),
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_eq!(result.exit_code, 0);
    assert!(
        !result.stdout.contains("super-secret-value"),
        "env output should NOT contain the secret value"
    );
    assert!(
        !result.stdout.contains("TEST_SECRET_API_KEY"),
        "env output should NOT contain the secret key name"
    );

    // Clean up
    // SAFETY: test cleanup; no concurrent readers.
    unsafe { std::env::remove_var("TEST_SECRET_API_KEY") };
}

#[tokio::test]
async fn test_env_vars_isolation_passes_safe_vars() {
    let mut safe_env = HashMap::new();
    safe_env.insert(
        "PATH".to_string(),
        std::env::var("PATH").unwrap_or_default(),
    );
    safe_env.insert("MY_TEST_VAR".to_string(), "test_value_123".to_string());

    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "echo $MY_TEST_VAR".to_string(),
        background: false,
        timeout: None,
        workdir: None,
        env_vars: Some(safe_env),
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("test_value_123"),
        "stdout should contain the safe var value, got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn test_env_vars_none_inherits_process_env() {
    // When env_vars is None, the command should inherit the process env.
    // SAFETY: test runs single-threaded for this env var; no concurrent readers.
    unsafe { std::env::set_var("TEST_INHERIT_VAR", "inherited_value") };

    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "echo $TEST_INHERIT_VAR".to_string(),
        background: false,
        timeout: None,
        workdir: None,
        env_vars: None,
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("inherited_value"),
        "without env_vars, process env should be inherited, got: {}",
        result.stdout
    );

    // SAFETY: test cleanup; no concurrent readers.
    unsafe { std::env::remove_var("TEST_INHERIT_VAR") };
}
