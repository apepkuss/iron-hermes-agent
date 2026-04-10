use iron_sandbox::sandbox::{Sandbox, SandboxConfig, SandboxStatus};
use iron_tools::registry::ToolRegistry;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

fn make_sandbox(config: SandboxConfig) -> Sandbox {
    let registry = Arc::new(ToolRegistry::new());
    let enabled: HashSet<String> = HashSet::new();
    Sandbox::new(config, registry, enabled)
}

#[tokio::test]
async fn test_execute_python_script() {
    let sandbox = make_sandbox(SandboxConfig::default());
    let result = sandbox
        .execute_python("print('hello from sandbox')")
        .await
        .expect("sandbox execute failed");

    assert_eq!(result.status, SandboxStatus::Success);
    assert!(
        result.stdout.contains("hello from sandbox"),
        "stdout was: {}",
        result.stdout
    );
}

#[tokio::test]
async fn test_execute_shell_script() {
    let sandbox = make_sandbox(SandboxConfig::default());
    let result = sandbox
        .execute_shell("echo 'hello shell'")
        .await
        .expect("sandbox execute failed");

    assert_eq!(result.status, SandboxStatus::Success);
    assert!(
        result.stdout.contains("hello shell"),
        "stdout was: {}",
        result.stdout
    );
}

#[tokio::test]
async fn test_timeout_kills_process() {
    let config = SandboxConfig {
        timeout: Duration::from_secs(2),
        ..Default::default()
    };
    let sandbox = make_sandbox(config);
    let result = sandbox
        .execute_python("import time; time.sleep(60)")
        .await
        .expect("sandbox execute failed");

    assert_eq!(result.status, SandboxStatus::Timeout);
}

#[tokio::test]
async fn test_env_vars_filtered() {
    // Set a secret-like env var in the parent process
    // SAFETY: single-threaded test context; no other threads access env at this point.
    unsafe {
        std::env::set_var("TEST_SECRET_KEY", "super_secret_value_12345");
    }

    let sandbox = make_sandbox(SandboxConfig::default());
    // Try to print the env var; if filtered it should be empty
    let result = sandbox
        .execute_python(
            r#"
import os
val = os.environ.get('TEST_SECRET_KEY', 'NOT_FOUND')
print(f'SECRET_VALUE={val}')
"#,
        )
        .await
        .expect("sandbox execute failed");

    assert_eq!(result.status, SandboxStatus::Success);
    // The secret should not be visible inside the sandbox
    assert!(
        result.stdout.contains("NOT_FOUND"),
        "Expected secret to be filtered, but stdout was: {}",
        result.stdout
    );

    // Clean up
    // SAFETY: single-threaded test context.
    unsafe {
        std::env::remove_var("TEST_SECRET_KEY");
    }
}
