use iron_memory::MemoryManager;
use tempfile::TempDir;

fn make_manager(dir: &TempDir) -> MemoryManager {
    let mut mgr = MemoryManager::new(dir.path(), None, None);
    mgr.initialize().expect("initialize failed");
    mgr
}

// ──────────────────────────────────────────────────────────────────────────────
// 1. system_prompt_block — empty store returns None
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_system_prompt_block_empty() {
    let dir = TempDir::new().unwrap();
    let mgr = make_manager(&dir);
    assert!(
        mgr.system_prompt_block().is_none(),
        "empty store should produce no block"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 2. system_prompt_block — add entry, reload, verify headers + content
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_system_prompt_block() {
    let dir = TempDir::new().unwrap();

    // Write data in a first manager instance (so it hits disk).
    {
        let mut mgr = MemoryManager::new(dir.path(), None, None);
        mgr.initialize().unwrap();

        let r = mgr
            .handle_tool_call("add", "memory", Some("User prefers Rust over Python"), None)
            .unwrap();
        assert!(r["success"].as_bool().unwrap(), "add should succeed");

        let r2 = mgr
            .handle_tool_call("add", "user", Some("Name: Alice"), None)
            .unwrap();
        assert!(r2["success"].as_bool().unwrap(), "add user should succeed");
    }

    // Reload in a fresh manager — snapshots are frozen from disk.
    let mgr2 = make_manager(&dir);
    let block = mgr2
        .system_prompt_block()
        .expect("block should be Some after entries were written");

    assert!(
        block.contains("## Agent Memory"),
        "block must contain '## Agent Memory' header, got:\n{block}"
    );
    assert!(
        block.contains("User prefers Rust over Python"),
        "block must contain the memory entry, got:\n{block}"
    );
    assert!(
        block.contains("## User Profile"),
        "block must contain '## User Profile' header, got:\n{block}"
    );
    assert!(
        block.contains("Name: Alice"),
        "block must contain the user entry, got:\n{block}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 3. handle_tool_call — add
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_handle_tool_call_add() {
    let dir = TempDir::new().unwrap();
    let mut mgr = make_manager(&dir);

    let r = mgr
        .handle_tool_call("add", "memory", Some("Entry A"), None)
        .unwrap();

    assert!(r["success"].as_bool().unwrap(), "add should succeed");
    assert_eq!(r["entry_count"].as_u64().unwrap(), 1);
    assert!(r["message"].as_str().unwrap().contains("Added"));
    assert!(r["usage"].as_str().is_some());
}

// ──────────────────────────────────────────────────────────────────────────────
// 4. handle_tool_call — replace
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_handle_tool_call_replace() {
    let dir = TempDir::new().unwrap();
    let mut mgr = make_manager(&dir);

    mgr.handle_tool_call("add", "memory", Some("Original entry text"), None)
        .unwrap();

    let r = mgr
        .handle_tool_call(
            "replace",
            "memory",
            Some("Updated entry text"),
            Some("Original entry"),
        )
        .unwrap();

    assert!(r["success"].as_bool().unwrap(), "replace should succeed");
    assert_eq!(r["entry_count"].as_u64().unwrap(), 1, "count stays 1");
    assert!(r["message"].as_str().unwrap().contains("Replaced"));
}

// ──────────────────────────────────────────────────────────────────────────────
// 5. handle_tool_call — remove
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_handle_tool_call_remove() {
    let dir = TempDir::new().unwrap();
    let mut mgr = make_manager(&dir);

    mgr.handle_tool_call("add", "memory", Some("Entry to delete"), None)
        .unwrap();

    let r = mgr
        .handle_tool_call("remove", "memory", None, Some("Entry to delete"))
        .unwrap();

    assert!(r["success"].as_bool().unwrap(), "remove should succeed");
    assert_eq!(r["entry_count"].as_u64().unwrap(), 0, "count drops to 0");
    assert!(r["message"].as_str().unwrap().contains("Removed"));
}

// ──────────────────────────────────────────────────────────────────────────────
// 6. handle_tool_call — unknown action returns Err
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_handle_tool_call_unknown_action() {
    let dir = TempDir::new().unwrap();
    let mut mgr = make_manager(&dir);

    let err = mgr
        .handle_tool_call("delete_all", "memory", None, None)
        .unwrap_err();

    assert!(
        err.to_string().contains("unknown action"),
        "unexpected error: {err}"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 7. handle_tool_call — only-memory block (no user entries)
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_system_prompt_block_memory_only() {
    let dir = TempDir::new().unwrap();

    {
        let mut mgr = MemoryManager::new(dir.path(), None, None);
        mgr.initialize().unwrap();
        mgr.handle_tool_call("add", "memory", Some("Only memory entry"), None)
            .unwrap();
    }

    let mgr2 = make_manager(&dir);
    let block = mgr2.system_prompt_block().unwrap();

    assert!(block.contains("## Agent Memory"));
    assert!(
        !block.contains("## User Profile"),
        "no user entries → no user header"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// 8. handle_tool_call — only-user block (no memory entries)
// ──────────────────────────────────────────────────────────────────────────────
#[test]
fn test_manager_system_prompt_block_user_only() {
    let dir = TempDir::new().unwrap();

    {
        let mut mgr = MemoryManager::new(dir.path(), None, None);
        mgr.initialize().unwrap();
        mgr.handle_tool_call("add", "user", Some("Only user entry"), None)
            .unwrap();
    }

    let mgr2 = make_manager(&dir);
    let block = mgr2.system_prompt_block().unwrap();

    assert!(block.contains("## User Profile"));
    assert!(
        !block.contains("## Agent Memory"),
        "no memory entries → no memory header"
    );
}
