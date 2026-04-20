use async_trait::async_trait;
use tokio::sync::mpsc;
use std::path::PathBuf;

use super::{Agent, AgentRole};
use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Reviewer agent implementation.
///
/// Reviews code changes for correctness, style, security, and adherence
/// to the plan. Can run tests.
pub struct ReviewerAgent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approved,
    ChangesNeeded(Vec<String>),
}

#[async_trait]
impl Agent for ReviewerAgent {
    fn role(&self) -> AgentRole {
        AgentRole::Reviewer
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

impl ReviewerAgent {
    /// Parse a reviewer response into a verdict.
    pub fn parse_verdict(response: &str) -> ReviewVerdict {
        let lower = response.to_lowercase();
        
        // Look for explicit Verdict: APPROVED or APPROVED marker
        if (lower.contains("verdict: approved") || lower.contains("approved")) 
            && !lower.contains("changes_needed") 
            && !lower.contains("changes needed") 
        {
            return ReviewVerdict::Approved;
        }

        // Extract specific feedback items
        let items: Vec<String> = response
            .lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("- ") || t.starts_with("* ")
            })
            .map(|l| l.trim()[2..].trim().to_string())
            .collect();

        if items.is_empty() && lower.contains("approved") {
            ReviewVerdict::Approved
        } else {
            ReviewVerdict::ChangesNeeded(items)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approved_verdict() {
        let response = "Code looks good. APPROVED.";
        assert_eq!(ReviewerAgent::parse_verdict(response), ReviewVerdict::Approved);
    }

    #[test]
    fn test_changes_needed_verdict() {
        let response = "CHANGES_NEEDED\n- Fix error handling in line 42\n- Add tests";
        match ReviewerAgent::parse_verdict(response) {
            ReviewVerdict::ChangesNeeded(items) => assert_eq!(items.len(), 2),
            _ => panic!("Expected ChangesNeeded"),
        }
    }
}
