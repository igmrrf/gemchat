use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Researcher agent implementation.
///
/// Explores codebases, searches documentation, and summarizes findings
/// for other agents. Uses read-only tools only.
pub struct ResearcherAgent;

#[async_trait]
impl Agent for ResearcherAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Researcher
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

impl ResearcherAgent {
    /// Extract key findings from a researcher response.
    pub fn extract_findings(response: &str) -> Vec<String> {
        response
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("- ") || trimmed.starts_with("* ")
            })
            .map(|l| l.trim().to_string())
            .collect()
    }
}
