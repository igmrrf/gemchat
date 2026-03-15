use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::sync::mpsc::UnboundedSender;

use super::{AiProvider, AiUpdate, ChatMessage, ToolDefinition, Usage};

/// Gemini API provider (Google AI Studio / Vertex).
pub struct GeminiProvider {
    model: String,
    api_key: String,
    client: Client,
}

impl GeminiProvider {
    pub fn new(model: String, api_key: String) -> Self {
        Self {
            model,
            api_key,
            client: Client::new(),
        }
    }

    /// Build the Gemini API request body from messages and tools.
    fn build_body(messages: &[ChatMessage], tools: &[ToolDefinition]) -> serde_json::Value {
        // Convert messages to Gemini content format
        let contents: Vec<serde_json::Value> = messages
            .iter()
            .map(|msg| {
                json!({
                    "role": match msg.role.as_str() {
                        "user" => "user",
                        "model" | "assistant" => "model",
                        _ => "user",
                    },
                    "parts": [{ "text": msg.content }]
                })
            })
            .collect();

        // Convert tool definitions to Gemini function declarations
        let func_declarations: Vec<serde_json::Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                })
            })
            .collect();

        let mut body = json!({ "contents": contents });

        if !func_declarations.is_empty() {
            body["tools"] = json!([{
                "functionDeclarations": func_declarations
            }]);
        }

        body
    }

    /// Parse an SSE data line from the Gemini streaming API.
    fn parse_sse_line(line: &str, tx: &UnboundedSender<AiUpdate>) {
        if !line.starts_with("data: ") {
            return;
        }

        let json_str = &line[6..];
        let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) else {
            return;
        };

        // Extract content and tool calls from candidates
        if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
            if let Some(first) = candidates.first() {
                if let Some(parts) = first
                    .get("content")
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array())
                {
                    for part in parts {
                        // Text content
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            let _ = tx.send(AiUpdate::Content(text.to_string()));
                        }
                        // Function/tool call
                        if let Some(func_call) = part.get("functionCall") {
                            if let Some(name) = func_call.get("name").and_then(|n| n.as_str()) {
                                let args = func_call
                                    .get("args")
                                    .unwrap_or(&serde_json::Value::Null)
                                    .to_string();
                                let _ = tx.send(AiUpdate::ToolCall {
                                    name: name.to_string(),
                                    args,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Extract usage metadata
        if let Some(usage) = json.get("usageMetadata") {
            let prompt_tokens = usage["promptTokenCount"].as_i64().unwrap_or(0) as i32;
            let response_tokens = usage["candidatesTokenCount"].as_i64().unwrap_or(0) as i32;
            let total_tokens = usage["totalTokenCount"].as_i64().unwrap_or(0) as i32;
            let _ = tx.send(AiUpdate::Usage(Usage {
                prompt_tokens,
                response_tokens,
                total_tokens,
            }));
        }
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    async fn stream_response(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tx: UnboundedSender<AiUpdate>,
    ) {
        if self.api_key.is_empty() {
            // Mock mode when no API key is set
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = tx.send(AiUpdate::Content(
                "(Mock Gemini) Set GEMINI_API_KEY for real responses.\n".into(),
            ));
            let _ = tx.send(AiUpdate::Usage(Usage {
                prompt_tokens: 10,
                response_tokens: 20,
                total_tokens: 30,
            }));
            let _ = tx.send(AiUpdate::Finished);
            return;
        }

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
            self.model, self.api_key
        );

        let body = Self::build_body(messages, tools);

        let resp = match self.client.post(&url).json(&body).send().await {
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

            // Process complete lines
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
        "gemini"
    }
}
