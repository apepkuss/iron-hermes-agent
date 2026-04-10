use iron_tools::web::{
    ExtractResultItem, SearchResult, TavilyExtractResponse, TavilySearchResponse,
    format_search_results,
};
use serde_json::json;

#[test]
fn test_search_result_deserialize() {
    let json = json!({
        "title": "Rust Programming Language",
        "url": "https://www.rust-lang.org",
        "content": "A language empowering everyone to build reliable and efficient software.",
        "score": 0.95
    });

    let result: SearchResult = serde_json::from_value(json).unwrap();
    assert_eq!(result.title, "Rust Programming Language");
    assert_eq!(result.url, "https://www.rust-lang.org");
    assert_eq!(
        result.content,
        "A language empowering everyone to build reliable and efficient software."
    );
    assert_eq!(result.score, Some(0.95));
}

#[test]
fn test_search_result_deserialize_no_score() {
    let json = json!({
        "title": "Some Page",
        "url": "https://example.com",
        "content": "Some content here."
    });

    let result: SearchResult = serde_json::from_value(json).unwrap();
    assert_eq!(result.title, "Some Page");
    assert_eq!(result.url, "https://example.com");
    assert_eq!(result.content, "Some content here.");
    assert_eq!(result.score, None);
}

#[test]
fn test_tavily_search_response_deserialize() {
    let json = json!({
        "results": [
            {
                "title": "Result 1",
                "url": "https://example.com/1",
                "content": "Content 1",
                "score": 0.9
            },
            {
                "title": "Result 2",
                "url": "https://example.com/2",
                "content": "Content 2",
                "score": 0.8
            }
        ]
    });

    let response: TavilySearchResponse = serde_json::from_value(json).unwrap();
    assert_eq!(response.results.len(), 2);
    assert_eq!(response.results[0].title, "Result 1");
    assert_eq!(response.results[1].url, "https://example.com/2");
}

#[test]
fn test_extract_result_deserialize() {
    let json = json!({
        "url": "https://example.com",
        "raw_content": "<html><body>Raw HTML content</body></html>",
        "content": "Extracted text content",
        "error": null
    });

    let result: ExtractResultItem = serde_json::from_value(json).unwrap();
    assert_eq!(result.url, "https://example.com");
    assert_eq!(
        result.raw_content,
        Some("<html><body>Raw HTML content</body></html>".to_string())
    );
    assert_eq!(result.content, Some("Extracted text content".to_string()));
    assert_eq!(result.error, None);
}

#[test]
fn test_extract_result_deserialize_with_error() {
    let json = json!({
        "url": "https://example.com/missing",
        "raw_content": null,
        "content": null,
        "error": "Page not found"
    });

    let result: ExtractResultItem = serde_json::from_value(json).unwrap();
    assert_eq!(result.url, "https://example.com/missing");
    assert_eq!(result.raw_content, None);
    assert_eq!(result.content, None);
    assert_eq!(result.error, Some("Page not found".to_string()));
}

#[test]
fn test_tavily_extract_response_deserialize() {
    let json = json!({
        "results": [
            {
                "url": "https://example.com/1",
                "raw_content": "Raw 1",
                "content": "Content 1",
                "error": null
            }
        ]
    });

    let response: TavilyExtractResponse = serde_json::from_value(json).unwrap();
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].url, "https://example.com/1");
}

#[test]
fn test_search_result_to_tool_result() {
    let results = vec![
        SearchResult {
            title: "First Result".to_string(),
            url: "https://example.com/1".to_string(),
            content: "Description of first result".to_string(),
            score: Some(0.95),
        },
        SearchResult {
            title: "Second Result".to_string(),
            url: "https://example.com/2".to_string(),
            content: "Description of second result".to_string(),
            score: Some(0.80),
        },
    ];

    let output = format_search_results(&results);

    assert_eq!(output["success"], true);
    let web = &output["data"]["web"];
    assert!(web.is_array());
    let items = web.as_array().unwrap();
    assert_eq!(items.len(), 2);

    assert_eq!(items[0]["title"], "First Result");
    assert_eq!(items[0]["url"], "https://example.com/1");
    assert_eq!(items[0]["description"], "Description of first result");
    assert_eq!(items[0]["position"], 1);

    assert_eq!(items[1]["title"], "Second Result");
    assert_eq!(items[1]["url"], "https://example.com/2");
    assert_eq!(items[1]["description"], "Description of second result");
    assert_eq!(items[1]["position"], 2);
}

#[test]
fn test_format_search_results_empty() {
    let results: Vec<SearchResult> = vec![];
    let output = format_search_results(&results);

    assert_eq!(output["success"], true);
    let web = &output["data"]["web"];
    assert!(web.is_array());
    assert_eq!(web.as_array().unwrap().len(), 0);
}
