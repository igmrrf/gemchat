use std::collections::HashMap;

/// Shared context passed between agents in a pipeline run.
///
/// Stores outputs from each agent so subsequent agents can
/// reference earlier work.
pub struct PipelineContext {
    /// Agent name → their output text
    outputs: HashMap<String, String>,
    /// Ordered list of (agent_name, output) for recency queries
    history: Vec<(String, String)>,
}

impl PipelineContext {
    pub fn new() -> Self {
        Self {
            outputs: HashMap::new(),
            history: Vec::new(),
        }
    }

    /// Store an agent's output.
    pub fn add_output(&mut self, agent: String, output: String) {
        self.outputs.insert(agent.clone(), output.clone());
        self.history.push((agent, output));
    }

    /// Get a specific agent's output.
    pub fn get_output(&self, agent: &str) -> Option<&str> {
        self.outputs.get(agent).map(|s| s.as_str())
    }

    /// Get the N most recent outputs (agent_name, output).
    pub fn recent_outputs(&self, n: usize) -> Vec<(&str, &str)> {
        self.history
            .iter()
            .rev()
            .take(n)
            .map(|(a, o)| (a.as_str(), o.as_str()))
            .collect()
    }

    /// Clear all context (for new session).
    pub fn clear(&mut self) {
        self.outputs.clear();
        self.history.clear();
    }
}

impl Default for PipelineContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_store_and_retrieve() {
        let mut ctx = PipelineContext::new();
        ctx.add_output("planner".into(), "Plan: do stuff".into());
        ctx.add_output("coder".into(), "Code: done".into());

        assert_eq!(ctx.get_output("planner"), Some("Plan: do stuff"));
        assert_eq!(ctx.get_output("coder"), Some("Code: done"));
        assert_eq!(ctx.get_output("reviewer"), None);
    }

    #[test]
    fn test_recent_outputs() {
        let mut ctx = PipelineContext::new();
        ctx.add_output("a".into(), "1".into());
        ctx.add_output("b".into(), "2".into());
        ctx.add_output("c".into(), "3".into());

        let recent = ctx.recent_outputs(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].0, "c"); // Most recent first
    }
}
