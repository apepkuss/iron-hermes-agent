use crate::error::SandboxError;
use iron_tools::registry::ToolRegistry;
use iron_tools::types::ToolContext;
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

pub struct RpcServer {
    registry: Arc<ToolRegistry>,
    allowed_tools: HashSet<String>,
    max_calls: u32,
    call_count: Arc<AtomicU32>,
}

impl RpcServer {
    pub fn new(
        registry: Arc<ToolRegistry>,
        allowed_tools: HashSet<String>,
        max_calls: u32,
    ) -> Self {
        Self {
            registry,
            allowed_tools,
            max_calls,
            call_count: Arc::new(AtomicU32::new(0)),
        }
    }

    pub fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }

    /// Bind a UnixListener, accept connections in a loop, serve line-delimited JSON requests.
    ///
    /// Each connection handles one request-response pair (the Python/Shell bridge
    /// creates a new connection per tool call). The loop continues until the
    /// listener is dropped (when the sandbox subprocess exits and the temp dir
    /// is cleaned up).
    pub async fn serve(self, socket_path: &Path) -> Result<Arc<AtomicU32>, SandboxError> {
        let listener = UnixListener::bind(socket_path)?;
        let call_count = Arc::clone(&self.call_count);
        let server = Arc::new(self);

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let server = Arc::clone(&server);
                tokio::spawn(async move {
                    let (read_half, mut write_half) = stream.into_split();
                    let mut lines = BufReader::new(read_half).lines();

                    while let Ok(Some(line)) = lines.next_line().await {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }

                        let response = server.handle_request(&line).await;
                        let mut resp_str = serde_json::to_string(&response)
                            .unwrap_or_else(|_| r#"{"error":"serialization error"}"#.to_string());
                        resp_str.push('\n');

                        if write_half.write_all(resp_str.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });

        Ok(call_count)
    }

    async fn handle_request(&self, line: &str) -> Value {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return serde_json::json!({"error": format!("invalid JSON: {}", e)});
            }
        };

        let tool = match req.get("tool").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => return serde_json::json!({"error": "missing 'tool' field"}),
        };

        if !self.allowed_tools.contains(&tool) {
            return serde_json::json!({"error": format!("tool '{}' not allowed", tool)});
        }

        let count = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;
        if count > self.max_calls {
            self.call_count.fetch_sub(1, Ordering::SeqCst);
            return serde_json::json!({"error": "tool call limit exceeded"});
        }

        let args = req
            .get("args")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));

        let ctx = ToolContext {
            task_id: "sandbox".to_string(),
            working_dir: std::env::temp_dir(),
            enabled_tools: self.allowed_tools.clone(),
            env_vars: iron_tool_api::env::collect_safe_env(),
        };

        match self.registry.dispatch_sync(&tool, args, &ctx) {
            Ok(result) => serde_json::json!({
                "success": result.success,
                "output": result.output,
            }),
            Err(e) => serde_json::json!({"error": e.to_string()}),
        }
    }
}
