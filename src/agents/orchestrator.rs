use serde::{Deserialize, Serialize};

use super::AgentRole;

/// A step in the orchestrator's execution plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: u32,
    pub description: String,
    pub agent: AgentRole,
    /// IDs of steps that must complete before this one runs.
    pub depends_on: Vec<u32>,
}

/// The orchestrator analyzes a task and produces this plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub task_summary: String,
    pub steps: Vec<PlanStep>,
}

impl ExecutionPlan {
    /// Get steps that have no dependencies (can start immediately).
    pub fn root_steps(&self) -> Vec<&PlanStep> {
        self.steps.iter().filter(|s| s.depends_on.is_empty()).collect()
    }

    /// Get steps whose dependencies are all in `completed_ids`.
    pub fn ready_steps(&self, completed_ids: &[u32]) -> Vec<&PlanStep> {
        self.steps
            .iter()
            .filter(|s| {
                !completed_ids.contains(&s.id)
                    && s.depends_on.iter().all(|dep| completed_ids.contains(dep))
            })
            .collect()
    }
}
