use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// QA agent implementation.
///
/// Validates changes by running tests and checking edge cases.
/// Can run commands and test suites.
pub struct QaAgent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QaVerdict {
    Pass,
    Fail(Vec<String>),
}

#[async_trait]
impl Agent for QaAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Qa
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

impl QaAgent {
    /// Parse a QA response into a pass/fail verdict.
    pub fn parse_verdict(response: &str) -> QaVerdict {
        let lower = response.to_lowercase();
        
        if (lower.contains("verdict: pass") || lower.contains("qa pass") || lower.contains("all tests pass") || lower.contains("✅"))
            && !lower.contains("fail") 
            && !lower.contains("❌") 
        {
            return QaVerdict::Pass;
        }

        let failures: Vec<String> = response
            .lines()
            .filter(|l| {
                let t = l.trim().to_lowercase();
                t.contains("fail") || t.contains("error") || t.contains("❌") || t.starts_with("- ")
            })
            .map(|l| {
                let t = l.trim();
                if let Some(stripped) = t.strip_prefix("- ") {
                    stripped.trim().to_string()
                } else {
                    t.to_string()
                }
            })
            .collect();

        if failures.is_empty() && (lower.contains("pass") || lower.contains("success")) {
            QaVerdict::Pass
        } else {
            QaVerdict::Fail(failures)
        }
    }
}
