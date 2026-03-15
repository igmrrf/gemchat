pub mod command;
pub mod filesystem;
pub mod git;
pub mod search;
pub mod test_runner;

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

use crate::config::ApprovalTier;

/// Safety classification for tool approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyTier {
    /// Auto-approved in tiered mode (read-only, no side effects)
    Safe,
    /// Requires user confirmation in tiered mode
    Dangerous,
}

/// Core trait all tools implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (matches what AI calls)
    fn name(&self) -> &str;
    /// Human-readable description for the AI
    fn description(&self) -> &str;
    /// JSON Schema for parameters
    fn parameters(&self) -> Value;
    /// Safety classification
    fn safety_tier(&self) -> SafetyTier;
    /// Execute the tool with given arguments in working directory
    async fn execute(&self, args: Value, working_dir: &Path) -> String;
}

/// Registry managing all available tools.
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create a new registry with all built-in tools.
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };

        // Register filesystem tools
        registry.register(Box::new(filesystem::ReadFile));
        registry.register(Box::new(filesystem::CreateFile));
        registry.register(Box::new(filesystem::UpdateFile));
        registry.register(Box::new(filesystem::DeleteFile));
        registry.register(Box::new(filesystem::ListDirectory));

        // Register command tools
        registry.register(Box::new(command::RunCommand));

        // Register search tools
        registry.register(Box::new(search::SearchGoogle));

        // Register git tools
        registry.register(Box::new(git::GitDiff));
        registry.register(Box::new(git::GitStatus));
        registry.register(Box::new(git::GitCommit));
        registry.register(Box::new(git::GitBranch));

        // Register test runner
        registry.register(Box::new(test_runner::RunTests));

        registry
    }

    fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Execute a tool by name.
    pub async fn execute(&self, name: &str, args: &str, working_dir: &Path) -> String {
        let Some(tool) = self.tools.get(name) else {
            return format!("Error: Unknown tool '{}'", name);
        };

        let args_value: Value = serde_json::from_str(args).unwrap_or(Value::Null);
        tool.execute(args_value, working_dir).await
    }

    /// Check if a tool requires user approval under the given tier.
    pub fn needs_approval(&self, tool_name: &str, tier: ApprovalTier) -> bool {
        match tier {
            ApprovalTier::Autonomous => false,
            ApprovalTier::Manual => true,
            ApprovalTier::Tiered => {
                if let Some(tool) = self.tools.get(tool_name) {
                    tool.safety_tier() == SafetyTier::Dangerous
                } else {
                    true // Unknown tools require approval
                }
            }
        }
    }

    /// Get all tool definitions (for sending to AI).
    pub fn tool_definitions(&self) -> Vec<crate::provider::ToolDefinition> {
        self.tools
            .values()
            .map(|tool| crate::provider::ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect()
    }

    /// Get tool definitions filtered to only allowed tool names.
    pub fn tool_definitions_for(&self, allowed: &[&str]) -> Vec<crate::provider::ToolDefinition> {
        self.tools
            .values()
            .filter(|t| allowed.contains(&t.name()))
            .map(|tool| crate::provider::ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters(),
            })
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Backward-compatible free function for the existing TUI.
/// Wraps the registry pattern so callers can just do `tools::execute_tool(name, args)`.
pub async fn execute_tool(name: &str, args: &str) -> String {
    let registry = ToolRegistry::new();
    let working_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    registry.execute(name, args, &working_dir).await
}

/// Helper: extract a string field from a JSON value.
pub fn extract_field(args: &Value, field: &str) -> Option<String> {
    args.get(field).and_then(|v| v.as_str()).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_all_tools() {
        let reg = ToolRegistry::new();
        assert!(reg.get("read_file").is_some());
        assert!(reg.get("create_file").is_some());
        assert!(reg.get("run_command").is_some());
        assert!(reg.get("search_google").is_some());
        assert!(reg.get("git_diff").is_some());
        assert!(reg.get("run_tests").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_safety_tiers() {
        let reg = ToolRegistry::new();
        assert_eq!(reg.get("read_file").unwrap().safety_tier(), SafetyTier::Safe);
        assert_eq!(reg.get("run_command").unwrap().safety_tier(), SafetyTier::Dangerous);
        assert_eq!(reg.get("delete_file").unwrap().safety_tier(), SafetyTier::Dangerous);
    }

    #[test]
    fn test_approval_logic() {
        let reg = ToolRegistry::new();
        // Autonomous: never needs approval
        assert!(!reg.needs_approval("run_command", ApprovalTier::Autonomous));
        // Manual: always needs approval
        assert!(reg.needs_approval("read_file", ApprovalTier::Manual));
        // Tiered: depends on safety tier
        assert!(!reg.needs_approval("read_file", ApprovalTier::Tiered));
        assert!(reg.needs_approval("run_command", ApprovalTier::Tiered));
    }
}
