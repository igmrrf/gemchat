/// Planner agent implementation.
///
/// Takes high-level goals and produces detailed implementation plans
/// by researching the codebase structure first.
///
/// The planner uses read-only tools (read_file, list_directory, search_google)
/// to understand the project before producing a plan.
use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Planner agent implementation.
pub struct PlannerAgent;

#[async_trait]
impl Agent for PlannerAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Planner
    }

    async fn process(
        &self,
        pipeline: &crate::pipeline::Pipeline,
        user_message: &str,
        working_dir: Option<PathBuf>,
        tx_out: mpsc::Sender<AiUpdate>,
    ) -> StepResult {
        pipeline.execute_agent_with_streaming(self.role(), user_message, working_dir, tx_out).await
    }
}
impl PlannerAgent {
    /// Parse a planner response to extract actionable steps.
    /// Supports both structured JSON and markdown list fallback.
    pub fn extract_plan_items(response: &str) -> Vec<String> {
        // Try JSON parsing first
        if let Some(json_str) = Self::extract_json_block(response) {
            if let Ok(items) = serde_json::from_str::<Vec<String>>(&json_str) {
                return items;
            }
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(&json_str)
                && let Some(items) = obj.get("steps").and_then(|s| s.as_array()) {
                return items.iter().filter_map(|i| i.as_str().map(|s| s.to_string())).collect();
            }
        }

        // Fallback to markdown list parsing
        response
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || (trimmed.len() > 2
                        && trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && trimmed.contains('.'))
            })
            .map(|l| {
                let t = l.trim();
                if t.starts_with("- ") || t.starts_with("* ") {
                    t[2..].trim().to_string()
                } else if let Some(pos) = t.find('.') {
                    t[pos + 1..].trim().to_string()
                } else {
                    t.to_string()
                }
            })
            .collect()
    }

    fn extract_json_block(output: &str) -> Option<String> {
        let trimmed = output.trim();
        if trimmed.starts_with('[') || trimmed.starts_with('{') {
            return Some(trimmed.to_string());
        }

        if let Some(start) = output.find("```json") {
            let rest = &output[start + 7..];
            if let Some(end) = rest.find("```") {
                return Some(rest[..end].trim().to_string());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_plan_items_markdown() {
        let response = "Here's the plan:\n\
            1. Create the config module\n\
            2. Add provider trait\n\
            - Implement Gemini provider\n\
            * Add tests\n\
            This is just a note.";
        let items = PlannerAgent::extract_plan_items(response);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0], "Create the config module");
        assert_eq!(items[2], "Implement Gemini provider");
    }

    #[test]
    fn test_extract_plan_items_json() {
        let response = "```json\n[\"Step 1\", \"Step 2\"]\n```";
        let items = PlannerAgent::extract_plan_items(response);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], "Step 1");
    }
}
