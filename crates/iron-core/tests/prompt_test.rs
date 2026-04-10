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
    }
}

#[test]
fn test_build_with_defaults() {
    let ctx = minimal_ctx();
    let output = PromptBuilder::build(&ctx);

    // Default identity must be present
    assert!(
        output.contains("Iron Hermes"),
        "Expected default identity in output"
    );

    // Memory guidance always included
    assert!(
        output.contains("persistent memory"),
        "Expected memory guidance in output"
    );

    // Skills guidance always included
    assert!(
        output.contains("complex task"),
        "Expected skills guidance in output"
    );

    // Metadata must be present
    assert!(
        output.contains("2026-04-10"),
        "Expected current_date in output"
    );
    assert!(
        output.contains("sess-abc123"),
        "Expected session_id in output"
    );
    assert!(
        output.contains("claude-opus-4"),
        "Expected model_name in output"
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
    };

    let output = PromptBuilder::build(&ctx);

    // All sections present
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

    // Verify order: identity before memory guidance before skills guidance
    let identity_pos = output.find("Custom identity text").unwrap();
    let mem_guidance_pos = output.find("persistent memory").unwrap();
    let skills_guidance_pos = output.find("complex task").unwrap();
    let custom_msg_pos = output.find("Always respond in JSON.").unwrap();
    let memory_block_pos = output.find("MEMORY: user prefers dark mode").unwrap();
    let skills_index_pos = output.find("SKILLS: rust-debugging-v1").unwrap();
    let context_file_pos = output.find("AGENTS.md content here").unwrap();
    let metadata_pos = output.find("sess-xyz999").unwrap();

    assert!(
        identity_pos < mem_guidance_pos,
        "identity must come before memory guidance"
    );
    assert!(
        mem_guidance_pos < skills_guidance_pos,
        "memory guidance must come before skills guidance"
    );
    assert!(
        skills_guidance_pos < custom_msg_pos,
        "skills guidance must come before custom system message"
    );
    assert!(
        custom_msg_pos < memory_block_pos,
        "custom message must come before memory block"
    );
    assert!(
        memory_block_pos < skills_index_pos,
        "memory block must come before skills index"
    );
    assert!(
        skills_index_pos < context_file_pos,
        "skills index must come before context files"
    );
    assert!(
        context_file_pos < metadata_pos,
        "context files must come before metadata"
    );
}

#[test]
fn test_build_with_custom_identity() {
    let ctx = PromptContext {
        identity: Some("I am IronHermes, an expert coding assistant.".to_string()),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    // Custom identity present
    assert!(output.contains("IronHermes"));
    // Default identity NOT present
    assert!(
        !output.contains("helpful AI assistant"),
        "Default identity should not appear when custom identity is set"
    );
}

#[test]
fn test_build_includes_memory_block() {
    let ctx = PromptContext {
        memory_block: Some("user_name: Alice\nprefers_dark_mode: true".to_string()),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("user_name: Alice"));
    assert!(output.contains("prefers_dark_mode: true"));

    // Memory block must appear after memory guidance
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
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    assert!(output.contains("rust-error-handling"));
    assert!(output.contains("cargo-workspace-setup"));

    // Skills index must appear after skills guidance
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

    assert!(
        output.contains("2026-04-10"),
        "date must appear in metadata"
    );
    assert!(
        output.contains("sess-meta-test"),
        "session_id must appear in metadata"
    );
    assert!(
        output.contains("claude-haiku-4"),
        "model_name must appear in metadata"
    );
}

#[test]
fn test_sections_separated_by_double_newline() {
    let ctx = PromptContext {
        memory_block: Some("mem-data".to_string()),
        ..minimal_ctx()
    };

    let output = PromptBuilder::build(&ctx);

    // Sections must be separated by double newlines (not just single)
    assert!(
        output.contains("\n\n"),
        "Sections must be separated by double newlines"
    );
}
