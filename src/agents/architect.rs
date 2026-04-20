use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Architect agent implementation.
///
/// Designs system architecture and reviews structural soundness.
/// Uses read-only tools plus git diff/status for context.
pub struct ArchitectAgent;

#[async_trait]
impl Agent for ArchitectAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Architect
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

impl ArchitectAgent {
    /// Check if an architect response contains an approval verdict.
    pub fn is_approved(response: &str) -> bool {
        let lower = response.to_lowercase();
        lower.contains("approved") && !lower.contains("not approved")
    }
}
