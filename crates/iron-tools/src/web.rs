use crate::error::ToolError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

const TAVILY_BASE_URL: &str = "https://api.tavily.com";
const REQUEST_TIMEOUT_SECS: u64 = 60;
const MAX_EXTRACT_URLS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TavilySearchResponse {
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractResultItem {
    pub url: String,
    pub raw_content: Option<String>,
    pub content: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TavilyExtractResponse {
    pub results: Vec<ExtractResultItem>,
}

pub struct TavilyClient {
    api_key: String,
    http: reqwest::Client,
}

impl TavilyClient {
    pub fn new(api_key: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .build()
            .expect("Failed to build reqwest client");
        Self { api_key, http }
    }

    pub fn from_env() -> Result<Self, ToolError> {
        let api_key = std::env::var("TAVILY_API_KEY").map_err(|_| {
            ToolError::ExecutionFailed("TAVILY_API_KEY environment variable not set".to_string())
        })?;
        Ok(Self::new(api_key))
    }

    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>, ToolError> {
        let url = format!("{}/search", TAVILY_BASE_URL);
        let body = json!({
            "api_key": self.api_key,
            "query": query,
        });

        let response = self.http.post(&url).json(&body).send().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Tavily search request failed: {e}"))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionFailed(format!(
                "Tavily search returned HTTP {status}: {text}"
            )));
        }

        let parsed: TavilySearchResponse = response.json().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to parse Tavily search response: {e}"))
        })?;

        Ok(parsed.results)
    }

    pub async fn extract(&self, urls: &[String]) -> Result<Vec<ExtractResultItem>, ToolError> {
        if urls.len() > MAX_EXTRACT_URLS {
            return Err(ToolError::InvalidArgs {
                tool: "tavily_extract".to_string(),
                reason: format!(
                    "Too many URLs: got {}, maximum is {MAX_EXTRACT_URLS}",
                    urls.len()
                ),
            });
        }

        let url = format!("{}/extract", TAVILY_BASE_URL);
        let body = json!({
            "api_key": self.api_key,
            "urls": urls,
        });

        let response = self.http.post(&url).json(&body).send().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Tavily extract request failed: {e}"))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(ToolError::ExecutionFailed(format!(
                "Tavily extract returned HTTP {status}: {text}"
            )));
        }

        let parsed: TavilyExtractResponse = response.json().await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to parse Tavily extract response: {e}"))
        })?;

        Ok(parsed.results)
    }
}

pub fn format_search_results(results: &[SearchResult]) -> Value {
    let web: Vec<Value> = results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            json!({
                "title": r.title,
                "url": r.url,
                "description": r.content,
                "position": i + 1,
            })
        })
        .collect();

    json!({
        "success": true,
        "data": {
            "web": web,
        }
    })
}
