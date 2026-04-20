pub mod architect;
pub mod coder;
pub mod executor;
pub mod orchestrator;
pub mod planner;
pub mod qa;
pub mod researcher;
pub mod reviewer;
pub mod skill;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use std::path::PathBuf;

use crate::provider::AiUpdate;
use crate::pipeline::step::StepResult;

/// Shared trait for all agents in the system.
#[async_trait]
pub trait Agent: Send + Sync {
    /// The role of this agent.
    fn role(&self) -> AgentRole;

    /// Process a task given a user message and an optional working directory.
    /// Streams updates through `tx_out` and returns the final `StepResult`.
    async fn process(
        &self,
        pipeline: &crate::pipeline::Pipeline,
        user_message: &str,
        working_dir: Option<PathBuf>,
        tx_out: mpsc::Sender<AiUpdate>,
    ) -> StepResult;
}

/// Unique agent roles in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    Orchestrator,
    Planner,
    Researcher,
    Architect,
    Coder,
    Reviewer,
    Qa,
    Executor,
}

impl AgentRole {
    /// String key used for config lookups.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Planner => "planner",
            Self::Researcher => "researcher",
            Self::Architect => "architect",
            Self::Coder => "coder",
            Self::Reviewer => "reviewer",
            Self::Qa => "qa",
            Self::Executor => "executor",
        }
    }

    /// Which tools this role is allowed to use.
    pub fn allowed_tools(&self) -> &[&str] {
        match self {
            Self::Orchestrator => &["search_google", "read_file", "list_directory"],
            Self::Planner => &["search_google", "read_file", "list_directory"],
            Self::Researcher => &["search_google", "read_file", "list_directory"],
            Self::Architect => &["read_file", "list_directory", "git_diff", "git_status"],
            Self::Coder => &[
                "read_file", "create_file", "update_file", "delete_file",
                "list_directory", "run_command", "git_diff", "git_status",
            ],
            Self::Reviewer => &["read_file", "list_directory", "git_diff", "git_status", "run_tests"],
            Self::Qa => &["read_file", "list_directory", "run_command", "run_tests", "git_status"],
            Self::Executor => &[
                "run_command", "git_commit", "git_branch", "git_diff", "git_status",
            ],
        }
    }

    /// Default skills for this role.
    pub fn default_skills(&self) -> Vec<String> {
        match self {
            Self::Coder => vec!["rust_expert".into()],
            Self::Qa => vec!["tdd".into()],
            _ => vec![],
        }
    }

    /// System prompt for this agent.
    pub fn system_prompt(&self) -> &str {
        match self {
            Self::Orchestrator => ORCHESTRATOR_SYSTEM,
            Self::Planner => PLANNER_SYSTEM,
            Self::Researcher => RESEARCHER_SYSTEM,
            Self::Architect => ARCHITECT_SYSTEM,
            Self::Coder => CODER_SYSTEM,
            Self::Reviewer => REVIEWER_SYSTEM,
            Self::Qa => QA_SYSTEM,
            Self::Executor => EXECUTOR_SYSTEM,
        }
    }

    /// User-friendly display name.
    pub fn display_name(&self) -> &str {
        match self {
            Self::Orchestrator => "🎯 Orchestrator",
            Self::Planner => "📋 Planner",
            Self::Researcher => "🔍 Researcher",
            Self::Architect => "🏗️ Architect",
            Self::Coder => "💻 Coder",
            Self::Reviewer => "👀 Reviewer",
            Self::Qa => "🧪 QA",
            Self::Executor => "🚀 Executor",
        }
    }
}

impl std::fmt::Display for AgentRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// ── System prompts ──

const ORCHESTRATOR_SYSTEM: &str = "\
You are the Orchestrator agent. You analyze user requests, decompose them into \
sub-tasks, decide which agents should handle each sub-task, and determine the \
execution order. You identify dependencies between tasks and decide what can \
run in parallel vs serially. Output a structured plan as JSON with steps, \
their assigned agents, dependencies, and descriptions.";

const PLANNER_SYSTEM: &str = "\
You are the Planner agent. You take high-level goals and create detailed, \
step-by-step implementation plans. Research the codebase structure first, \
then produce a specific plan with file paths, functions to modify, and the \
order of changes. Be thorough but concise.";

const RESEARCHER_SYSTEM: &str = "\
You are the Researcher agent. You explore codebases, search for relevant \
documentation, patterns, and context. Summarize findings clearly for other \
agents. Focus on identifying existing patterns, dependencies, and potential \
impacts of changes.";

const ARCHITECT_SYSTEM: &str = "\
You are the Architect agent. You design system architecture, review proposed \
changes for structural soundness, and ensure consistency with existing patterns. \
Output architecture decisions, data flow diagrams (in text), and interface \
contracts.";

const CODER_SYSTEM: &str = "\
You are the Coder agent. You write production-quality code following the plan \
and architecture provided. Create files, modify existing code, and run commands \
when needed. Write clean, well-documented, idiomatic code. Use the tools \
provided to interact with the filesystem.";

const REVIEWER_SYSTEM: &str = "\
You are the Reviewer agent. You review code changes for correctness, style, \
security issues, and adherence to the plan. Run tests when appropriate. \
Provide specific, actionable feedback. If changes are acceptable, state \
APPROVED. If not, state CHANGES_NEEDED with specific items.";

const QA_SYSTEM: &str = "\
You are the QA agent. You validate that changes work correctly by running \
tests, checking edge cases, and verifying the implementation matches \
requirements. Report pass/fail status with details. Auto-generate test \
cases when appropriate.";

const EXECUTOR_SYSTEM: &str = "\
You are the Executor agent. You handle git operations (commits, branches, \
merges) and deployment-related commands. Follow conventional commit message \
format. Only act when instructed by the orchestrator or after QA approval.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_string_roundtrip() {
        assert_eq!(AgentRole::Coder.as_str(), "coder");
        assert_eq!(AgentRole::Qa.as_str(), "qa");
    }

    #[test]
    fn test_coder_has_write_tools() {
        let tools = AgentRole::Coder.allowed_tools();
        assert!(tools.contains(&"create_file"));
        assert!(tools.contains(&"run_command"));
    }

    #[test]
    fn test_researcher_no_write_tools() {
        let tools = AgentRole::Researcher.allowed_tools();
        assert!(!tools.contains(&"create_file"));
        assert!(!tools.contains(&"run_command"));
    }
}
