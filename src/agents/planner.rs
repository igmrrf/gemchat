/// Planner agent implementation.
///
/// Takes high-level goals and produces detailed implementation plans
/// by researching the codebase structure first.
///
/// The planner uses read-only tools (read_file, list_directory, search_google)
/// to understand the project before producing a plan.

// Currently the planner logic is driven by the system prompt in mod.rs
// and the pipeline coordinator. This module exists for future expansion
// (e.g., plan-specific parsing, structured output validation).

/// Marker type for planner-specific behavior.
pub struct PlannerAgent;

impl PlannerAgent {
    /// Parse a planner response to extract actionable steps.
    /// In Phase 1, this is a simple extraction; Phase 2 will
    /// use structured JSON output.
    pub fn extract_plan_items(response: &str) -> Vec<String> {
        response
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || (trimmed.len() > 2
                        && trimmed.chars().next().map_or(false, |c| c.is_ascii_digit())
                        && trimmed.contains('.'))
            })
            .map(|l| l.trim().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_plan_items() {
        let response = "Here's the plan:\n\
            1. Create the config module\n\
            2. Add provider trait\n\
            - Implement Gemini provider\n\
            * Add tests\n\
            This is just a note.";
        let items = PlannerAgent::extract_plan_items(response);
        assert_eq!(items.len(), 4);
    }
}
