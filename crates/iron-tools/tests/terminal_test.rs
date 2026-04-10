use iron_tools::terminal::{TerminalParams, TerminalTool};
use std::path::PathBuf;

#[tokio::test]
async fn test_execute_simple_command() {
    let tool = TerminalTool::new(30);
    let params = TerminalParams {
        command: "echo hello".to_string(),
        background: false,
        timeout: None,
        workdir: None,
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
    };
    let result = tool.execute(params).await.expect("execute should succeed");
    assert!(
        result.stdout.len() <= 102_400 + 512,
        "stdout should be <= ~100KB, got {} bytes",
        result.stdout.len()
    );
    assert!(result.truncated, "output should be marked as truncated");
}
