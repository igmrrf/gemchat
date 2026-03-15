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

impl ReviewerAgent {
    /// Parse a reviewer response into a verdict.
    pub fn parse_verdict(response: &str) -> ReviewVerdict {
        let lower = response.to_lowercase();
        if lower.contains("approved") && !lower.contains("changes_needed") {
            ReviewVerdict::Approved
        } else {
            // Extract specific feedback items
            let items: Vec<String> = response
                .lines()
                .filter(|l| {
                    let t = l.trim();
                    t.starts_with("- ") || t.starts_with("* ")
                })
                .map(|l| l.trim().to_string())
                .collect();
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
