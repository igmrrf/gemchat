use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::sync::mpsc::Sender;

use super::{AiProvider, AiUpdate, ChatMessage, ToolDefinition, Usage};

/// Anthropic Claude API provider (Messages API with streaming).
pub struct ClaudeProvider {
    model: String,
    api_key: String,
    client: Client,
}

impl ClaudeProvider {
    pub fn new(model: String, api_key: String) -> Self {
        Self {
            model,
            api_key,
            client: Client::new(),
        }
    }

    /// Build the Claude Messages API request body.
    fn build_body(messages: &[ChatMessage], tools: &[ToolDefinition]) -> serde_json::Value {
        // Convert messages to Claude format
        let claude_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "user" => "user",
                    "model" | "assistant" => "assistant",
                    _ => "user",
                };
                json!({
                    "role": role,
                    "content": msg.content
                })
            })
            .collect();

        let mut body = json!({
            "model": "",  // Will be set by caller
            "max_tokens": 8192,
            "messages": claude_messages,
            "stream": true
        });

        // Add tools if provided
        if !tools.is_empty() {
            let claude_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters
                    })
                })
                .collect();
            body["tools"] = json!(claude_tools);
        }

        body
    }

    /// Parse an SSE event from the Claude streaming API.
    async fn parse_sse_event(event_type: &str, data: &str, tx: &Sender<AiUpdate>) {
        match event_type {
            "content_block_delta" => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                    && let Some(delta) = json.get("delta") {
                        // Text delta
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            let _ = tx.send(AiUpdate::Content(text.to_string())).await;
                        }
                        // Tool use input delta (partial JSON)
                        if let Some(partial) =
                            delta.get("partial_json").and_then(|p| p.as_str())
                        {
                            // Accumulate — will be assembled by the pipeline
                            let _ = tx.send(AiUpdate::Content(partial.to_string())).await;
                        }
                    }
            }
            "content_block_start" => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                    && let Some(content_block) = json.get("content_block")
                        && content_block.get("type").and_then(|t| t.as_str())
                            == Some("tool_use")
                        {
                            // Tool call starting — extract name
                            if let Some(name) =
                                content_block.get("name").and_then(|n| n.as_str())
                            {
                                // We'll get the args via content_block_delta
                                let _ = tx.send(AiUpdate::ToolCall {
                                    name: name.to_string(),
                                    args: String::new(), // Filled via delta accumulation
                                }).await;
                            }
                        }
            }
            "message_delta" => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    // Extract usage from message_delta
                    if let Some(usage) = json.get("usage") {
                        let output_tokens =
                            usage.get("output_tokens").and_then(|t| t.as_i64()).unwrap_or(0)
                                as i32;
                        let _ = tx.send(AiUpdate::Usage(Usage {
                            prompt_tokens: 0,
                            response_tokens: output_tokens,
                            total_tokens: output_tokens,
                        })).await;
                    }
                }
            }
            "message_start" => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
                    && let Some(message) = json.get("message")
                        && let Some(usage) = message.get("usage") {
                            let input_tokens = usage
                                .get("input_tokens")
                                .and_then(|t| t.as_i64())
                                .unwrap_or(0) as i32;
                            let _ = tx.send(AiUpdate::Usage(Usage {
                                prompt_tokens: input_tokens,
                                response_tokens: 0,
                                total_tokens: input_tokens,
                            })).await;
                        }
            }
            "message_stop" => {
                let _ = tx.send(AiUpdate::Finished).await;
            }
            _ => {}
        }
    }
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    async fn stream_response(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tx: Sender<AiUpdate>,
    ) {
        if self.api_key.is_empty() {
            // Mock mode
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = tx.send(AiUpdate::Content(
                "(Mock Claude) Set ANTHROPIC_API_KEY for real responses.\n".into(),
            )).await;
            let _ = tx.send(AiUpdate::Usage(Usage {
                prompt_tokens: 10,
                response_tokens: 20,
                total_tokens: 30,
            })).await;
            let _ = tx.send(AiUpdate::Finished).await;
            return;
        }

        let mut body = Self::build_body(messages, tools);
        body["model"] = json!(self.model);

        let resp = match self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(AiUpdate::Error(format!("Request failed: {}", e))).await;
                let _ = tx.send(AiUpdate::Finished).await;
                return;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|_| "unknown".into());
            let _ = tx.send(AiUpdate::Error(format!("API Error {}: {}", status, text))).await;
            let _ = tx.send(AiUpdate::Finished).await;
            return;
        }

        // Parse SSE stream
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let mut current_event = String::new();

        while let Some(item) = stream.next().await {
            let chunk = match item {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(AiUpdate::Error(format!("Stream error: {}", e))).await;
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

                if line.starts_with("event: ") {
                    current_event = line[7..].to_string();
                } else if let Some(data) = line.strip_prefix("data: ") {
                    Self::parse_sse_event(&current_event, data, &tx).await;
                }
                // Empty lines separate events — reset
                if line.is_empty() {
                    current_event.clear();
                }
            }
        }

        let _ = tx.send(AiUpdate::Finished).await;
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "claude"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_provider_metadata() {
        let provider = ClaudeProvider::new("claude-sonnet-4-20250514".into(), String::new());
        assert_eq!(provider.provider_name(), "claude");
        assert_eq!(provider.model_name(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_build_body_without_tools() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let body = ClaudeProvider::build_body(&messages, &[]);
        assert!(body.get("messages").is_some());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_build_body_with_tools() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "Hello".into(),
        }];
        let tools = vec![ToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: serde_json::json!({"type": "object"}),
        }];
        let body = ClaudeProvider::build_body(&messages, &tools);
        assert!(body.get("tools").is_some());
    }
}
