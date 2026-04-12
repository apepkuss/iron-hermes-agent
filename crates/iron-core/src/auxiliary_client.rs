use reqwest::Client;
use std::time::Duration;

use crate::error::CoreError;
use crate::llm::types::{ChatRequest, ChatResponse, Message};

/// A simplified LLM client for auxiliary tasks such as context summarisation.
///
/// Uses a fixed 120-second timeout and only supports non-streaming chat completions.
pub struct AuxiliaryClient {
    base_url: String,
    model: String,
    http: Client,
}

impl AuxiliaryClient {
    /// Create a new auxiliary client.
    pub fn new(base_url: String, model: String) -> Self {
        let http = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            base_url,
            model,
            http,
        }
    }

    /// Build the chat completions endpoint URL.
    ///
    /// Handles both `/v1`-suffixed and plain base URLs.
    fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    /// Generate a structured summary of a conversation excerpt.
    ///
    /// If `previous_summary` is provided, the model is asked to update it
    /// rather than generate a fresh summary.
    pub async fn generate_summary(
        &self,
        conversation_text: &str,
        max_tokens: u32,
        previous_summary: Option<&str>,
    ) -> Result<String, CoreError> {
        let system_content = if let Some(prev) = previous_summary {
            format!(
                "You are a technical assistant that maintains a running context summary for an AI \
                 coding agent. Update the existing summary below by incorporating the new \
                 conversation turns provided by the user. Keep the same structured format:\n\
                 - **Goal**: What the user is trying to achieve\n\
                 - **Progress Done**: Completed steps\n\
                 - **In Progress**: Current work\n\
                 - **Key Decisions**: Important choices made\n\
                 - **Relevant Files**: Files created or modified\n\
                 - **Critical Context**: Important facts to remember\n\n\
                 Be concise. Do not repeat information already in the summary unless it changed.\n\n\
                 ## Existing Summary\n\n{prev}"
            )
        } else {
            "You are a technical assistant that creates a structured context summary for an AI \
             coding agent. Summarise the conversation turns provided by the user. Use this \
             format:\n\
             - **Goal**: What the user is trying to achieve\n\
             - **Progress Done**: Completed steps\n\
             - **In Progress**: Current work\n\
             - **Key Decisions**: Important choices made\n\
             - **Relevant Files**: Files created or modified\n\
             - **Critical Context**: Important facts to remember\n\n\
             Be concise and accurate. Only include information actually present in the conversation."
                .to_string()
        };

        let messages = vec![
            Message {
                role: "system".to_string(),
                content: Some(system_content),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
            Message {
                role: "user".to_string(),
                content: Some(conversation_text.to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            },
        ];

        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            tools: None,
            stream: Some(false),
            stream_options: None,
            temperature: Some(0.3),
            max_tokens: Some(max_tokens),
        };

        let response = self
            .http
            .post(self.endpoint())
            .json(&request)
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

        let content = chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .unwrap_or_default();

        Ok(content)
    }
}
