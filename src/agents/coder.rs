/// Coder agent implementation.
///
/// The primary code-writing agent. Has access to filesystem tools,
/// command execution, and git read tools.

pub struct CoderAgent;

impl CoderAgent {
    /// Check if a coder response indicates completion.
    pub fn is_complete(response: &str) -> bool {
        let lower = response.to_lowercase();
        lower.contains("implementation complete")
            || lower.contains("changes complete")
            || lower.contains("done")
    }
}
