use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use tokio::sync::mpsc::Sender;

use super::{AiProvider, AiUpdate, ChatMessage, ToolDefinition, Usage};

/// Gemini API provider (Google AI Studio / Vertex).
pub struct GeminiProvider {
    pub api_key: String,
    pub model: String,
    pub client: Client,
}

impl GeminiProvider {
    pub fn new(model: String, api_key: String) -> Self {
        Self {
            api_key,
            model,
            client: Client::new(),
        }
    }

    fn build_body(messages: &[ChatMessage], tools: &[ToolDefinition]) -> serde_json::Value {
        let mut contents = Vec::new();

        for msg in messages {
            let role = match msg.role.as_str() {
                "user" => "user",
                _ => "model",
            };
            contents.push(json!({
                "role": role,
                "parts": [{"text": msg.content}]
            }));
        }

        let mut body = json!({
            "contents": contents,
        });

        if !tools.is_empty() {
            let mut tool_defs = Vec::new();
            for tool in tools {
                tool_defs.push(json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }));
            }
            body["tools"] = json!([{
                "function_declarations": tool_defs
            }]);
        }

        body
    }

    /// Parse an SSE data line from the Gemini streaming API.
    async fn parse_sse_line(line: &str, tx: &Sender<AiUpdate>) {
        if !line.starts_with("data: ") {
            return;
        }

        let json_str = &line[6..];
        let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) else {
            return;
        };

        // Extract content and tool calls from candidates
        if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array())
            && let Some(first) = candidates.first()
            && let Some(parts) = first
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array())
        {
            for part in parts {
                // Text content
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    let _ = tx.send(AiUpdate::Content(text.to_string())).await;
                }
                // Function/tool call
                if let Some(func_call) = part.get("functionCall")
                    && let Some(name) = func_call.get("name").and_then(|n| n.as_str())
                {
                    let args = func_call
                        .get("args")
                        .unwrap_or(&serde_json::Value::Null)
                        .to_string();
                    let _ = tx
                        .send(AiUpdate::ToolCall {
                            name: name.to_string(),
                            args,
                        })
                        .await;
                }
            }
        }

        // Extract usage metadata
        if let Some(usage) = json.get("usageMetadata") {
            let prompt_tokens = usage["promptTokenCount"].as_i64().unwrap_or(0) as i32;
            let response_tokens = usage["candidatesTokenCount"].as_i64().unwrap_or(0) as i32;
            let total_tokens = usage["totalTokenCount"].as_i64().unwrap_or(0) as i32;
            let _ = tx
                .send(AiUpdate::Usage(Usage {
                    prompt_tokens,
                    response_tokens,
                    total_tokens,
                }))
                .await;
        }
    }
}

#[async_trait]
impl AiProvider for GeminiProvider {
    async fn stream_response(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tx: Sender<AiUpdate>,
    ) {
        if self.api_key.is_empty() {
            // Mock mode when no API key is set
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let _ = tx
                .send(AiUpdate::Content(
                    "(Mock Gemini) Set GEMINI_API_KEY for real responses.\n".into(),
                ))
                .await;
            let _ = tx
                .send(AiUpdate::Usage(Usage {
                    prompt_tokens: 10,
                    response_tokens: 20,
                    total_tokens: 30,
                }))
                .await;
            let _ = tx.send(AiUpdate::Finished).await;
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
                let _ = tx.send(AiUpdate::Error(format!("Request failed: {}", e))).await;
                let _ = tx.send(AiUpdate::Finished).await;
                return;
            }
        };

        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            if let Ok(bytes) = chunk {
                if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                    buffer.push_str(&text);
                    while let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].to_string();
                        buffer = buffer[pos + 1..].to_string();
                        Self::parse_sse_line(&line, &tx).await;
                    }
                }
            }
        }
        let _ = tx.send(AiUpdate::Finished).await;
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn provider_name(&self) -> &str {
        "gemini"
    }
}
