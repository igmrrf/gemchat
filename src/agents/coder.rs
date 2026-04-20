use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Coder agent implementation.
///
/// The primary code-writing agent. Has access to filesystem tools,
/// command execution, and git read tools.
pub struct CoderAgent;

#[async_trait]
impl Agent for CoderAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Coder
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

impl CoderAgent {
    /// Check if a coder response indicates completion.
    pub fn is_complete(response: &str) -> bool {
        let lower = response.to_lowercase();
        lower.contains("implementation complete")
            || lower.contains("changes complete")
            || lower.contains("done")
    }
}
