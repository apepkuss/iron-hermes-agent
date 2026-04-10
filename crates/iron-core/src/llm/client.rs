use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::error::CoreError;
use crate::llm::types::{ChatRequest, ChatResponse, ChatStreamChunk, Message};

/// Configuration for the LLM client.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
}

/// OpenAI-compatible LLM client with streaming and non-streaming support.
pub struct LlmClient {
    config: LlmConfig,
    http: Client,
}

impl LlmClient {
    /// Create a new LLM client with the given configuration.
    pub fn new(config: LlmConfig) -> Self {
        let http = Client::builder()
            .no_proxy()
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { config, http }
    }

    /// Build the endpoint URL for chat completions.
    /// If base_url already ends with `/v1`, appends `/chat/completions`.
    /// Otherwise appends `/v1/chat/completions`.
    fn endpoint(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    /// Build an HTTP request with common headers.
    fn build_request(&self, body: &ChatRequest) -> Result<reqwest::RequestBuilder, CoreError> {
        let mut req = self.http.post(self.endpoint()).json(body);
        if let Some(ref key) = self.config.api_key {
            req = req.header("Authorization", format!("Bearer {key}"));
        }
        Ok(req)
    }

    /// Non-streaming chat completion.
    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<ChatResponse, CoreError> {
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages,
            tools,
            stream: Some(false),
            stream_options: None,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        debug!("Sending non-streaming chat request to {}", self.endpoint());

        let response = self
            .build_request(&request)?
            .send()
            .await
            .map_err(|e| CoreError::LlmRequest(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".to_string());
            return Err(CoreError::LlmApi {
                status: status.as_u16(),
                message: body,
            });
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| CoreError::LlmRequest(format!("Failed to parse response: {e}")))?;

        Ok(chat_response)
    }

    /// Streaming chat completion. Returns an `mpsc::Receiver` that yields `ChatStreamChunk`s.
    pub async fn chat_stream(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<mpsc::Receiver<Result<ChatStreamChunk, CoreError>>, CoreError> {
        let request = ChatRequest {
            model: self.config.model.clone(),
            messages,
            tools,
            stream: Some(true),
            stream_options: Some(json!({"include_usage": true})),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        debug!("Sending streaming chat request to {}", self.endpoint());

        let response = self
            .build_request(&request)?
            .send()
            .await
            .map_err(|e| CoreError::LlmRequest(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "unable to read response body".to_string());
            return Err(CoreError::LlmApi {
                status: status.as_u16(),
                message: body,
            });
        }

        let (tx, rx) = mpsc::channel::<Result<ChatStreamChunk, CoreError>>(64);

        tokio::spawn(async move {
            if let Err(e) = process_sse_stream(response, &tx).await {
                let _ = tx.send(Err(e)).await;
            }
        });

        Ok(rx)
    }
}

/// Process an SSE response stream, parsing chunks and sending them through the channel.
async fn process_sse_stream(
    response: reqwest::Response,
    tx: &mpsc::Sender<Result<ChatStreamChunk, CoreError>>,
) -> Result<(), CoreError> {
    use bytes::BytesMut;
    use futures_util::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buffer = BytesMut::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| CoreError::LlmRequest(e.to_string()))?;
        buffer.extend_from_slice(&chunk);

        // Process complete lines from the buffer
        while let Some(newline_pos) = buffer.iter().position(|&b| b == b'\n') {
            let line_bytes = buffer.split_to(newline_pos + 1);
            let line = String::from_utf8_lossy(&line_bytes).trim().to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(());
                }

                match serde_json::from_str::<ChatStreamChunk>(data) {
                    Ok(parsed) => {
                        if tx.send(Ok(parsed)).await.is_err() {
                            // Receiver dropped
                            return Ok(());
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse SSE chunk: {e}, data: {data}");
                        if tx
                            .send(Err(CoreError::LlmRequest(format!(
                                "Failed to parse SSE chunk: {e}"
                            ))))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
