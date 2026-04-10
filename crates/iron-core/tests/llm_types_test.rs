use iron_core::llm::types::*;
use serde_json::json;

#[test]
fn test_message_serialize() {
    let msg = Message {
        role: "user".to_string(),
        content: Some("Hello, world!".to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
    };

    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["content"], "Hello, world!");
    // Option::None fields should be skipped
    assert!(json.get("tool_calls").is_none());
    assert!(json.get("tool_call_id").is_none());
    assert!(json.get("name").is_none());
}

#[test]
fn test_message_with_tool_calls() {
    let msg = Message {
        role: "assistant".to_string(),
        content: None,
        tool_calls: Some(vec![ToolCall {
            id: "call_123".to_string(),
            r#type: "function".to_string(),
            function: FunctionCall {
                name: "get_weather".to_string(),
                arguments: r#"{"city":"Tokyo"}"#.to_string(),
            },
        }]),
        tool_call_id: None,
        name: None,
    };

    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "assistant");
    assert!(json.get("content").is_none());

    let tool_calls = json["tool_calls"].as_array().unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["id"], "call_123");
    assert_eq!(tool_calls[0]["type"], "function");
    assert_eq!(tool_calls[0]["function"]["name"], "get_weather");
    assert_eq!(
        tool_calls[0]["function"]["arguments"],
        r#"{"city":"Tokyo"}"#
    );
}

#[test]
fn test_chat_response_deserialize() {
    let raw = json!({
        "id": "chatcmpl-abc123",
        "object": "chat.completion",
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
        }
    });

    let resp: ChatResponse = serde_json::from_value(raw).unwrap();
    assert_eq!(resp.id, "chatcmpl-abc123");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.model, "gpt-4");
    assert_eq!(resp.choices.len(), 1);
    assert_eq!(resp.choices[0].index, 0);
    assert_eq!(resp.choices[0].message.role, "assistant");
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello! How can I help you?")
    );
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));

    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 8);
    assert_eq!(usage.total_tokens, 18);
}

#[test]
fn test_stream_chunk_deserialize() {
    let raw = json!({
        "id": "chatcmpl-abc123",
        "object": "chat.completion.chunk",
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "delta": {
                "content": "Hello"
            },
            "finish_reason": null
        }]
    });

    let chunk: ChatStreamChunk = serde_json::from_value(raw).unwrap();
    assert_eq!(chunk.id, "chatcmpl-abc123");
    assert_eq!(chunk.object, "chat.completion.chunk");
    assert_eq!(chunk.model.as_deref(), Some("gpt-4"));
    assert_eq!(chunk.choices.len(), 1);
    assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hello"));
    assert!(chunk.choices[0].finish_reason.is_none());
    assert!(chunk.usage.is_none());
}

#[test]
fn test_stream_chunk_tool_call() {
    let raw = json!({
        "id": "chatcmpl-abc123",
        "object": "chat.completion.chunk",
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_456",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"q\":"
                    }
                }]
            },
            "finish_reason": null
        }]
    });

    let chunk: ChatStreamChunk = serde_json::from_value(raw).unwrap();
    let tool_calls = chunk.choices[0].delta.tool_calls.as_ref().unwrap();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].index, 0);
    assert_eq!(tool_calls[0].id.as_deref(), Some("call_456"));
    assert_eq!(tool_calls[0].r#type.as_deref(), Some("function"));

    let func = tool_calls[0].function.as_ref().unwrap();
    assert_eq!(func.name.as_deref(), Some("search"));
    assert_eq!(func.arguments.as_deref(), Some("{\"q\":"));
}

#[test]
fn test_tool_message_serialize() {
    let msg = Message {
        role: "tool".to_string(),
        content: Some(r#"{"temperature": 22}"#.to_string()),
        tool_calls: None,
        tool_call_id: Some("call_123".to_string()),
        name: Some("get_weather".to_string()),
    };

    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "tool");
    assert_eq!(json["content"], r#"{"temperature": 22}"#);
    assert_eq!(json["tool_call_id"], "call_123");
    assert_eq!(json["name"], "get_weather");
    assert!(json.get("tool_calls").is_none());
}
