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

impl QaAgent {
    /// Parse a QA response into a pass/fail verdict.
    pub fn parse_verdict(response: &str) -> QaVerdict {
        let lower = response.to_lowercase();
        if lower.contains("all tests pass") || lower.contains("qa pass") || lower.contains("✅") {
            QaVerdict::Pass
        } else {
            let failures: Vec<String> = response
                .lines()
                .filter(|l| {
                    let t = l.trim().to_lowercase();
                    t.contains("fail") || t.contains("error") || t.contains("❌")
                })
                .map(|l| l.trim().to_string())
                .collect();
            QaVerdict::Fail(failures)
        }
    }
}
