use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use super::{extract_field, SafetyTier, Tool};

pub struct RunTests;

#[async_trait]
impl Tool for RunTests {
    fn name(&self) -> &str { "run_tests" }
    fn description(&self) -> &str { "Runs the project's test suite (auto-detects language)" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "command": { "type": "STRING", "description": "Custom test command (auto-detects if omitted)" }
            },
            "required": []
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let cmd = if let Some(custom) = extract_field(&args, "command") {
            custom
        } else {
            detect_test_command(working_dir).await
        };

        match Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
        {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let status = if out.status.success() { "PASSED" } else { "FAILED" };
                format!("[{status}] `{cmd}`\n{stdout}\n{stderr}")
            }
            Err(e) => format!("Test execution failed: {}", e),
        }
    }
}

/// Auto-detect the test command based on project files.
async fn detect_test_command(dir: &Path) -> String {
    // Rust
    if dir.join("Cargo.toml").exists() {
        return "cargo test".into();
    }
    // Node.js
    if dir.join("package.json").exists() {
        return "npm test".into();
    }
    // Python
    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        return "python -m pytest".into();
    }
    // Go
    if dir.join("go.mod").exists() {
        return "go test ./...".into();
    }
    // Fallback
    "echo 'No test framework detected'".into()
}
