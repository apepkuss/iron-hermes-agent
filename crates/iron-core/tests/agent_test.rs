use iron_core::agent::SessionState;
use iron_core::budget::IterationBudget;
use iron_core::llm::types::Message;

// ─── IterationBudget tests ───

#[test]
fn test_budget_consume_and_exhaust() {
    let budget = IterationBudget::new(3);
    assert!(budget.consume(), "1st consume should succeed");
    assert!(budget.consume(), "2nd consume should succeed");
    assert!(budget.consume(), "3rd consume should succeed");
    assert!(
        !budget.consume(),
        "4th consume should fail (budget exhausted)"
    );

    assert_eq!(budget.remaining(), 0);
    assert_eq!(budget.used(), 3);
}

#[test]
fn test_budget_refund() {
    let budget = IterationBudget::new(3);
    assert!(budget.consume());
    assert_eq!(budget.remaining(), 2);
    assert_eq!(budget.used(), 1);

    budget.refund();
    assert_eq!(budget.remaining(), 3);
    assert_eq!(budget.used(), 0);
}

#[test]
fn test_budget_warning_levels() {
    let budget = IterationBudget::new(10);

    // 0-6 used: no warning
    for _ in 0..6 {
        budget.consume();
    }
    assert!(budget.budget_warning().is_none(), "no warning at 60% usage");

    // 7 used (70%): caution
    budget.consume();
    let w = budget.budget_warning();
    assert!(w.is_some(), "should have caution at 70%");
    assert!(
        w.as_ref().unwrap().contains("CAUTION"),
        "should be CAUTION level"
    );

    // 8 used (80%): still caution
    budget.consume();
    let w = budget.budget_warning();
    assert!(w.is_some());
    assert!(w.as_ref().unwrap().contains("CAUTION"));

    // 9 used (90%): warning
    budget.consume();
    let w = budget.budget_warning();
    assert!(w.is_some(), "should have warning at 90%");
    assert!(
        w.as_ref().unwrap().contains("WARNING"),
        "should be WARNING level"
    );
}

// ─── SessionState tests ───

#[test]
fn test_session_state_message_management() {
    let mut session = SessionState::new("test-session-1".to_string());

    assert_eq!(session.session_id, "test-session-1");
    assert!(session.messages.is_empty());
    assert!(session.system_prompt.is_none());

    // Add messages.
    session.messages.push(Message {
        role: "user".to_string(),
        content: Some("Hello".to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    session.messages.push(Message {
        role: "assistant".to_string(),
        content: Some("Hi there!".to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    session.messages.push(Message {
        role: "user".to_string(),
        content: Some("How are you?".to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    });

    assert_eq!(session.messages.len(), 3);
    assert_eq!(session.messages[0].role, "user");
    assert_eq!(session.messages[0].content.as_deref(), Some("Hello"));
    assert_eq!(session.messages[1].role, "assistant");
    assert_eq!(session.messages[2].role, "user");
    assert_eq!(session.messages[2].content.as_deref(), Some("How are you?"));
}

#[test]
fn test_session_state_system_prompt() {
    let mut session = SessionState::new("test-session-2".to_string());
    assert!(session.system_prompt.is_none());

    session.system_prompt = Some("You are a helpful assistant.".to_string());
    assert_eq!(
        session.system_prompt.as_deref(),
        Some("You are a helpful assistant.")
    );
}
