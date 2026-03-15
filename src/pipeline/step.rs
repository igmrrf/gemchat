use crate::agents::AgentRole;
use crate::provider::Usage;

/// Status of a pipeline step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

/// Record of a tool call made during a step.
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub name: String,
    pub args: String,
    pub result: String,
}

/// Result of executing a single agent step.
#[derive(Debug)]
pub struct StepResult {
    pub role: AgentRole,
    pub status: StepStatus,
    pub output: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub usage: Option<Usage>,
}

impl StepResult {
    /// Did this step complete successfully?
    pub fn is_success(&self) -> bool {
        self.status == StepStatus::Completed
    }

    /// Total tokens used by this step.
    pub fn total_tokens(&self) -> i32 {
        self.usage.as_ref().map_or(0, |u| u.total_tokens)
    }
}
