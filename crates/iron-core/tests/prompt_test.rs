use std::collections::HashSet;

use iron_core::prompt::{PromptBuilder, PromptContext};

fn minimal_ctx() -> PromptContext {
    PromptContext {
        identity: None,
        memory_block: None,
        skills_index: None,
        context_files: vec![],
        custom_system_message: None,
        model_name: "claude-opus-4".to_string(),
        session_id: "sess-abc123".to_string(),
        current_date: "2026-04-10".to_string(),
        available_tools: HashSet::new(),
    }
}

fn full_tools() -> HashSet<String> {
    [
        "memory",
        "skill_manage",
        "skills_list",
        "skill_view",
        "terminal",
        "read_file",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn test_build_with_defaults_no_tools() {
    let ctx = minimal_ctx();
    let output = PromptBuilder::build(&ctx);

    // Default identity must be present
    assert!(
        output.contains("Iron Hermes"),
        "Expected default identity in output"
    );

    // Memory guidance NOT included (no memory tool)
    assert!(
        !output.contains("persistent memory"),
        "Memory guidance should not appear without memory tool"
    );

    // Skills guidance NOT included (no skill_manage tool)
    assert!(
        !output.contains("complex task"),
        "Skills guidance should not appear without skill_manage tool"
    );

    // Metadata must be present
    assert!(output.contains("2026-04-10"));
    assert!(output.contains("sess-abc123"));
    assert!(output.contains("claude-opus-4"));
}

#[test]
fn test_build_with_memory_tool() {
    let ctx = PromptContext {
        available_tools: ["memory"].iter().map(|s| s.to_string()).collect(),
        ..minimal_ctx()
    };
    let output = PromptBuilder::build(&ctx);

    assert!(
        output.contains("persistent memory"),
        "Memory guidance should appear when memory tool is available"
    );
    assert!(
        !output.contains("complex task"),
        "Skills guidance should not appear without skill_manage tool"
    );
}

#[test]
fn test_build_with_skill_manage_tool() {
    let ctx = PromptContext {
        available_tools: ["skill_manage"].iter().map(|s| s.to_string()).collect(),
        ..minimal_ctx()
    };
    let output = PromptBuilder::build(&ctx);

    assert!(
        !output.contains("persistent memory"),
        "Memory guidance should not appear without memory tool"
    );
    assert!(
        output.contains("complex task"),
        "Skills guidance should appear when skill_manage tool is available"
    );
}

#[test]
fn test_build_with_all_fields() {
    let ctx = PromptContext {
        identity: Some("Custom identity text".to_string()),
        memory_block: Some("MEMORY: user prefers dark mode".to_string()),
        skills_index: Some("SKILLS: rust-debugging-v1".to_string()),
        context_files: vec![
            "AGENTS.md content here".to_string(),
            "Second context file".to_string(),
        ],
        custom_system_message: Some("Always respond in JSON.".to_string()),
        model_name: "claude-sonnet-4".to_string(),
        session_id: "sess-xyz999".to_string(),
        current_date: "2026-04-10".to_string(),
        available_tools: full_tools(),
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("Custom identity text"));
    assert!(output.contains("persistent memory"));
    assert!(output.contains("complex task"));
    assert!(output.contains("Always respond in JSON."));
    assert!(output.contains("MEMORY: user prefers dark mode"));
    assert!(output.contains("SKILLS: rust-debugging-v1"));
    assert!(output.contains("AGENTS.md content here"));
    assert!(output.contains("Second context file"));
    assert!(output.contains("2026-04-10"));
    assert!(output.contains("sess-xyz999"));
    assert!(output.contains("claude-sonnet-4"));
}

#[test]
fn test_build_with_custom_identity() {
    let ctx = PromptContext {
        identity: Some("I am IronHermes, an expert coding assistant.".to_string()),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("IronHermes"));
    assert!(
        !output.contains("helpful AI assistant"),
        "Default identity should not appear when custom identity is set"
    );
}

#[test]
fn test_build_includes_memory_block() {
    let ctx = PromptContext {
        memory_block: Some("user_name: Alice\nprefers_dark_mode: true".to_string()),
        available_tools: ["memory"].iter().map(|s| s.to_string()).collect(),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("user_name: Alice"));
    assert!(output.contains("prefers_dark_mode: true"));

    let guidance_pos = output.find("persistent memory").unwrap();
    let block_pos = output.find("user_name: Alice").unwrap();
    assert!(
        guidance_pos < block_pos,
        "memory block must appear after memory guidance"
    );
}

#[test]
fn test_build_includes_skills_index() {
    let ctx = PromptContext {
        skills_index: Some("skill:rust-error-handling\nskill:cargo-workspace-setup".to_string()),
        available_tools: ["skill_manage"].iter().map(|s| s.to_string()).collect(),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("rust-error-handling"));
    assert!(output.contains("cargo-workspace-setup"));

    let guidance_pos = output.find("complex task").unwrap();
    let index_pos = output.find("rust-error-handling").unwrap();
    assert!(
        guidance_pos < index_pos,
        "skills index must appear after skills guidance"
    );
}

#[test]
fn test_metadata_format() {
    let ctx = PromptContext {
        model_name: "claude-haiku-4".to_string(),
        session_id: "sess-meta-test".to_string(),
        current_date: "2026-04-10".to_string(),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("2026-04-10"));
    assert!(output.contains("sess-meta-test"));
    assert!(output.contains("claude-haiku-4"));
}

#[test]
fn test_sections_separated_by_double_newline() {
    let ctx = PromptContext {
        memory_block: Some("mem-data".to_string()),
        available_tools: ["memory"].iter().map(|s| s.to_string()).collect(),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(
        output.contains("\n\n"),
        "Sections must be separated by double newlines"
    );
}
