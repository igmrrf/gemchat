/// Executor agent implementation.
///
/// Handles git operations (commits, branches, merges) and
/// deployment-related commands. Only acts after approval.

pub struct ExecutorAgent;

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
