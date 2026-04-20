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
    /// Whether this tool requires direct user input (interactive)
    fn requires_input(&self, _args: &Value) -> bool { false }
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

    /// Check if a tool requires input.
    pub fn requires_input(&self, name: &str, args: &str) -> bool {
        if let Some(tool) = self.tools.get(name) {
            let args_value: Value = serde_json::from_str(args).unwrap_or(Value::Null);
            tool.requires_input(&args_value)
        } else {
            false
        }
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
    pub fn needs_approval(&self, tool_name: &str, config: &crate::config::ApprovalConfig) -> bool {
        // 1. Check explicit user overrides
        if config.safe_tools.contains(&tool_name.to_string()) {
            return false;
        }
        if config.dangerous_tools.contains(&tool_name.to_string()) {
            return true;
        }

        // 2. Fall back to tier-based logic
        match config.default_tier {
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

    /// Get tool definitions filtered to only allowed tool names (Vec version).
    pub fn tool_definitions_for_vec(&self, allowed: &[String]) -> Vec<crate::provider::ToolDefinition> {
        self.tools
            .values()
            .filter(|t| allowed.contains(&t.name().to_string()))
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

/// Helper: resolve a path and ensure it stays within the working directory.
pub fn resolve_safe_path(working_dir: &Path, user_path: &str) -> Result<std::path::PathBuf, String> {
    let user_path = Path::new(user_path);
    
    // 1. Prevent absolute paths
    if user_path.is_absolute() {
        return Err("Error: Absolute paths are not allowed for security reasons.".to_string());
    }

    // 2. Resolve the path relative to working_dir
    let _joined = working_dir.join(user_path);
    
    // 3. Canonicalize both paths to compare them
    // Note: canonicalize() requires the path to exist for some OS, 
    // but we want to allow creating new files.
    // So we'll use a simpler check for parent-of-working-dir traversal.
    
    let mut components = std::collections::VecDeque::new();
    for component in user_path.components() {
        match component {
            std::path::Component::Normal(c) => components.push_back(c),
            std::path::Component::CurDir => {},
            std::path::Component::ParentDir => {
                if components.pop_back().is_none() {
                    return Err("Error: Path traversal outside the working directory is not allowed.".to_string());
                }
            }
            _ => return Err("Error: Invalid path component.".to_string()),
        }
    }

    Ok(working_dir.join(user_path))
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
        use crate::config::ApprovalConfig;
        let reg = ToolRegistry::new();
        
        // Autonomous: never needs approval unless explicitly marked dangerous
        let mut autonomous = ApprovalConfig::default();
        autonomous.default_tier = ApprovalTier::Autonomous;
        autonomous.dangerous_tools = vec![]; // Clear defaults for test
        assert!(!reg.needs_approval("run_command", &autonomous));

        // Manual: always needs approval unless explicitly marked safe
        let mut manual = ApprovalConfig::default();
        manual.default_tier = ApprovalTier::Manual;
        manual.safe_tools = vec![]; // Clear defaults for test
        assert!(reg.needs_approval("read_file", &manual));

        // Tiered: depends on safety tier
        let mut tiered = ApprovalConfig::default();
        tiered.default_tier = ApprovalTier::Tiered;
        tiered.safe_tools = vec![];
        tiered.dangerous_tools = vec![];
        assert!(!reg.needs_approval("read_file", &tiered));
        assert!(reg.needs_approval("run_command", &tiered));

        // Overrides
        let mut overrides = ApprovalConfig::default();
        overrides.safe_tools = vec!["run_command".into()];
        assert!(!reg.needs_approval("run_command", &overrides));

        let mut overrides2 = ApprovalConfig::default();
        overrides2.safe_tools = vec![];
        overrides2.dangerous_tools = vec!["read_file".into()];
        assert!(reg.needs_approval("read_file", &overrides2));
    }
}
