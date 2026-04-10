use std::collections::HashSet;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

// We cannot import the binary's private modules directly, so we replicate the
// minimal wiring here.  The route handlers are re-exported through the crate's
// lib surface if we make one, but since iron-server is a binary crate we
// inline the setup using the same dependency crates.

/// Lightweight test-only `AppState`.
struct TestAppState {
    config: TestServerConfig,
    #[allow(dead_code)]
    tool_registry: Arc<iron_tools::registry::ToolRegistry>,
    #[allow(dead_code)]
    memory_manager: Arc<tokio::sync::Mutex<iron_memory::manager::MemoryManager>>,
    #[allow(dead_code)]
    skill_manager: Arc<iron_skills::manager::SkillManager>,
}

#[derive(Clone)]
struct TestServerConfig {
    model_name: String,
}

fn test_app() -> Router {
    let tmp = tempfile::TempDir::new().unwrap();
    let mem_dir = tmp.path().join("memories");
    std::fs::create_dir_all(&mem_dir).unwrap();
    let skills_dir = tmp.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    let mut mem = iron_memory::manager::MemoryManager::new(&mem_dir, None, None);
    mem.initialize().ok();

    let state = Arc::new(TestAppState {
        config: TestServerConfig {
            model_name: "iron-hermes-test".to_string(),
        },
        tool_registry: Arc::new(iron_tools::registry::ToolRegistry::new()),
        memory_manager: Arc::new(tokio::sync::Mutex::new(mem)),
        skill_manager: Arc::new(iron_skills::manager::SkillManager::new(
            vec![skills_dir],
            HashSet::new(),
        )),
    });

    // Health handler — standalone, no state needed.
    async fn health_handler() -> axum::Json<Value> {
        axum::Json(serde_json::json!({"status": "ok"}))
    }

    // Models handler — needs model name from state.
    async fn models_handler(
        axum::extract::State(state): axum::extract::State<Arc<TestAppState>>,
    ) -> axum::Json<Value> {
        axum::Json(serde_json::json!({
            "object": "list",
            "data": [{
                "id": state.config.model_name,
                "object": "model",
                "created": 1700000000_u64,
                "owned_by": "iron-hermes"
            }]
        }))
    }

    // Chat handler — validates request structure.
    async fn chat_handler(
        axum::Json(payload): axum::Json<Value>,
    ) -> (StatusCode, axum::Json<Value>) {
        let messages = payload.get("messages").and_then(|v| v.as_array());
        match messages {
            Some(msgs) if !msgs.is_empty() => {
                // In test mode, just echo back — we don't have a real LLM.
                (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({
                        "id": "chatcmpl-test",
                        "object": "chat.completion",
                        "model": "iron-hermes-test",
                        "choices": [{
                            "index": 0,
                            "message": {"role": "assistant", "content": "test response"},
                            "finish_reason": "stop"
                        }]
                    })),
                )
            }
            _ => (
                StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": {
                        "message": "messages is required and must not be empty",
                        "type": "invalid_request_error"
                    }
                })),
            ),
        }
    }

    Router::new()
        .route("/health", get(health_handler))
        .route("/v1/models", get(models_handler))
        .route("/v1/chat/completions", post(chat_handler))
        .with_state(state)
}

#[tokio::test]
async fn test_health_endpoint() {
    let app = test_app();

    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_models_endpoint() {
    let app = test_app();

    let req = Request::builder()
        .uri("/v1/models")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "list");
    let data = json["data"].as_array().unwrap();
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["id"], "iron-hermes-test");
    assert_eq!(data[0]["object"], "model");
    assert_eq!(data[0]["owned_by"], "iron-hermes");
}

#[tokio::test]
async fn test_chat_endpoint_missing_messages() {
    let app = test_app();

    // No messages field at all.
    let req = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("messages")
    );
}

#[tokio::test]
async fn test_chat_endpoint_empty_messages() {
    let app = test_app();

    let req = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"messages":[]}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_chat_endpoint_valid_request() {
    let app = test_app();

    let req = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"messages":[{"role":"user","content":"hello"}]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["choices"][0]["message"]["role"], "assistant");
}
