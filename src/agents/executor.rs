use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Executor agent implementation.
///
/// Handles git operations (commits, branches, merges) and
/// deployment-related commands. Only acts after approval.
pub struct ExecutorAgent;

#[async_trait]
impl Agent for ExecutorAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Executor
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

impl ExecutorAgent {
    /// Build a conventional commit message from a description.
    pub fn format_commit_message(change_type: &str, description: &str) -> String {
        format!("{}: {}", change_type, description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_message_format() {
        let msg = ExecutorAgent::format_commit_message("feat", "add multi-agent pipeline");
        assert_eq!(msg, "feat: add multi-agent pipeline");
    }
}
