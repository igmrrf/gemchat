use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

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

pub struct OrchestratorAgent;

#[async_trait]
impl Agent for OrchestratorAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Orchestrator
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

impl OrchestratorAgent {
    /// Decompose a user request into a structured execution plan.
    pub async fn decompose(
        &self,
        pipeline: &crate::pipeline::Pipeline,
        user_message: &str,
        working_dir: Option<PathBuf>,
        tx_out: mpsc::Sender<AiUpdate>,
    ) -> color_eyre::Result<ExecutionPlan> {
        let decomposition_prompt = format!(
            "Decompose the following user request into a structured execution plan. \
            Output ONLY a JSON object with 'task_summary' and 'steps'. \
            Each step must have 'id' (numeric), 'agent' (role), 'description', and 'depends_on' (array of IDs). \
            Available agents: planner, researcher, architect, coder, reviewer, qa, executor.\n\n\
            Request: {}",
            user_message
        );

        let result = self.process(pipeline, &decomposition_prompt, working_dir, tx_out).await;

        if result.status == crate::pipeline::step::StepStatus::Failed {
            return Err(color_eyre::eyre::eyre!("Decomposition failed: {}", result.output));
        }

        let plan_json = Self::extract_json(&result.output)?;
        let plan: ExecutionPlan = serde_json::from_str(&plan_json)?;
        Ok(plan)
    }

    fn extract_json(output: &str) -> color_eyre::Result<String> {
        let trimmed = output.trim();
        let json = if trimmed.starts_with("```json") {
            trimmed.trim_start_matches("```json").trim_end_matches("```")
        } else if trimmed.starts_with("```") {
            trimmed.trim_start_matches("```").trim_end_matches("```")
        } else {
            trimmed
        };
        Ok(json.trim().to_string())
    }
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
