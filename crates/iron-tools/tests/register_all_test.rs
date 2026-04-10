use iron_tools::register_all::register_default_tools;
use std::collections::HashSet;

/// Verify that the core tools (terminal, read_file, write_file, patch,
/// search_files) are always registered regardless of environment.
#[test]
fn test_register_all_creates_expected_tools() {
    let registry = register_default_tools();

    let expected: HashSet<&str> = [
        "terminal",
        "read_file",
        "write_file",
        "patch",
        "search_files",
    ]
    .iter()
    .copied()
    .collect();

    for name in &expected {
        assert!(
            registry.has_tool(name),
            "expected tool '{name}' to be registered"
        );
    }
}

/// Verify that every registered tool schema has a non-empty name,
/// description, and parameters object.
#[test]
fn test_all_tools_have_valid_schemas() {
    use iron_tools::types::ToolContext;
    use std::path::PathBuf;

    let registry = register_default_tools();
    let tool_names = registry.tool_names();

    // Build a ToolContext that enables every registered tool.
    let ctx = ToolContext {
        task_id: "schema-check".to_string(),
        working_dir: PathBuf::from("/tmp"),
        enabled_tools: tool_names.clone(),
    };

    let schemas = registry.get_schemas(&ctx);

    // At least the 5 core tools must be present.
    assert!(
        schemas.len() >= 5,
        "expected at least 5 schemas, got {}",
        schemas.len()
    );

    for schema in &schemas {
        let func = &schema["function"];

        let name = func["name"].as_str().unwrap_or("");
        assert!(!name.is_empty(), "schema has empty name");

        let desc = func["description"].as_str().unwrap_or("");
        assert!(!desc.is_empty(), "schema '{name}' has empty description");

        assert!(
            func["parameters"].is_object(),
            "schema '{name}' parameters is not an object"
        );
    }
}
