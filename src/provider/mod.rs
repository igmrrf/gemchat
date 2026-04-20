pub mod claude;
pub mod gemini;
pub mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;

use crate::config::AppConfig;

// ── Shared types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user" | "model" | "assistant" | "system" | "tool"
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub response_tokens: i32,
    pub total_tokens: i32,
}

/// Events streamed from AI providers back to the caller.
pub enum AiUpdate {
    /// Incremental text content
    Content(String),
    /// AI wants to call a tool
    ToolCall { name: String, args: String },
    /// Tool call needs user approval. Returns (approved, optional_updated_args)
    PendingApproval {
        name: String,
        args: String,
        tx: tokio::sync::oneshot::Sender<(bool, Option<String>)>,
    },
    /// Result of a tool execution
    ToolResult { name: String, result: String },
    /// Request direct user input (e.g. for interactive shell)
    RequestInput {
        name: String,
        args: String,
        tx: tokio::sync::oneshot::Sender<String>,
    },
    /// Token usage stats
    Usage(Usage),
    /// Stream complete
    Finished,
    /// An error occurred
    Error(String),
}

impl std::fmt::Debug for AiUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Content(s) => f.debug_tuple("Content").field(s).finish(),
            Self::ToolCall { name, args } => f
                .debug_struct("ToolCall")
                .field("name", name)
                .field("args", args)
                .finish(),
            Self::PendingApproval { name, args, .. } => f
                .debug_struct("PendingApproval")
                .field("name", name)
                .field("args", args)
                .finish(),
            Self::ToolResult { name, result } => f
                .debug_struct("ToolResult")
                .field("name", name)
                .field("result", result)
                .finish(),
            Self::RequestInput { name, args, .. } => f
                .debug_struct("RequestInput")
                .field("name", name)
                .field("args", args)
                .finish(),
            Self::Usage(u) => f.debug_tuple("Usage").field(u).finish(),
            Self::Finished => write!(f, "Finished"),
            Self::Error(e) => f.debug_tuple("Error").field(e).finish(),
        }
    }
}

/// Core trait all AI providers implement.
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// Stream a response given conversation history and available tools.
    async fn stream_response(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        tx: Sender<AiUpdate>,
    );

    /// The model identifier (e.g. "gemini-2.5-flash")
    fn model_name(&self) -> &str;

    /// The provider name (e.g. "gemini", "claude", "openai")
    fn provider_name(&self) -> &str;
}

/// Create a provider by parsing the model name prefix.
///
/// - `gemini-*` → GeminiProvider
/// - `claude-*` → ClaudeProvider
/// - `gpt-*` / `codex-*` / `o1-*` / `o3-*` → OpenAiProvider
pub fn create_provider(
    model: &str,
    config: &AppConfig,
) -> color_eyre::Result<Box<dyn AiProvider>> {
    if model.starts_with("gemini-") {
        let api_key_env = if config.providers.gemini.api_key_env.is_empty() {
            "GEMINI_API_KEY"
        } else {
            &config.providers.gemini.api_key_env
        };
        let api_key = std::env::var(api_key_env).unwrap_or_default();
        Ok(Box::new(gemini::GeminiProvider::new(
            model.to_string(),
            api_key,
        )))
    } else if model.starts_with("claude-") {
        let api_key_env = if config.providers.claude.api_key_env.is_empty() {
            "ANTHROPIC_API_KEY"
        } else {
            &config.providers.claude.api_key_env
        };
        let api_key = std::env::var(api_key_env).unwrap_or_default();
        Ok(Box::new(claude::ClaudeProvider::new(
            model.to_string(),
            api_key,
        )))
    } else if model.starts_with("gpt-")
        || model.starts_with("codex-")
        || model.starts_with("o1-")
        || model.starts_with("o3-")
    {
        let api_key_env = if config.providers.openai.api_key_env.is_empty() {
            "OPENAI_API_KEY"
        } else {
            &config.providers.openai.api_key_env
        };
        let api_key = std::env::var(api_key_env).unwrap_or_default();

        // Support custom base URL via provider extra config
        let base_url = config
            .providers
            .openai
            .extra
            .get("base_url")
            .cloned()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

        Ok(Box::new(openai::OpenAiProvider::with_base_url(
            model.to_string(),
            api_key,
            base_url,
        )))
    } else {
        // Default to Gemini for unknown prefixes
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        Ok(Box::new(gemini::GeminiProvider::new(
            model.to_string(),
            api_key,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_gemini_provider() {
        let config = AppConfig::default();
        let provider = create_provider("gemini-2.5-flash", &config).unwrap();
        assert_eq!(provider.provider_name(), "gemini");
        assert_eq!(provider.model_name(), "gemini-2.5-flash");
    }

    #[test]
    fn test_create_claude_provider() {
        let config = AppConfig::default();
        let provider = create_provider("claude-sonnet-4-20250514", &config).unwrap();
        assert_eq!(provider.provider_name(), "claude");
        assert_eq!(provider.model_name(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn test_create_openai_provider() {
        let config = AppConfig::default();
        let provider = create_provider("gpt-4o", &config).unwrap();
        assert_eq!(provider.provider_name(), "openai");
        assert_eq!(provider.model_name(), "gpt-4o");
    }

    #[test]
    fn test_create_openai_o_series() {
        let config = AppConfig::default();
        let provider = create_provider("o3-mini", &config).unwrap();
        assert_eq!(provider.provider_name(), "openai");
    }

    #[test]
    fn test_unknown_prefix_defaults_to_gemini() {
        let config = AppConfig::default();
        let provider = create_provider("mystery-model", &config).unwrap();
        assert_eq!(provider.provider_name(), "gemini");
    }
}
