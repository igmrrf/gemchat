use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use super::{extract_field, SafetyTier, Tool};

pub struct RunCommand;

#[async_trait]
impl Tool for RunCommand {
    fn name(&self) -> &str { "run_command" }
    fn description(&self) -> &str { "Executes a terminal command via sh -c" }
    fn parameters(&self) -> Value {
        json!({
            "type": "OBJECT",
            "properties": {
                "command": { "type": "STRING", "description": "The command to run" }
            },
            "required": ["command"]
        })
    }
    fn safety_tier(&self) -> SafetyTier { SafetyTier::Dangerous }

    async fn execute(&self, args: Value, working_dir: &Path) -> String {
        let Some(cmd) = extract_field(&args, "command") else {
            return "Error: 'command' is required".into();
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
                let status = if out.status.success() { "OK" } else { "FAILED" };
                format!("[{status}]\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}")
            }
            Err(e) => format!("Failed to execute command: {}", e),
        }
    }
}
