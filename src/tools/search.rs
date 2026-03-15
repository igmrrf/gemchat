use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;

use super::{extract_field, SafetyTier, Tool};

pub struct SearchGoogle;

#[async_trait]
impl Tool for SearchGoogle {
    fn name(&self) -> &str { "search_google" }
    fn description(&self) -> &str { "Performs a web search using DuckDuckGo" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "query": { "type": "STRING", "description": "The search query" }
            },
            "required": ["query"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Safe }

    async fn execute(&self, args: Value, _working_dir: &Path) -> String {
        let Some(query) = extract_field(&args, "query") else {
            return "Error: 'query' is required".into();
        };

        let url = match reqwest::Url::parse_with_params(
            "https://html.duckduckgo.com/html/",
            &[("q", &query)],
        ) {
            Ok(u) => u,
            Err(e) => return format!("URL builder error: {}", e),
        };

        match reqwest::get(url).await {
            Ok(res) => match res.text().await {
                Ok(text) => format!(
                    "Search for '{}' returned {} bytes of HTML results.",
                    query,
                    text.len()
                ),
                Err(_) => "Failed to read response text".into(),
            },
            Err(e) => format!("Search request failed: {}", e),
        }
    }
}
