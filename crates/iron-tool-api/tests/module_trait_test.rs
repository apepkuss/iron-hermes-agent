use iron_tool_api::{ToolModule, ToolRegistry, ToolResult, ToolSchema};
use serde_json::{Value, json};

struct DummyModule {
    tool_name: String,
}

impl ToolModule for DummyModule {
    fn register(self: Box<Self>, registry: &mut ToolRegistry) {
        let name = self.tool_name.clone();
        registry.register_sync(
            &name,
            "test",
            ToolSchema {
                name: name.clone(),
                description: "A dummy tool".to_string(),
                parameters: json!({"type": "object", "properties": {}}),
            },
            |_args: Value, _ctx| Ok(ToolResult::ok(json!({"dummy": true}))),
        );
    }
}

#[test]
fn test_tool_module_registers_tool() {
    let mut registry = ToolRegistry::new();
    let module: Box<dyn ToolModule> = Box::new(DummyModule {
        tool_name: "dummy".to_string(),
    });
    module.register(&mut registry);
    assert!(registry.has_tool("dummy"));
}

#[test]
fn test_multiple_modules_register() {
    let mut registry = ToolRegistry::new();
    let modules: Vec<Box<dyn ToolModule>> = vec![
        Box::new(DummyModule {
            tool_name: "tool_a".to_string(),
        }),
        Box::new(DummyModule {
            tool_name: "tool_b".to_string(),
        }),
    ];
    for module in modules {
        module.register(&mut registry);
    }
    assert!(registry.has_tool("tool_a"));
    assert!(registry.has_tool("tool_b"));
    assert_eq!(registry.tool_names().len(), 2);
}
