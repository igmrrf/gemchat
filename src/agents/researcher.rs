/// Researcher agent implementation.
///
/// Explores codebases, searches documentation, and summarizes findings
/// for other agents. Uses read-only tools only.

pub struct ResearcherAgent;

impl ResearcherAgent {
    /// Extract key findings from a researcher response.
    pub fn extract_findings(response: &str) -> Vec<String> {
        response
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.starts_with("- ") || trimmed.starts_with("* ")
            })
            .map(|l| l.trim().to_string())
            .collect()
    }
}
