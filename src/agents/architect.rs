/// Architect agent implementation.
///
/// Designs system architecture and reviews structural soundness.
/// Uses read-only tools plus git diff/status for context.

pub struct ArchitectAgent;

impl ArchitectAgent {
    /// Check if an architect response contains an approval verdict.
    pub fn is_approved(response: &str) -> bool {
        let lower = response.to_lowercase();
        lower.contains("approved") && !lower.contains("not approved")
    }
}
