use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use super::{AiProvider, AiUpdate, ChatMessage, ToolDefinition, Usage};

/// OpenAI Chat Completions API provider (GPT-4o, Codex, etc.).
pub struct OpenAiProvider {
    model: String,
    api_key: String,
    client: Client,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(model: String, api_key: String) -> Self {
        Self {
            model,
            api_key,
            client: Client::new(),
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    /// Create with a custom base URL (for Azure OpenAI, local proxies, etc.).
    pub fn with_base_url(model: String, api_key: String, base_url: String) -> Self {
        Self {
            model,
            api_key,
            client: Client::new(),
            base_url,
        }
    }

    /// Build the OpenAI Chat Completions request body.
    fn build_body(
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> serde_json::Value {
        let oai_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "user" => "user",
                    "model" | "assistant" => "assistant",
                    "system" => "system",
                    "tool" => "tool",
                    _ => "user",
                };
                json!({
                    "role": role,
                    "content": msg.content
                })
            })
            .collect();

        let mut body = json!({
            "model": model,
            "messages": oai_messages,
            "stream": true,
            "stream_options": { "include_usage": true }
        });

        if !tools.is_empty() {
            let oai_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters
                        }
                    })
                })
                .collect();
            body["tools"] = json!(oai_tools);
        }

        body
    }

    /// Parse an SSE data line from the OpenAI streaming API.
    fn parse_sse_line(line: &str, tx: &UnboundedSender<AiUpdate>) {
        if !line.starts_with("data: ") {
            return;
        }

        let json_str = &line[6..];

        // [DONE] signals end of stream
        if json_str.trim() == "[DONE]" {
            let _ = tx.send(AiUpdate::Finished);
            return;
        }

        let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) else {
            return;
        };

        // Extract from choices[0].delta
        if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
            if let Some(first) = choices.first() {
                if let Some(delta) = first.get("delta") {
                    // Content text
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        let _ = tx.send(AiUpdate::Content(content.to_string()));
                    }

                    // Tool calls
                    if let Some(tool_calls) =
                        delta.get("tool_calls").and_then(|tc| tc.as_array())
                    {
                        for tc in tool_calls {
                            if let Some(function) = tc.get("function") {
                                let name = function
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let args = function
                                    .get("arguments")
                                    .and_then(|a| a.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                if !name.is_empty() {
                                    let _ = tx.send(AiUpdate::ToolCall { name, args });
                                }
                            }
                        }
                    }
                }
            }
        }

        // Extract usage (only in the final chunk with stream_options)
        if let Some(usage) = json.get("usage") {
            let prompt_tokens =
                usage.get("prompt_tokens").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
            let completion_tokens = usage
                .get("completion_tokens")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32;
            let total_tokens =
                usage.get("total_tokens").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
            let _ = tx.send(AiUpdate::Usage(Usage {
                prompt_tokens,
                response_tokens: completion_tokens,
                total_tokens,
            }));
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    async fn stream_response(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tx: UnboundedSender<AiUpdate>,
    ) {
        if self.api_key.is_empty() {
            // Mock mode
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = tx.send(AiUpdate::Content(
                "(Mock OpenAI) Set OPENAI_API_KEY for real responses.\n".into(),
            ));
            let _ = tx.send(AiUpdate::Usage(Usage {
                prompt_tokens: 10,
                response_tokens: 20,
                total_tokens: 30,
            }));
            let _ = tx.send(AiUpdate::Finished);
            return;
        }

        let url = format!("{}/chat/completions", self.base_url);
        let body = Self::build_body(&self.model, messages, tools);

        let resp = match self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(AiUpdate::Error(format!("Request failed: {}", e)));
                let _ = tx.send(AiUpdate::Finished);
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| "unknown".into());
            let _ = tx.send(AiUpdate::Error(format!("API Error {}: {}", status, text)));
            let _ = tx.send(AiUpdate::Finished);
            return;
        }

        // Parse SSE stream
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(AiUpdate::Error(format!("Stream error: {}", e)));
                    break;
                }
            };

            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let mut line = buffer[..pos].to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.ends_with('\r') {
                    line.pop();
                }

                Self::parse_sse_line(&line, &tx);
            }
        }

        let _ = tx.send(AiUpdate::Finished);
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "openai"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_metadata() {
        let provider = OpenAiProvider::new("gpt-4o".into(), String::new());
        assert_eq!(provider.provider_name(), "openai");
        assert_eq!(provider.model_name(), "gpt-4o");
    }

    #[test]
    fn test_custom_base_url() {
        let provider = OpenAiProvider::with_base_url(
            "gpt-4o".into(),
            "key".into(),
            "https://custom.openai.azure.com".into(),
        );
        assert_eq!(provider.base_url, "https://custom.openai.azure.com");
    }

    #[test]
    fn test_build_body_without_tools() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let body = OpenAiProvider::build_body("gpt-4o", &messages, &[]);
        assert_eq!(body["model"], "gpt-4o");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_build_body_with_tools() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let tools = vec![ToolDefinition {
            name: "search".into(),
            description: "Search the web".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let body = OpenAiProvider::build_body("gpt-4o", &messages, &tools);
        assert!(body.get("tools").is_some());
        assert_eq!(body["tools"][0]["type"], "function");
    }
}
